//! One SMPP conversation: bind state, sequence numbers, request/response
//! matching and the automatic answers the protocol requires.
//!
//! It never reconnects and never re-binds. When the socket goes, the session
//! is `Closed` and stays closed until the user starts a new one.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::app::transport::{Transport, TransportEvent};
use crate::smpp::constants::{
    command_id, command_name, command_status, is_response, response_id_for, ESME_ROK,
};
use crate::smpp::decode::{decode_pdu, Pdu};
use crate::smpp::encode::{encode_pdu, PduInput};

pub const MAX_SEQUENCE: u32 = 0x7fff_ffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindMode {
    Transceiver,
    Transmitter,
    Receiver,
}

impl BindMode {
    fn command(self) -> &'static str {
        match self {
            BindMode::Transceiver => "bind_transceiver",
            BindMode::Transmitter => "bind_transmitter",
            BindMode::Receiver => "bind_receiver",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BindState {
    Unbound,
    Binding,
    Bound,
    Unbinding,
    Closed,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindOptions {
    pub system_id: String,
    pub password: String,
    pub system_type: String,
    pub interface_version: u8,
    pub mode: BindMode,
    #[serde(default)]
    pub addr_ton: u8,
    #[serde(default)]
    pub addr_npi: u8,
    #[serde(default)]
    pub address_range: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SessionOptions {
    /// Requests allowed in flight at once. A further send waits for a slot.
    pub window_size: usize,
    pub response_timeout: Duration,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    PduIn {
        pdu: Pdu,
        at: u64,
    },
    PduOut {
        pdu: Pdu,
        at: u64,
    },
    State {
        state: BindState,
        mode: Option<BindMode>,
    },
    /// A response matching no pending request, or a command we do not answer.
    Orphan(Pdu),
    Timeout {
        sequence_number: u32,
    },
    DecodeError {
        message: String,
        raw: Vec<u8>,
    },
    /// Bytes from `send_raw` that do not decode.
    RawOut {
        bytes: Vec<u8>,
        at: u64,
    },
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct Inner {
    state_value: Option<BindState>,
    mode: Option<BindMode>,
    next_sequence: u32,
    inflight: HashMap<u32, oneshot::Sender<Result<Pdu, String>>>,
    window_used: usize,
    waiters: VecDeque<oneshot::Sender<Result<(), String>>>,
    closing: bool,
}

impl Inner {
    fn state(&self) -> BindState {
        self.state_value.unwrap_or(BindState::Unbound)
    }
}

pub struct Session {
    transport: Arc<Transport>,
    inner: Arc<Mutex<Inner>>,
    events: mpsc::UnboundedSender<SessionEvent>,
    opts: SessionOptions,
}

impl Session {
    pub fn new(
        transport: Transport,
        transport_events: mpsc::UnboundedReceiver<TransportEvent>,
        opts: SessionOptions,
    ) -> (Self, mpsc::UnboundedReceiver<SessionEvent>) {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Mutex::new(Inner {
            next_sequence: 1,
            ..Default::default()
        }));
        let transport = Arc::new(transport);

        let session = Self {
            transport: transport.clone(),
            inner: inner.clone(),
            events: events_tx.clone(),
            opts,
        };

        spawn_dispatch(transport, inner, events_tx, transport_events);
        (session, events_rx)
    }

    pub fn state(&self) -> BindState {
        self.inner.lock().unwrap().state()
    }

    pub fn mode(&self) -> Option<BindMode> {
        self.inner.lock().unwrap().mode
    }

    /// Requests still waiting for a response.
    pub fn pending(&self) -> usize {
        self.inner.lock().unwrap().inflight.len()
    }

    pub fn set_next_sequence_for_test(&self, n: u32) {
        self.inner.lock().unwrap().next_sequence = n;
    }

    pub fn shutdown_transport_for_test(&self) {
        self.transport.shutdown();
    }

    pub async fn bind(&self, opts: BindOptions) -> Result<Pdu, String> {
        if self.state() != BindState::Unbound {
            return Err(format!("cannot bind while {:?}", self.state()));
        }
        set_state(
            &self.inner,
            &self.events,
            BindState::Binding,
            Some(opts.mode),
        );

        let input = PduInput::new(command_id(opts.mode.command()).expect("a bind command"), 0)
            .with_str("system_id", &opts.system_id)
            .with_str("password", &opts.password)
            .with_str("system_type", &opts.system_type)
            .with_num("interface_version", opts.interface_version as u32)
            .with_num("addr_ton", opts.addr_ton as u32)
            .with_num("addr_npi", opts.addr_npi as u32)
            .with_str("address_range", &opts.address_range);

        let resp = match self.request(input, false).await {
            Ok(resp) => resp,
            Err(e) => {
                set_state(&self.inner, &self.events, BindState::Unbound, None);
                return Err(e);
            }
        };

        if resp.command_status != ESME_ROK {
            set_state(&self.inner, &self.events, BindState::Unbound, None);
            return Err(format!(
                "bind refused: {}",
                crate::smpp::constants::command_status_name(resp.command_status)
            ));
        }

        set_state(&self.inner, &self.events, BindState::Bound, Some(opts.mode));
        Ok(resp)
    }

    pub async fn unbind(&self) -> Result<Pdu, String> {
        if self.state() != BindState::Bound {
            return Err(format!("cannot unbind while {:?}", self.state()));
        }
        let mode = self.mode();
        set_state(&self.inner, &self.events, BindState::Unbinding, mode);

        let result = self
            .request(
                PduInput::new(command_id("unbind").expect("unbind"), 0),
                false,
            )
            .await;
        set_state(&self.inner, &self.events, BindState::Closed, None);
        result
    }

    /// Send a request and resolve with its response.
    ///
    /// A non-zero command_status resolves — the caller needs to see it. Only a
    /// timeout or a lost connection fails.
    pub async fn send(&self, input: PduInput) -> Result<Pdu, String> {
        if self.state() != BindState::Bound {
            return Err(format!("not bound (state is {:?})", self.state()));
        }
        self.acquire_window_slot().await?;
        self.request(input, true).await
    }

    /// Put bytes on the wire exactly as given: no sequence number allocated,
    /// no response awaited, nothing validated. The hex composer's path.
    pub fn send_raw(&self, bytes: &[u8]) -> Result<(), String> {
        self.transport.write(bytes)?;
        match decode_pdu(bytes) {
            Ok(pdu) => {
                let _ = self.events.send(SessionEvent::PduOut { pdu, at: now_ms() });
            }
            Err(_) => {
                let _ = self.events.send(SessionEvent::RawOut {
                    bytes: bytes.to_vec(),
                    at: now_ms(),
                });
            }
        }
        Ok(())
    }

    async fn acquire_window_slot(&self) -> Result<(), String> {
        let rx = {
            let mut inner = self.inner.lock().unwrap();
            if inner.closing {
                return Err("connection closed".into());
            }
            if inner.window_used < self.opts.window_size {
                inner.window_used += 1;
                return Ok(());
            }
            let (tx, rx) = oneshot::channel();
            inner.waiters.push_back(tx);
            rx
        };
        rx.await.unwrap_or_else(|_| Err("connection closed".into()))
    }

    fn release_window_slot(&self) {
        release_slot(&self.inner);
    }

    async fn request(&self, input: PduInput, windowed: bool) -> Result<Pdu, String> {
        let (sequence_number, rx) = {
            let mut inner = self.inner.lock().unwrap();
            let seq = inner.next_sequence;
            inner.next_sequence = if seq >= MAX_SEQUENCE { 1 } else { seq + 1 };
            let (tx, rx) = oneshot::channel();
            inner.inflight.insert(seq, tx);
            (seq, rx)
        };

        let bytes = encode_pdu(&PduInput {
            sequence_number,
            ..input.clone()
        });

        if let Err(e) = self.transport.write(&bytes) {
            self.inner.lock().unwrap().inflight.remove(&sequence_number);
            if windowed {
                self.release_window_slot();
            }
            return Err(e);
        }

        if let Ok(pdu) = decode_pdu(&bytes) {
            let _ = self.events.send(SessionEvent::PduOut { pdu, at: now_ms() });
        }

        let outcome = match tokio::time::timeout(self.opts.response_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                self.inner.lock().unwrap().inflight.remove(&sequence_number);
                let _ = self.events.send(SessionEvent::Timeout { sequence_number });
                Err(format!(
                    "{} seq {sequence_number} timed out after {}ms",
                    command_name(input.command_id),
                    self.opts.response_timeout.as_millis()
                ))
            }
        };

        if windowed {
            self.release_window_slot();
        }
        outcome
    }
}

fn set_state(
    inner: &Arc<Mutex<Inner>>,
    events: &mpsc::UnboundedSender<SessionEvent>,
    state: BindState,
    mode: Option<BindMode>,
) {
    {
        let mut guard = inner.lock().unwrap();
        guard.state_value = Some(state);
        guard.mode = mode;
    }
    let _ = events.send(SessionEvent::State { state, mode });
}

fn release_slot(inner: &Arc<Mutex<Inner>>) {
    let waiter = {
        let mut guard = inner.lock().unwrap();
        guard.window_used = guard.window_used.saturating_sub(1);
        // Once the connection has gone there is no slot worth granting: a
        // waiter woken here would write to a dead socket.
        if guard.closing {
            None
        } else {
            guard.waiters.pop_front().inspect(|_| {
                // The slot is handed straight to the waiter, so it stays taken.
                guard.window_used += 1;
            })
        }
    };
    if let Some(tx) = waiter {
        let _ = tx.send(Ok(()));
    }
}

fn spawn_dispatch(
    transport: Arc<Transport>,
    inner: Arc<Mutex<Inner>>,
    events: mpsc::UnboundedSender<SessionEvent>,
    mut transport_events: mpsc::UnboundedReceiver<TransportEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = transport_events.recv().await {
            match event {
                TransportEvent::Pdu(raw) => {
                    let pdu = match decode_pdu(&raw) {
                        Ok(pdu) => pdu,
                        Err(e) => {
                            let _ = events.send(SessionEvent::DecodeError {
                                message: e.message,
                                raw: e.raw,
                            });
                            continue;
                        }
                    };

                    let _ = events.send(SessionEvent::PduIn {
                        pdu: pdu.clone(),
                        at: now_ms(),
                    });

                    if is_response(pdu.command_id) {
                        let waiting = inner.lock().unwrap().inflight.remove(&pdu.sequence_number);
                        match waiting {
                            Some(tx) => {
                                let _ = tx.send(Ok(pdu));
                            }
                            None => {
                                let _ = events.send(SessionEvent::Orphan(pdu));
                            }
                        }
                        continue;
                    }

                    answer(&transport, &inner, &events, &pdu);
                }
                TransportEvent::DecodeError { message, raw } => {
                    let _ = events.send(SessionEvent::DecodeError { message, raw });
                }
                TransportEvent::Closed(reason) => {
                    close_everything(&inner, &events, reason);
                    break;
                }
                _ => {}
            }
        }
    });
}

/// The automatic answers the protocol requires of a bound ESME.
fn answer(
    transport: &Arc<Transport>,
    inner: &Arc<Mutex<Inner>>,
    events: &mpsc::UnboundedSender<SessionEvent>,
    pdu: &Pdu,
) {
    let name = command_name(pdu.command_id);
    let reply = |events: &mpsc::UnboundedSender<SessionEvent>| {
        let bytes = encode_pdu(
            &PduInput::new(response_id_for(pdu.command_id), pdu.sequence_number)
                .with_status(command_status("ESME_ROK").unwrap_or(0)),
        );
        if transport.write(&bytes).is_ok() {
            if let Ok(out) = decode_pdu(&bytes) {
                let _ = events.send(SessionEvent::PduOut {
                    pdu: out,
                    at: now_ms(),
                });
            }
        }
    };

    match name.as_str() {
        "deliver_sm" | "data_sm" | "enquire_link" => reply(events),
        "unbind" => {
            reply(events);
            set_state(inner, events, BindState::Closed, None);
        }
        // Defined with no response.
        "alert_notification" => {}
        // An id the protocol does not define, or a request this client does
        // not answer. It goes in the trace as an orphan; the session stays
        // bound, and nothing is sent back, because a test client should not
        // invent traffic the user did not ask for.
        _ => {
            let _ = events.send(SessionEvent::Orphan(pdu.clone()));
        }
    }
}

fn close_everything(
    inner: &Arc<Mutex<Inner>>,
    events: &mpsc::UnboundedSender<SessionEvent>,
    reason: Option<String>,
) {
    let message = match reason {
        Some(r) => format!("connection closed: {r}"),
        None => "connection closed".to_string(),
    };

    // Mark the session closing before settling anything, so releasing a slot
    // cannot wake a parked send onto a socket that has already gone.
    let (waiters, inflight, was_closed) = {
        let mut guard = inner.lock().unwrap();
        guard.closing = true;
        let waiters: Vec<_> = guard.waiters.drain(..).collect();
        let inflight: Vec<_> = guard.inflight.drain().map(|(_, tx)| tx).collect();
        guard.window_used = 0;
        let was_closed = guard.state() == BindState::Closed;
        (waiters, inflight, was_closed)
    };

    // Fail whatever is queued for a slot, rather than dropping the callbacks
    // and leaving those sends unsettled for good.
    for waiter in waiters {
        let _ = waiter.send(Err(message.clone()));
    }
    for tx in inflight {
        let _ = tx.send(Err(message.clone()));
    }

    if !was_closed {
        set_state(inner, events, BindState::Closed, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::{command_id, command_status, InterfaceVersion};
    use crate::testing::fake_smsc::FakeSmsc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    fn bind_opts() -> BindOptions {
        BindOptions {
            system_id: "esme".into(),
            password: "pw".into(),
            system_type: "SMPP".into(),
            interface_version: InterfaceVersion::V34,
            mode: BindMode::Transceiver,
            addr_ton: 0,
            addr_npi: 0,
            address_range: String::new(),
        }
    }

    async fn open(
        port: u16,
        window_size: usize,
        response_timeout_ms: u64,
    ) -> (Session, mpsc::UnboundedReceiver<SessionEvent>) {
        let (transport, events) =
            crate::app::transport::Transport::connect(crate::app::transport::TransportOptions {
                host: "127.0.0.1".into(),
                port,
                tls: Default::default(),
                connect_timeout: Duration::from_secs(3),
            })
            .await
            .unwrap();
        Session::new(
            transport,
            events,
            SessionOptions {
                window_size,
                response_timeout: Duration::from_millis(response_timeout_ms),
            },
        )
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
    async fn binds_as_transceiver_and_reports_state_bound() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        assert_eq!(session.state(), BindState::Unbound);

        let resp = session.bind(bind_opts()).await.unwrap();
        assert_eq!(resp.command_id, id("bind_transceiver_resp"));
        assert_eq!(session.state(), BindState::Bound);
        assert_eq!(session.mode(), Some(BindMode::Transceiver));
    }

    #[tokio::test]
    async fn sends_bind_transmitter_for_transmitter_mode() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session
            .bind(BindOptions {
                mode: BindMode::Transmitter,
                ..bind_opts()
            })
            .await
            .unwrap();
        assert_eq!(smsc.received()[0].command_id, id("bind_transmitter"));
    }

    #[tokio::test]
    async fn sends_bind_receiver_for_receiver_mode() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session
            .bind(BindOptions {
                mode: BindMode::Receiver,
                ..bind_opts()
            })
            .await
            .unwrap();
        assert_eq!(smsc.received()[0].command_id, id("bind_receiver"));
    }

    #[tokio::test]
    async fn sends_the_credentials_and_version_it_was_given() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        let p = &smsc.received()[0];
        assert_eq!(p.str_field("system_id").unwrap(), "esme");
        assert_eq!(p.str_field("password").unwrap(), "pw");
        assert_eq!(p.str_field("system_type").unwrap(), "SMPP");
        assert_eq!(
            p.u32_field("interface_version"),
            Some(InterfaceVersion::V34 as u32)
        );
    }

    #[tokio::test]
    async fn reports_state_unbound_and_fails_when_the_bind_is_refused() {
        let smsc = FakeSmsc::new();
        smsc.on("bind_transceiver", |p| {
            Some(vec![crate::smpp::encode::PduInput::new(
                command_id("bind_transceiver_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RBINDFAIL").unwrap())])
        });
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;

        let err = session.bind(bind_opts()).await.unwrap_err();
        assert!(err.contains("ESME_RBINDFAIL"), "{err}");
        assert_eq!(session.state(), BindState::Unbound);
    }

    #[tokio::test]
    async fn unbinds_and_reports_state_closed() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();
        session.unbind().await.unwrap();
        assert_eq!(session.state(), BindState::Closed);
    }

    #[tokio::test]
    async fn allocates_increasing_sequence_numbers() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();
        session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();
        session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();

        let seqs: Vec<u32> = smsc.received().iter().map(|p| p.sequence_number).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn resolves_send_with_the_response_that_matches_the_sequence_number() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        let resp = session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();
        assert_eq!(resp.command_id, id("submit_sm_resp"));
        assert!(resp.str_field("message_id").is_some());
    }

    #[tokio::test]
    async fn resolves_on_a_non_zero_command_status_rather_than_failing() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            Some(vec![crate::smpp::encode::PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RTHROTTLED").unwrap())])
        });
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        let resp = session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();
        assert_eq!(
            resp.command_status,
            command_status("ESME_RTHROTTLED").unwrap()
        );
    }

    #[tokio::test]
    async fn refuses_to_send_when_it_is_not_bound() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        let err = session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap_err();
        assert!(err.to_lowercase().contains("not bound"), "{err}");
    }

    #[tokio::test]
    async fn sends_raw_bytes_verbatim() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        let bytes = crate::smpp::encode::encode_pdu(&crate::smpp::encode::PduInput::new(
            id("enquire_link"),
            4242,
        ));
        session.send_raw(&bytes).unwrap();
        until(|| smsc.received().len() == 2).await;
        assert_eq!(smsc.received()[1].sequence_number, 4242);
    }

    #[tokio::test]
    async fn answers_deliver_sm_automatically() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        smsc.send(
            crate::smpp::encode::PduInput::new(id("deliver_sm"), 77)
                .with_num("esm_class", 4)
                .with_bytes("short_message", b"id:A stat:DELIVRD".to_vec()),
        );
        until(|| smsc.received().len() == 2).await;

        assert_eq!(smsc.received()[1].command_id, id("deliver_sm_resp"));
        assert_eq!(smsc.received()[1].sequence_number, 77);
        assert_eq!(smsc.received()[1].command_status, 0);
    }

    #[tokio::test]
    async fn answers_an_inbound_enquire_link() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        smsc.send(crate::smpp::encode::PduInput::new(id("enquire_link"), 55));
        until(|| smsc.received().len() == 2).await;
        assert_eq!(smsc.received()[1].command_id, id("enquire_link_resp"));
    }

    #[tokio::test]
    async fn moves_to_closed_on_an_inbound_unbind() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        smsc.send(crate::smpp::encode::PduInput::new(id("unbind"), 66));
        until(|| session.state() == BindState::Closed).await;
        assert_eq!(smsc.received()[1].command_id, id("unbind_resp"));
    }

    #[tokio::test]
    async fn emits_orphan_for_a_response_that_matches_no_pending_request() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, mut events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        smsc.send(
            crate::smpp::encode::PduInput::new(id("submit_sm_resp"), 9999)
                .with_str("message_id", "GHOST"),
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .unwrap()
                .unwrap();
            if let SessionEvent::Orphan(pdu) = event {
                assert_eq!(pdu.str_field("message_id").unwrap(), "GHOST");
                break;
            }
        }
        assert_eq!(session.state(), BindState::Bound);
    }

    #[tokio::test]
    async fn emits_orphan_for_an_unknown_command_and_stays_bound() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, mut events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        smsc.send(crate::smpp::encode::PduInput::new(0x4242, 3).with_bytes("body", vec![1, 2]));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .unwrap()
                .unwrap();
            if let SessionEvent::Orphan(pdu) = event {
                assert_eq!(pdu.command_id, 0x4242);
                break;
            }
        }
        assert_eq!(session.state(), BindState::Bound);
        // Nothing was invented in reply: only our bind reached the SMSC.
        assert_eq!(smsc.received().len(), 1);
    }

    #[tokio::test]
    async fn does_not_re_bind_after_the_socket_closes() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();

        session.shutdown_transport_for_test();
        until(|| session.state() == BindState::Closed).await;

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(session.state(), BindState::Closed);
    }

    #[tokio::test]
    async fn fails_send_after_the_response_timeout() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |_| None);
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 150).await;
        session.bind(bind_opts()).await.unwrap();

        let err = session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap_err();
        assert!(err.to_lowercase().contains("timed out"), "{err}");
        assert_eq!(session.pending(), 0);
    }

    #[tokio::test]
    async fn frees_the_window_slot_after_a_timeout() {
        let smsc = FakeSmsc::new();
        let answered = Arc::new(AtomicUsize::new(0));
        let seen = answered.clone();
        smsc.on("submit_sm", move |p| {
            // Never answer the first; answer the second normally.
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                None
            } else {
                Some(vec![crate::smpp::encode::PduInput::new(
                    command_id("submit_sm_resp").unwrap(),
                    p.sequence_number,
                )
                .with_str("message_id", "OK")])
            }
        });
        let port = smsc.listen().await;
        let (session, _events) = open(port, 1, 150).await;
        session.bind(bind_opts()).await.unwrap();

        let session = Arc::new(session);
        let first = {
            let s = session.clone();
            tokio::spawn(async move {
                s.send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
                    .await
            })
        };
        let second = {
            let s = session.clone();
            tokio::spawn(async move {
                s.send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
                    .await
            })
        };

        assert!(first
            .await
            .unwrap()
            .unwrap_err()
            .to_lowercase()
            .contains("timed out"));
        assert_eq!(
            second
                .await
                .unwrap()
                .unwrap()
                .str_field("message_id")
                .unwrap(),
            "OK"
        );
    }

    #[tokio::test]
    async fn fails_sends_still_queued_for_a_window_slot_when_the_connection_drops() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |_| None); // the window fills and stays full
        let port = smsc.listen().await;
        let (session, _events) = open(port, 2, 30_000).await;
        session.bind(bind_opts()).await.unwrap();

        let session = Arc::new(session);
        let mut sends = Vec::new();
        for _ in 0..6 {
            let s = session.clone();
            sends.push(tokio::spawn(async move {
                s.send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
                    .await
            }));
        }

        until(|| smsc.received().len() >= 3).await;
        session.shutdown_transport_for_test();

        for handle in sends {
            let err = handle.await.unwrap().unwrap_err();
            assert!(err.to_lowercase().contains("connection closed"), "{err}");
        }
    }

    #[tokio::test]
    async fn wraps_the_sequence_number_at_the_maximum() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (session, _events) = open(port, 10, 2000).await;
        session.bind(bind_opts()).await.unwrap();
        session.set_next_sequence_for_test(MAX_SEQUENCE);

        session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();
        session
            .send(crate::smpp::encode::PduInput::new(id("submit_sm"), 0))
            .await
            .unwrap();

        let seqs: Vec<u32> = smsc.received()[1..]
            .iter()
            .map(|p| p.sequence_number)
            .collect();
        assert_eq!(seqs, vec![MAX_SEQUENCE, 1]);
    }
}
