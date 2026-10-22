//! The socket, and nothing else.
//!
//! It frames the byte stream into PDUs and reports what happens. It does not
//! reconnect — ever. The connection state the UI shows is the real one, and a
//! dropped bind stays dropped until the user says otherwise.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::smpp::frame::{PduFramer, Pending};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TlsOptions {
    pub enabled: bool,
    /// A CA bundle to trust in addition to the system store.
    #[serde(skip)]
    pub ca: Option<Vec<u8>>,
    /// false accepts a certificate the client cannot verify.
    pub reject_unauthorized: bool,
    /// The SNI name to send, when it differs from `host`.
    pub servername: Option<String>,
    #[serde(skip)]
    pub client_cert: Option<Vec<u8>>,
    #[serde(skip)]
    pub client_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct TransportOptions {
    pub host: String,
    pub port: u16,
    pub tls: TlsOptions,
    pub connect_timeout: Duration,
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// One complete PDU, framed by command_length.
    Pdu(Vec<u8>),
    /// A framing failure. The PDUs that preceded it in the same read have
    /// already been delivered as `Pdu` events.
    DecodeError {
        message: String,
        raw: Vec<u8>,
    },
    Closed(Option<String>),
    BytesIn(usize),
    BytesOut(usize),
}

/// What the writer task is asked to do. A close is its own message rather
/// than an empty payload: when the two were the same thing, sending no bytes
/// tore the connection down with no error and no explanation.
#[derive(Debug)]
enum Outbound {
    Bytes(Vec<u8>),
    Shutdown,
}

#[derive(Debug)]
pub struct Transport {
    out: mpsc::UnboundedSender<Outbound>,
    connected: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
}

#[derive(Debug)]
struct AcceptAnyServer;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn client_config(tls: &TlsOptions) -> Result<rustls::ClientConfig, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = rustls::ClientConfig::builder();

    let builder = if tls.reject_unauthorized {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(ca) = &tls.ca {
            for cert in rustls_pemfile::certs(&mut ca.as_slice()).flatten() {
                roots
                    .add(cert)
                    .map_err(|e| format!("the CA bundle could not be used: {e}"))?;
            }
        }
        builder.with_root_certificates(roots)
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServer))
    };

    match (&tls.client_cert, &tls.client_key) {
        (Some(cert), Some(key)) => {
            let chain = rustls_pemfile::certs(&mut cert.as_slice())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("the client certificate could not be read: {e}"))?;
            let key = rustls_pemfile::private_key(&mut key.as_slice())
                .map_err(|e| format!("the client key could not be read: {e}"))?
                .ok_or_else(|| "the client key file held no private key".to_string())?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|e| format!("the client certificate was refused: {e}"))
        }
        _ => Ok(builder.with_no_client_auth()),
    }
}

impl Transport {
    pub async fn connect(
        opts: TransportOptions,
    ) -> Result<(Self, mpsc::UnboundedReceiver<TransportEvent>), String> {
        let address = (opts.host.clone(), opts.port);

        let socket = tokio::time::timeout(opts.connect_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| {
                format!(
                    "connection to {}:{} timed out after {}ms",
                    opts.host,
                    opts.port,
                    opts.connect_timeout.as_millis()
                )
            })?
            .map_err(|e| format!("could not connect to {}:{}: {e}", opts.host, opts.port))?;

        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel::<Outbound>();
        let connected = Arc::new(AtomicBool::new(true));
        let pending = Arc::new(Mutex::new(Pending {
            buffered: 0,
            declared: None,
        }));

        if opts.tls.enabled {
            let name = opts
                .tls
                .servername
                .clone()
                .unwrap_or_else(|| opts.host.clone());
            let config = client_config(&opts.tls)?;
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let server_name = rustls::pki_types::ServerName::try_from(name.clone())
                .map_err(|_| format!("\"{name}\" is not a valid server name"))?
                .to_owned();
            let stream = connector
                .connect(server_name, socket)
                .await
                .map_err(|e| format!("the TLS handshake failed: {e}"))?;
            spawn_io(
                stream,
                events_tx,
                out_rx,
                connected.clone(),
                pending.clone(),
            );
        } else {
            spawn_io(
                socket,
                events_tx,
                out_rx,
                connected.clone(),
                pending.clone(),
            );
        }

        Ok((
            Self {
                out: out_tx,
                connected,
                pending,
            },
            events_rx,
        ))
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Bytes held by the framer and the length the pending PDU claims.
    pub fn pending(&self) -> Pending {
        *self.pending.lock().unwrap()
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if !self.is_connected() {
            return Err("the transport is not connected".into());
        }
        if bytes.is_empty() {
            return Err("there are no bytes to send".into());
        }
        self.out
            .send(Outbound::Bytes(bytes.to_vec()))
            .map_err(|_| "the transport is not connected".to_string())
    }

    /// Close the socket. The writer task is what announces it, so the flag is
    /// left alone here: clearing it first would make that task think somebody
    /// else had already reported the close, and nothing would be emitted.
    pub fn shutdown(&self) {
        let _ = self.out.send(Outbound::Shutdown);
    }
}

fn spawn_io<S>(
    stream: S,
    events: mpsc::UnboundedSender<TransportEvent>,
    mut out: mpsc::UnboundedReceiver<Outbound>,
    connected: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    {
        let events = events.clone();
        let connected = connected.clone();
        tokio::spawn(async move {
            while let Some(message) = out.recv().await {
                let Outbound::Bytes(bytes) = message else {
                    break;
                };
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
                // Counted after the write, so a failed one is not reported as
                // bytes that went out.
                let _ = events.send(TransportEvent::BytesOut(bytes.len()));
            }
            let _ = writer.shutdown().await;
            if connected.swap(false, Ordering::SeqCst) {
                let _ = events.send(TransportEvent::Closed(None));
            }
        });
    }

    tokio::spawn(async move {
        let mut framer = PduFramer::new();
        let mut buf = vec![0u8; 16384];

        loop {
            let read = match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let _ = events.send(TransportEvent::BytesIn(read));

            match framer.push(&buf[..read]) {
                Ok(raws) => {
                    for raw in raws {
                        let _ = events.send(TransportEvent::Pdu(raw));
                    }
                }
                Err(error) => {
                    // PDUs the framer had already read out of this same chunk
                    // are good traffic: deliver them before reporting the bad
                    // one, or they are lost with it.
                    for raw in error.framed {
                        let _ = events.send(TransportEvent::Pdu(raw));
                    }
                    framer.reset();
                    let _ = events.send(TransportEvent::DecodeError {
                        message: error.message,
                        raw: error.raw,
                    });
                }
            }

            *pending.lock().unwrap() = framer.pending();
        }

        if connected.swap(false, Ordering::SeqCst) {
            let _ = events.send(TransportEvent::Closed(None));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::command_id;
    use crate::smpp::decode::decode_pdu;
    use crate::smpp::encode::{encode_pdu, PduInput};
    use crate::testing::fake_smsc::{
        test_ca, test_client_cert, test_client_key, FakeSmsc, TlsMaterial,
    };
    use std::time::Duration;

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    fn opts(port: u16, tls: TlsOptions) -> TransportOptions {
        TransportOptions {
            host: "127.0.0.1".into(),
            port,
            tls,
            connect_timeout: Duration::from_secs(3),
        }
    }

    fn plain() -> TlsOptions {
        TlsOptions {
            enabled: false,
            reject_unauthorized: true,
            ..Default::default()
        }
    }

    /// Drains the event stream into a vector a test can assert on.
    struct Watch {
        pdus: Arc<Mutex<Vec<Vec<u8>>>>,
        errors: Arc<Mutex<Vec<String>>>,
        closes: Arc<Mutex<Vec<Option<String>>>>,
    }

    fn watch(mut events: tokio::sync::mpsc::UnboundedReceiver<TransportEvent>) -> Watch {
        let w = Watch {
            pdus: Arc::new(Mutex::new(Vec::new())),
            errors: Arc::new(Mutex::new(Vec::new())),
            closes: Arc::new(Mutex::new(Vec::new())),
        };
        let (pdus, errors, closes) = (w.pdus.clone(), w.errors.clone(), w.closes.clone());
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    TransportEvent::Pdu(raw) => pdus.lock().unwrap().push(raw),
                    TransportEvent::DecodeError { message, .. } => {
                        errors.lock().unwrap().push(message)
                    }
                    TransportEvent::Closed(reason) => closes.lock().unwrap().push(reason),
                    _ => {}
                }
            }
        });
        w
    }

    async fn until(mut predicate: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(
                std::time::Instant::now() < deadline,
                "condition never became true"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn connects_over_plain_tcp_and_emits_framed_pdus() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (transport, events) = Transport::connect(opts(port, plain())).await.unwrap();
        let seen = watch(events);

        transport
            .write(&encode_pdu(&PduInput::new(id("enquire_link"), 1)))
            .unwrap();
        until(|| seen.pdus.lock().unwrap().len() == 1).await;

        let raw = seen.pdus.lock().unwrap()[0].clone();
        assert_eq!(
            decode_pdu(&raw).unwrap().command_id,
            id("enquire_link_resp")
        );
    }

    #[tokio::test]
    async fn delivers_the_good_pdus_that_shared_a_read_with_a_malformed_one() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (transport, events) = Transport::connect(opts(port, plain())).await.unwrap();
        let seen = watch(events);
        smsc.wait_for_connection().await;

        let good1 = encode_pdu(&PduInput::new(id("deliver_sm"), 11).with_num("esm_class", 4));
        let good2 = encode_pdu(&PduInput::new(id("deliver_sm"), 12).with_num("esm_class", 4));
        let mut chunk = good1;
        chunk.extend(good2);
        chunk.extend(vec![0u8, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        smsc.send_raw(&chunk);

        until(|| seen.errors.lock().unwrap().len() == 1).await;

        let seqs: Vec<u32> = seen
            .pdus
            .lock()
            .unwrap()
            .iter()
            .map(|r| decode_pdu(r).unwrap().sequence_number)
            .collect();
        assert_eq!(seqs, vec![11, 12]);
        assert!(transport.is_connected());
    }

    #[tokio::test]
    async fn reports_what_the_framer_is_still_holding() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (transport, _events) = Transport::connect(opts(port, plain())).await.unwrap();
        smsc.wait_for_connection().await;

        let whole = encode_pdu(&PduInput::new(id("deliver_sm"), 5).with_num("esm_class", 4));
        smsc.send_raw(&whole[..20]);
        until(|| transport.pending().buffered == 20).await;

        assert_eq!(transport.pending().declared, Some(whole.len() as u32));
    }

    #[tokio::test]
    async fn an_empty_write_is_refused_rather_than_closing_the_socket() {
        // An empty payload used to be how `shutdown` signalled the writer, so
        // sending no bytes silently tore the connection down: a user who
        // cleared the hex box and pressed Send watched the bind disappear
        // with no error and no explanation.
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (transport, _events) = Transport::connect(opts(port, plain())).await.unwrap();
        smsc.wait_for_connection().await;

        let refused = transport.write(&[]);
        assert!(refused.is_err(), "an empty write must be refused");
        assert!(
            refused.unwrap_err().contains("no bytes"),
            "the refusal should say what was wrong"
        );

        // And the socket is still there.
        assert!(transport.is_connected());
        transport
            .write(&encode_pdu(&PduInput::new(id("enquire_link"), 1)))
            .unwrap();
        until(|| !smsc.received().is_empty()).await;
    }

    #[tokio::test]
    async fn emits_closed_once_on_shutdown_and_never_reconnects() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (transport, events) = Transport::connect(opts(port, plain())).await.unwrap();
        let seen = watch(events);
        smsc.wait_for_connection().await;

        transport.shutdown();
        until(|| seen.closes.lock().unwrap().len() == 1).await;
        assert!(!transport.is_connected());

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!transport.is_connected());
        assert_eq!(seen.closes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_connect_against_a_refused_port() {
        // Bind a port and release it, so we know for certain nothing is
        // listening there — guessing a free port races the other tests.
        let dead = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        assert!(Transport::connect(opts(dead, plain())).await.is_err());
    }

    #[tokio::test]
    async fn rejects_connect_after_the_timeout_against_an_unroutable_address() {
        // 192.0.2.0/24 is TEST-NET-1: reserved for documentation, never routed.
        let err = Transport::connect(TransportOptions {
            host: "192.0.2.1".into(),
            port: 2775,
            tls: plain(),
            connect_timeout: Duration::from_millis(200),
        })
        .await
        .unwrap_err();
        assert!(err.to_lowercase().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn refuses_an_untrusted_certificate_when_verification_is_on() {
        let smsc = FakeSmsc::with_tls(TlsMaterial::test_server(), false);
        let port = smsc.listen().await;
        let tls = TlsOptions {
            enabled: true,
            reject_unauthorized: true,
            servername: Some("localhost".into()),
            ..Default::default()
        };
        assert!(Transport::connect(opts(port, tls)).await.is_err());
    }

    #[tokio::test]
    async fn accepts_an_untrusted_certificate_when_verification_is_off() {
        let smsc = FakeSmsc::with_tls(TlsMaterial::test_server(), false);
        let port = smsc.listen().await;
        let tls = TlsOptions {
            enabled: true,
            reject_unauthorized: false,
            servername: Some("localhost".into()),
            ..Default::default()
        };
        let (transport, events) = Transport::connect(opts(port, tls)).await.unwrap();
        let seen = watch(events);

        transport
            .write(&encode_pdu(&PduInput::new(id("enquire_link"), 1)))
            .unwrap();
        until(|| seen.pdus.lock().unwrap().len() == 1).await;
    }

    #[tokio::test]
    async fn accepts_the_certificate_when_its_ca_is_supplied() {
        let smsc = FakeSmsc::with_tls(TlsMaterial::test_server(), false);
        let port = smsc.listen().await;
        let tls = TlsOptions {
            enabled: true,
            reject_unauthorized: true,
            ca: Some(test_ca()),
            servername: Some("localhost".into()),
            ..Default::default()
        };
        let (transport, _events) = Transport::connect(opts(port, tls)).await.unwrap();
        assert!(transport.is_connected());
    }

    #[tokio::test]
    async fn presents_a_client_certificate_when_the_server_requests_one() {
        let smsc = FakeSmsc::with_tls(TlsMaterial::test_server(), true);
        let port = smsc.listen().await;
        let tls = TlsOptions {
            enabled: true,
            reject_unauthorized: true,
            ca: Some(test_ca()),
            servername: Some("localhost".into()),
            client_cert: Some(test_client_cert()),
            client_key: Some(test_client_key()),
        };
        let (transport, events) = Transport::connect(opts(port, tls)).await.unwrap();
        let seen = watch(events);

        transport
            .write(&encode_pdu(&PduInput::new(id("enquire_link"), 1)))
            .unwrap();
        until(|| seen.pdus.lock().unwrap().len() == 1).await;
    }
}
