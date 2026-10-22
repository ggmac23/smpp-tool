//! A loopback SMSC for the test suite.
//!
//! This is a test fixture, not a product feature: the client is the product,
//! and it needs something to talk to. Handlers decide what comes back, so a
//! test can produce a throttle, a bind failure, or silence.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::smpp::constants::{command_name, command_status, response_id_for};
use crate::smpp::decode::{decode_pdu, Pdu};
use crate::smpp::encode::{encode_pdu, PduInput};
use crate::smpp::frame::PduFramer;

pub type Handler = Arc<dyn Fn(&Pdu) -> Option<Vec<PduInput>> + Send + Sync>;

/// Key material for a TLS listener, named rather than carried: a private key
/// is deliberately not Clone, and each connection reads it afresh.
#[derive(Clone, Copy)]
pub struct TlsMaterial {
    pub cert_file: &'static str,
    pub key_file: &'static str,
}

fn fixture(path: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("no parent of src-tauri")
        .join("tests/fixtures/tls");
    std::fs::read(root.join(path)).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

impl TlsMaterial {
    pub fn test_server() -> Self {
        Self {
            cert_file: "server.crt",
            key_file: "server.key",
        }
    }

    fn load(
        &self,
    ) -> (
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    ) {
        let certs = rustls_pemfile::certs(&mut fixture(self.cert_file).as_slice())
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| panic!("reading {}: {e}", self.cert_file));
        let key = rustls_pemfile::private_key(&mut fixture(self.key_file).as_slice())
            .unwrap_or_else(|e| panic!("reading {}: {e}", self.key_file))
            .unwrap_or_else(|| panic!("no private key in {}", self.key_file));
        (certs, key)
    }
}

pub fn test_ca() -> Vec<u8> {
    fixture("ca.crt")
}

pub fn test_client_cert() -> Vec<u8> {
    fixture("client.crt")
}

pub fn test_client_key() -> Vec<u8> {
    fixture("client.key")
}

/// A connector for the fixture's own tests. `trust_ca` false accepts anything.
pub fn test_tls_connector(trust_ca: bool) -> tokio_rustls::TlsConnector {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = if trust_ca {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut test_ca().as_slice()).flatten() {
            let _ = roots.add(cert);
        }
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServer))
            .with_no_client_auth()
    };

    tokio_rustls::TlsConnector::from(Arc::new(config))
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

#[derive(Default)]
struct Shared {
    received: Vec<Pdu>,
    handlers: HashMap<String, Handler>,
    next_message_id: u32,
    connections: Vec<mpsc::UnboundedSender<Vec<u8>>>,
}

pub struct FakeSmsc {
    shared: Arc<Mutex<Shared>>,
    tls: Option<TlsMaterial>,
    request_client_cert: bool,
}

impl FakeSmsc {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                next_message_id: 1,
                ..Default::default()
            })),
            tls: None,
            request_client_cert: false,
        }
    }

    pub fn with_tls(material: TlsMaterial, request_client_cert: bool) -> Self {
        Self {
            tls: Some(material),
            request_client_cert,
            ..Self::new()
        }
    }

    pub fn on<F>(&self, command: &str, handler: F)
    where
        F: Fn(&Pdu) -> Option<Vec<PduInput>> + Send + Sync + 'static,
    {
        self.shared
            .lock()
            .unwrap()
            .handlers
            .insert(command.to_string(), Arc::new(handler));
    }

    pub fn received(&self) -> Vec<Pdu> {
        self.shared.lock().unwrap().received.clone()
    }

    pub fn connection_count(&self) -> usize {
        self.shared.lock().unwrap().connections.len()
    }

    pub async fn wait_for_connection(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.connection_count() == 0 {
            assert!(std::time::Instant::now() < deadline, "no client connected");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    /// Push a PDU at every connected client.
    pub fn send(&self, input: PduInput) {
        self.send_raw(&encode_pdu(&input));
    }

    /// Push arbitrary bytes at every connected client, malformed or not.
    pub fn send_raw(&self, bytes: &[u8]) {
        let shared = self.shared.lock().unwrap();
        for tx in &shared.connections {
            let _ = tx.send(bytes.to_vec());
        }
    }

    pub async fn listen(&self) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();

        let shared = self.shared.clone();
        let tls = self.tls;
        let request_client_cert = self.request_client_cert;

        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let shared = shared.clone();
                let tls = tls;

                tokio::spawn(async move {
                    match tls {
                        None => serve(socket, shared).await,
                        Some(material) => {
                            let _ = rustls::crypto::ring::default_provider().install_default();
                            let builder = rustls::ServerConfig::builder();
                            let config = if request_client_cert {
                                let mut roots = rustls::RootCertStore::empty();
                                for cert in
                                    rustls_pemfile::certs(&mut test_ca().as_slice()).flatten()
                                {
                                    let _ = roots.add(cert);
                                }
                                let verifier =
                                    rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                                        .allow_unauthenticated()
                                        .build()
                                        .expect("client verifier");
                                builder.with_client_cert_verifier(verifier)
                            } else {
                                builder.with_no_client_auth()
                            };
                            let (cert_chain, key) = material.load();
                            let Ok(config) = config.with_single_cert(cert_chain, key) else {
                                return;
                            };
                            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
                            if let Ok(stream) = acceptor.accept(socket).await {
                                serve(stream, shared).await;
                            }
                        }
                    }
                });
            }
        });

        port
    }
}

impl Default for FakeSmsc {
    fn default() -> Self {
        Self::new()
    }
}

fn default_reply(shared: &Arc<Mutex<Shared>>, pdu: &Pdu) -> Option<Vec<PduInput>> {
    let ok = |fields: Vec<(&str, String)>| {
        let mut input = PduInput::new(response_id_for(pdu.command_id), pdu.sequence_number)
            .with_status(command_status("ESME_ROK").unwrap());
        for (name, value) in fields {
            input = input.with_str(name, &value);
        }
        Some(vec![input])
    };

    match command_name(pdu.command_id).as_str() {
        "bind_transmitter" | "bind_receiver" | "bind_transceiver" => {
            ok(vec![("system_id", "FakeSmsc".into())])
        }
        "submit_sm" | "data_sm" | "submit_multi" | "broadcast_sm" => {
            let mut guard = shared.lock().unwrap();
            let id = format!("{:04X}", guard.next_message_id);
            guard.next_message_id += 1;
            drop(guard);
            ok(vec![("message_id", id)])
        }
        "query_sm" => ok(vec![(
            "message_id",
            pdu.str_field("message_id").unwrap_or_default(),
        )]),
        "cancel_sm" | "replace_sm" | "unbind" | "enquire_link" => ok(vec![]),
        // A response from the client, or a command with no reply of its own.
        _ => None,
    }
}

async fn serve<S>(stream: S, shared: Arc<Mutex<Shared>>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    shared.lock().unwrap().connections.push(tx.clone());

    let writer_task = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                return;
            }
        }
    });

    let mut framer = PduFramer::new();
    let mut buf = vec![0u8; 8192];

    while let Ok(read) = reader.read(&mut buf).await {
        if read == 0 {
            break;
        }

        let raws = match framer.push(&buf[..read]) {
            Ok(raws) => raws,
            Err(_) => {
                framer.reset();
                continue;
            }
        };

        for raw in raws {
            let Ok(pdu) = decode_pdu(&raw) else { continue };

            let handler = shared
                .lock()
                .unwrap()
                .handlers
                .get(&command_name(pdu.command_id))
                .cloned();

            let replies = match handler {
                Some(h) => h(&pdu),
                None => default_reply(&shared, &pdu),
            };

            shared.lock().unwrap().received.push(pdu);

            for reply in replies.unwrap_or_default() {
                let _ = tx.send(encode_pdu(&reply));
            }
        }
    }

    writer_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::{command_id, command_status};
    use crate::smpp::decode::decode_pdu;
    use crate::smpp::encode::{encode_pdu, PduInput};
    use crate::smpp::frame::PduFramer;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    /// A bare client: a socket plus a framer. Deliberately not the Session.
    struct BareClient {
        socket: TcpStream,
        framer: PduFramer,
        received: Vec<crate::smpp::decode::Pdu>,
    }

    impl BareClient {
        async fn connect(port: u16) -> Self {
            Self {
                socket: TcpStream::connect(("127.0.0.1", port)).await.unwrap(),
                framer: PduFramer::new(),
                received: Vec::new(),
            }
        }

        async fn send(&mut self, input: PduInput) {
            self.socket.write_all(&encode_pdu(&input)).await.unwrap();
        }

        async fn wait_for(&mut self, n: usize) {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut buf = [0u8; 4096];
            while self.received.len() < n {
                let read = tokio::time::timeout_at(deadline, self.socket.read(&mut buf))
                    .await
                    .unwrap_or_else(|_| panic!("timed out waiting for {n} PDU(s)"))
                    .unwrap();
                for raw in self.framer.push(&buf[..read]).unwrap() {
                    self.received.push(decode_pdu(&raw).unwrap());
                }
            }
        }
    }

    #[tokio::test]
    async fn accepts_a_bind_transceiver_and_answers_esme_rok() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;

        client
            .send(PduInput::new(id("bind_transceiver"), 1).with_str("system_id", "esme"))
            .await;
        client.wait_for(1).await;

        assert_eq!(client.received[0].command_id, id("bind_transceiver_resp"));
        assert_eq!(client.received[0].command_status, 0);
        assert_eq!(client.received[0].sequence_number, 1);
    }

    #[tokio::test]
    async fn records_every_pdu_it_received() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;

        client.send(PduInput::new(id("enquire_link"), 1)).await;
        client.send(PduInput::new(id("enquire_link"), 2)).await;
        client.wait_for(2).await;

        let names: Vec<u32> = smsc.received().iter().map(|p| p.command_id).collect();
        assert_eq!(names, vec![id("enquire_link"), id("enquire_link")]);
    }

    #[tokio::test]
    async fn answers_submit_sm_with_an_incrementing_message_id() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;

        client.send(PduInput::new(id("submit_sm"), 1)).await;
        client.send(PduInput::new(id("submit_sm"), 2)).await;
        client.wait_for(2).await;

        let ids: Vec<String> = client
            .received
            .iter()
            .map(|p| p.str_field("message_id").unwrap_or_default())
            .collect();
        assert_ne!(ids[0], ids[1]);
        assert!(ids.iter().all(|i| !i.is_empty()));
    }

    #[tokio::test]
    async fn sends_nothing_when_the_handler_says_so() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |_| None);
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;

        client.send(PduInput::new(id("submit_sm"), 1)).await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        assert!(client.received.is_empty());
        assert_eq!(smsc.received().len(), 1);
    }

    #[tokio::test]
    async fn lets_a_handler_choose_the_command_status() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RTHROTTLED").unwrap())])
        });
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;

        client.send(PduInput::new(id("submit_sm"), 1)).await;
        client.wait_for(1).await;
        assert_eq!(
            client.received[0].command_status,
            command_status("ESME_RTHROTTLED").unwrap()
        );
    }

    #[tokio::test]
    async fn delivers_an_unsolicited_deliver_sm_to_the_client() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;
        smsc.wait_for_connection().await;

        smsc.send(
            PduInput::new(id("deliver_sm"), 99)
                .with_num("esm_class", 4)
                .with_bytes("short_message", b"id:ABC stat:DELIVRD err:000".to_vec()),
        );
        client.wait_for(1).await;

        assert_eq!(client.received[0].command_id, id("deliver_sm"));
        assert_eq!(client.received[0].sequence_number, 99);
    }

    #[tokio::test]
    async fn pushes_malformed_bytes_with_send_raw() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let mut client = BareClient::connect(port).await;
        smsc.wait_for_connection().await;

        let mut bogus = vec![0u8; 20];
        bogus[..4].copy_from_slice(&20u32.to_be_bytes());
        bogus[4..8].copy_from_slice(&0x4242u32.to_be_bytes());
        smsc.send_raw(&bogus);
        client.wait_for(1).await;

        assert_eq!(client.received[0].command_id, 0x4242);
    }

    #[tokio::test]
    async fn reports_the_number_of_live_connections() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let _client = BareClient::connect(port).await;
        smsc.wait_for_connection().await;
        assert_eq!(smsc.connection_count(), 1);
    }

    #[tokio::test]
    async fn accepts_a_tls_connection_with_its_test_certificate() {
        let smsc = FakeSmsc::with_tls(TlsMaterial::test_server(), false);
        let port = smsc.listen().await;

        let connector = crate::testing::fake_smsc::test_tls_connector(true);
        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut tls = connector
            .connect("localhost".try_into().unwrap(), stream)
            .await
            .expect("TLS handshake failed");

        tls.write_all(&encode_pdu(&PduInput::new(id("enquire_link"), 1)))
            .await
            .unwrap();

        let mut buf = [0u8; 256];
        let read = tokio::time::timeout(std::time::Duration::from_secs(3), tls.read(&mut buf))
            .await
            .expect("timed out")
            .unwrap();
        let mut framer = PduFramer::new();
        let raws = framer.push(&buf[..read]).unwrap();
        assert_eq!(
            decode_pdu(&raws[0]).unwrap().command_id,
            id("enquire_link_resp")
        );
    }
}
