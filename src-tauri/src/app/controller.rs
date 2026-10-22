//! Everything the renderer can ask for, in one place.
//!
//! The controller owns the transport, the session, the trace, the burst
//! engine, the load tester and the profile store, and it is the only place
//! they are assembled. The renderer never sees a socket, a byte vector or a
//! PDU model: it sends intents and gets plain data back.
//!
//! Nothing here reconnects or re-binds on its own. A lost socket is reported
//! and stays lost until the user says otherwise.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::app::burst::{BurstConfig, BurstDone, BurstEvent, BurstRunner};
use crate::app::dlr::CorrelationConfig;
use crate::app::events::EventHub;
use crate::app::export::{to_csv, trace_to_text};
use crate::app::loadtest::{LoadEvent, LoadReport, LoadRunner, LoadTestConfig};
use crate::app::profiles::{Profile, ProfileStore};
use crate::app::session::{
    BindMode, BindOptions, BindState, Session, SessionEvent, SessionOptions,
};
use crate::app::trace::{Counter, Direction, TraceEntry, TraceStore, DEFAULT_TRACE_CAP};
use crate::app::transport::{TlsOptions, Transport, TransportOptions};
use crate::app::wire::{
    to_pdu_input, to_wire_burst_row, to_wire_pdu, BindRequest, ConnectRequest, ExportFormat,
    MeasureRequest, SendPduRequest, StartBurstRequest, StartLoadRequest, StatusReport,
    WireBurstRow, WirePdu,
};
use crate::smpp::constants::command_name;
use crate::smpp::decode::{decode_pdu, Pdu};
use crate::smpp::encode::encode_raw;
use crate::smpp::summary::{describe, summarise, DescribedField};
use crate::smpp::text::{encode_text, measure, Gsm7Packing, Measurement};

const TRACE_CSV_COLUMNS: &[&str] = &[
    "seq",
    "at",
    "direction",
    "commandName",
    "statusName",
    "sequenceNumber",
    "summary",
    "hex",
];

const BURST_CSV_COLUMNS: &[&str] = &[
    "id",
    "recipient",
    "body",
    "pdus",
    "messageId",
    "commandStatus",
    "statusName",
    "submitLatencyMs",
    "dlrState",
    "dlrLatencyMs",
];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    pub sequence_number: u32,
    pub command_status: u32,
}

/// A body, encoded, with the characters the coding could not hold named.
/// Those go out as question marks: a warning, never a refusal.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedBody {
    pub hex: String,
    pub unmappable: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSecrets {
    pub password: Option<String>,
    pub key_passphrase: Option<String>,
}

#[derive(Default)]
struct Live {
    session: Option<Arc<Session>>,
    burst: Option<Arc<BurstRunner>>,
    load: Option<Arc<LoadRunner>>,
    last_load_report: Option<LoadReport>,
    correlation: CorrelationConfig,
    /// Trace row ordinals, which are the controller's own numbering and not
    /// the protocol's sequence numbers.
    next_row: u64,
}

pub struct Controller {
    hub: Arc<EventHub>,
    profiles: ProfileStore,
    trace: Arc<Mutex<TraceStore>>,
    live: Arc<Mutex<Live>>,
}

impl Controller {
    pub fn new(hub: Arc<EventHub>, profiles: ProfileStore) -> Self {
        Self {
            hub,
            profiles,
            trace: Arc::new(Mutex::new(TraceStore::new(DEFAULT_TRACE_CAP))),
            live: Arc::new(Mutex::new(Live {
                next_row: 1,
                ..Default::default()
            })),
        }
    }

    /* ---------------------------------------------------------------- */
    /* connection                                                       */

    pub async fn connect(&self, req: ConnectRequest) -> Result<(), String> {
        // A second connect replaces the first rather than leaving a socket
        // nobody is reading.
        self.disconnect();

        let (transport, transport_events) = Transport::connect(TransportOptions {
            host: req.host.clone(),
            port: req.port,
            tls: tls_options(&req)?,
            connect_timeout: Duration::from_millis(req.connect_timeout_ms),
        })
        .await?;

        let (session, session_events) = Session::new(
            transport,
            transport_events,
            SessionOptions {
                window_size: req.window_size,
                response_timeout: Duration::from_millis(req.response_timeout_ms),
            },
        );

        {
            let mut live = self.live.lock().unwrap();
            live.session = Some(Arc::new(session));
            live.correlation = req.correlation;
        }

        self.pump_session(session_events);
        Ok(())
    }

    pub fn disconnect(&self) {
        let (session, burst) = {
            let mut live = self.live.lock().unwrap();
            live.burst = None;
            (live.session.take(), live.burst.clone())
        };
        if let Some(burst) = burst {
            burst.stop();
        }
        if let Some(session) = session {
            session.disconnect();
        }
    }

    pub async fn bind(&self, req: BindRequest) -> Result<BindAccepted, String> {
        let session = self.session()?;
        let resp = session
            .bind(BindOptions {
                system_id: req.system_id,
                password: req.password,
                system_type: req.system_type,
                interface_version: req.interface_version,
                mode: req.mode,
                addr_ton: req.addr_ton,
                addr_npi: req.addr_npi,
                address_range: req.address_range,
            })
            .await?;
        Ok(BindAccepted {
            system_id: resp.str_field("system_id").unwrap_or_default(),
        })
    }

    pub async fn unbind(&self) -> Result<(), String> {
        self.session()?.unbind().await.map(|_| ())
    }

    pub fn status(&self) -> StatusReport {
        let session = self.live.lock().unwrap().session.clone();
        let dropped = self.trace.lock().unwrap().dropped();

        match session {
            Some(session) => {
                let pending = session.framer_pending();
                StatusReport {
                    state: session.state(),
                    mode: session.mode(),
                    connected: session.is_connected(),
                    dropped,
                    pending_buffered: pending.buffered,
                    pending_declared: pending.declared,
                    inflight: session.pending(),
                }
            }
            None => StatusReport {
                state: BindState::Unbound,
                mode: None,
                connected: false,
                dropped,
                pending_buffered: 0,
                pending_declared: None,
                inflight: 0,
            },
        }
    }

    /* ---------------------------------------------------------------- */
    /* sending                                                          */

    pub async fn send_pdu(&self, req: SendPduRequest) -> Result<SendOutcome, String> {
        let resp = self.session()?.send(to_pdu_input(&req)).await?;
        Ok(SendOutcome {
            sequence_number: resp.sequence_number,
            command_status: resp.command_status,
        })
    }

    /// Hand-written bytes, sent exactly as typed. No sequence number is
    /// assigned and no response is matched: the point is to see what the SMSC
    /// makes of them.
    pub async fn send_raw(&self, hex: &str) -> Result<(), String> {
        let bytes = encode_raw(&crate::app::wire::hex_to_bytes(hex));
        self.session()?.send_raw(&bytes)
    }

    pub fn measure(&self, req: &MeasureRequest) -> Measurement {
        measure(&req.text, req.data_coding, req.mode, req.packing)
    }

    /// Encodes a message body, so the forms that are not the submit form can
    /// put bytes in an octet field without the renderer carrying a codec.
    pub fn encode_body(&self, text: &str, data_coding: u8, packing: Gsm7Packing) -> EncodedBody {
        let encoded = encode_text(text, data_coding, packing);
        EncodedBody {
            hex: crate::smpp::summary::hex(&encoded.bytes),
            unmappable: encoded.unmappable,
        }
    }

    /* ---------------------------------------------------------------- */
    /* burst                                                            */

    pub async fn start_burst(&self, req: StartBurstRequest) -> Result<BurstDone, String> {
        let session = self.session()?;
        let correlation = self.live.lock().unwrap().correlation;

        let (tx, rx) = mpsc::unbounded_channel();
        let runner = Arc::new(BurstRunner::new(session, correlation).with_events(tx));
        self.live.lock().unwrap().burst = Some(runner.clone());
        self.pump_burst(rx);

        runner
            .start(BurstConfig {
                recipients: req.recipients,
                rate_per_sec: req.rate_per_sec,
                template: to_pdu_input(&req.template),
                text: req.text,
                data_coding: req.data_coding,
                segmentation: req.segmentation,
                placement: req.placement,
                packing: req.packing,
                backoff_on_throttle: req.backoff_on_throttle,
                await_dlr: Duration::from_millis(req.await_dlr_ms),
            })
            .await
    }

    pub fn stop_burst(&self) {
        if let Some(burst) = self.live.lock().unwrap().burst.clone() {
            burst.stop();
        }
    }

    pub fn burst_rows(&self, from: usize, limit: usize) -> Vec<WireBurstRow> {
        let burst = self.live.lock().unwrap().burst.clone();
        match burst {
            Some(burst) => burst
                .rows()
                .iter()
                .skip(from)
                .take(limit)
                .map(to_wire_burst_row)
                .collect(),
            None => Vec::new(),
        }
    }

    /* ---------------------------------------------------------------- */
    /* load test                                                        */

    /// The load test opens its own connections: it is a separate exercise
    /// from the session the user is inspecting, and mixing them would make
    /// the trace useless at the rates it runs.
    pub async fn start_load_test(&self, req: StartLoadRequest) -> Result<LoadReport, String> {
        let (tx, rx) = mpsc::unbounded_channel();
        let runner = Arc::new(LoadRunner::new().with_events(tx));
        self.live.lock().unwrap().load = Some(runner.clone());
        self.pump_load(rx);

        let report = runner
            .start(LoadTestConfig {
                host: req.connect.host.clone(),
                port: req.connect.port,
                tls: tls_options(&req.connect)?,
                bind: BindOptions {
                    system_id: req.bind.system_id,
                    password: req.bind.password,
                    system_type: req.bind.system_type,
                    interface_version: req.bind.interface_version,
                    mode: req.bind.mode,
                    addr_ton: req.bind.addr_ton,
                    addr_npi: req.bind.addr_npi,
                    address_range: req.bind.address_range,
                },
                connections: req.connections,
                messages: req.messages,
                mode: req.mode,
                window_size: req.connect.window_size,
                response_timeout: Duration::from_millis(req.connect.response_timeout_ms),
                connect_timeout: Duration::from_millis(req.connect.connect_timeout_ms),
                template: to_pdu_input(&req.template),
                destination: req.destination,
                text: req.text,
                data_coding: req.data_coding,
                segmentation: req.segmentation,
                placement: req.placement,
                packing: req.packing,
            })
            .await?;

        self.live.lock().unwrap().last_load_report = Some(report.clone());
        Ok(report)
    }

    pub fn stop_load_test(&self) {
        if let Some(load) = self.live.lock().unwrap().load.clone() {
            load.stop();
        }
    }

    pub fn load_report(&self) -> Option<LoadReport> {
        self.live.lock().unwrap().last_load_report.clone()
    }

    /* ---------------------------------------------------------------- */
    /* trace                                                            */

    pub fn trace(&self, from: u64, limit: usize) -> Vec<WirePdu> {
        self.trace
            .lock()
            .unwrap()
            .entries(Some(from), Some(limit))
            .iter()
            .map(to_wire_pdu)
            .collect()
    }

    pub fn pdu_fields(&self, seq: u64) -> Vec<DescribedField> {
        let Some(raw) = self.raw_at(seq) else {
            return Vec::new();
        };
        match decode_pdu(&raw) {
            Ok(pdu) => describe(&pdu),
            Err(e) => vec![DescribedField {
                name: "decode error".into(),
                value: e.message,
            }],
        }
    }

    pub fn pdu_hex(&self, seq: u64) -> String {
        self.raw_at(seq)
            .map(|raw| crate::smpp::summary::hex(&raw))
            .unwrap_or_default()
    }

    pub fn clear_trace(&self) {
        let counters = {
            let mut trace = self.trace.lock().unwrap();
            trace.clear();
            trace.counters().clone()
        };
        self.hub.push_counters(counters);
    }

    pub fn set_trace_cap(&self, cap: usize) {
        self.trace.lock().unwrap().set_cap(cap);
    }

    pub fn export_trace(&self, format: ExportFormat, path: &str) -> Result<String, String> {
        let entries = self.trace.lock().unwrap().entries(None, None);
        let body = match format {
            ExportFormat::Text => trace_to_text(&entries),
            ExportFormat::Json => {
                let wire: Vec<WirePdu> = entries.iter().map(to_wire_pdu).collect();
                serde_json::to_string_pretty(&wire)
                    .map_err(|e| format!("could not write the trace as JSON: {e}"))?
            }
            ExportFormat::Csv => {
                let wire: Vec<WirePdu> = entries.iter().map(to_wire_pdu).collect();
                to_csv(&csv_rows(&wire, TRACE_CSV_COLUMNS), TRACE_CSV_COLUMNS)
            }
        };
        write_file(path, &body)
    }

    pub fn export_burst(&self, path: &str) -> Result<String, String> {
        let rows = self.burst_rows(0, usize::MAX);
        let body = to_csv(&csv_rows(&rows, BURST_CSV_COLUMNS), BURST_CSV_COLUMNS);
        write_file(path, &body)
    }

    /* ---------------------------------------------------------------- */
    /* profiles                                                         */

    pub fn list_profiles(&self) -> Vec<Profile> {
        self.profiles.list()
    }

    pub fn save_profile(
        &self,
        profile: &Profile,
        password: Option<&str>,
        key_passphrase: Option<&str>,
    ) -> Result<(), String> {
        self.profiles.save(profile, password, key_passphrase)
    }

    pub fn remove_profile(&self, id: &str) -> Result<(), String> {
        self.profiles.remove(id)
    }

    pub fn profile_secrets(&self, id: &str) -> ProfileSecrets {
        ProfileSecrets {
            password: self.profiles.password(id),
            key_passphrase: self.profiles.key_passphrase(id),
        }
    }

    /// Hands the result back untouched, and sends a failure on as an `error`
    /// event: a command never panics across the boundary, and the UI can log
    /// what went wrong even when nobody awaited the call.
    pub fn report<T>(&self, which: &str, result: Result<T, String>) -> Result<T, String> {
        self.hub.report(which, result)
    }

    /// Sends whatever the batcher is holding. The flush tick does this in
    /// production; the tests call it directly.
    pub fn flush_events(&self) {
        self.hub.flush();
    }

    /* ---------------------------------------------------------------- */

    fn session(&self) -> Result<Arc<Session>, String> {
        self.live
            .lock()
            .unwrap()
            .session
            .clone()
            .ok_or_else(|| "not connected".to_string())
    }

    fn raw_at(&self, seq: u64) -> Option<Vec<u8>> {
        self.trace.lock().unwrap().entry(seq).map(|e| e.raw.clone())
    }

    /// Reads the session's events into the trace and on to the webview.
    fn pump_session(&self, mut events: mpsc::UnboundedReceiver<SessionEvent>) {
        let hub = self.hub.clone();
        let trace = self.trace.clone();
        let live = self.live.clone();

        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    SessionEvent::PduOut { pdu, at } => {
                        record(&hub, &trace, &live, entry_for(&pdu, Direction::Out, at));
                    }
                    SessionEvent::PduIn { pdu, at } => {
                        // The burst engine needs every incoming PDU to match
                        // its receipts, and it needs them before the row is
                        // reported.
                        if let Some(burst) = live.lock().unwrap().burst.clone() {
                            burst.observe(&pdu);
                        }
                        record(&hub, &trace, &live, entry_for(&pdu, Direction::In, at));
                    }
                    SessionEvent::State { state, mode } => {
                        hub.emit_now("state", &StateEvent { state, mode });
                    }
                    SessionEvent::DecodeError { message, raw } => {
                        record(
                            &hub,
                            &trace,
                            &live,
                            TraceEntry {
                                seq: 0,
                                at: now_ms(),
                                direction: Direction::In,
                                command_name: "(undecodable)".into(),
                                command_status: 0,
                                sequence_number: 0,
                                summary: String::new(),
                                raw,
                                decode_error: Some(message),
                            },
                        );
                    }
                    SessionEvent::RawOut { bytes, at } => {
                        let summary = format!("{} bytes composed by hand", bytes.len());
                        record(
                            &hub,
                            &trace,
                            &live,
                            TraceEntry {
                                seq: 0,
                                at,
                                direction: Direction::Out,
                                command_name: "(raw)".into(),
                                command_status: 0,
                                sequence_number: 0,
                                summary,
                                raw: bytes,
                                decode_error: None,
                            },
                        );
                    }
                    // Already recorded as PduIn; the session reports them
                    // separately for its own bookkeeping.
                    SessionEvent::Orphan(_) | SessionEvent::Timeout { .. } => {}
                }
            }
        });
    }

    fn pump_burst(&self, mut events: mpsc::UnboundedReceiver<BurstEvent>) {
        let hub = self.hub.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    // Rows are batched: a 50,000-message burst must not be
                    // 50,000 messages to the webview.
                    BurstEvent::Row(row) => hub.push_burst_row(to_wire_burst_row(&row)),
                    BurstEvent::Progress(p) => hub.emit_now("burst-progress", &p),
                    BurstEvent::Done(done) => {
                        hub.flush();
                        hub.emit_now("burst-done", &done);
                    }
                }
            }
        });
    }

    fn pump_load(&self, mut events: mpsc::UnboundedReceiver<LoadEvent>) {
        let hub = self.hub.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    LoadEvent::Progress(p) => hub.push_load_progress(p),
                    LoadEvent::Step(step) => hub.emit_now("load-step", &step),
                    LoadEvent::Done(report) => {
                        hub.flush();
                        hub.emit_now("load-done", &report);
                    }
                }
            }
        });
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindAccepted {
    pub system_id: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StateEvent {
    state: BindState,
    mode: Option<BindMode>,
}

fn entry_for(pdu: &Pdu, direction: Direction, at: u64) -> TraceEntry {
    TraceEntry {
        seq: 0,
        at,
        direction,
        command_name: command_name(pdu.command_id),
        command_status: pdu.command_status,
        sequence_number: pdu.sequence_number,
        summary: summarise(pdu),
        raw: pdu.raw.clone(),
        decode_error: None,
    }
}

/// Numbers the row, stores it, and sends it on with the counters it changed.
fn record(hub: &EventHub, trace: &Mutex<TraceStore>, live: &Mutex<Live>, mut entry: TraceEntry) {
    entry.seq = {
        let mut live = live.lock().unwrap();
        let seq = live.next_row;
        live.next_row += 1;
        seq
    };

    let counters: BTreeMap<String, Counter> = {
        let mut trace = trace.lock().unwrap();
        trace.add(entry.clone());
        trace.counters().clone()
    };

    hub.push_pdu(to_wire_pdu(&entry));
    hub.push_counters(counters);
}

fn tls_options(req: &ConnectRequest) -> Result<TlsOptions, String> {
    let read = |path: &Option<String>, what: &str| -> Result<Option<Vec<u8>>, String> {
        match path {
            Some(p) if !p.is_empty() => std::fs::read(p)
                .map(Some)
                .map_err(|e| format!("could not read the {what} at {p}: {e}")),
            _ => Ok(None),
        }
    };

    Ok(TlsOptions {
        enabled: req.tls.enabled,
        // Read at connect time, so a renewed certificate takes effect without
        // re-saving the profile.
        ca: read(&req.tls.ca_path, "CA bundle")?,
        reject_unauthorized: req.tls.reject_unauthorized,
        servername: req.tls.servername.clone(),
        client_cert: read(&req.tls.client_cert_path, "client certificate")?,
        client_key: read(&req.tls.client_key_path, "client key")?,
    })
}

/// Serialises each item and pulls out the named columns as text, so the CSV
/// column list is stated once.
fn csv_rows<T: serde::Serialize>(items: &[T], columns: &[&str]) -> Vec<Vec<(String, String)>> {
    items
        .iter()
        .map(|item| {
            let value = serde_json::to_value(item).unwrap_or(serde_json::Value::Null);
            columns
                .iter()
                .map(|column| {
                    let cell = match value.get(column) {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Null) | None => String::new(),
                        Some(other) => other.to_string(),
                    };
                    ((*column).to_string(), cell)
                })
                .collect()
        })
        .collect()
}

fn write_file(path: &str, body: &str) -> Result<String, String> {
    std::fs::write(path, body).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(path.to_string())
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::profiles::{Profile, ProfileBind, ProfileTls, SecretStore};
    use crate::app::session::BindMode;
    use crate::smpp::constants::{command_id, InterfaceVersion};
    use crate::testing::fake_smsc::FakeSmsc;
    use crate::testing::recorder::Recorder;
    use crate::testing::tmp::TempDir;
    use std::sync::Arc;

    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn available(&self) -> bool {
            true
        }
        fn get(&self, _k: &str) -> Option<String> {
            None
        }
        fn set(&self, _k: &str, _v: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, _k: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn controller(dir: &TempDir) -> (Arc<Recorder>, Controller) {
        let rec = Arc::new(Recorder::default());
        let hub = Arc::new(EventHub::new(rec.clone()));
        let profiles = ProfileStore::new(dir.path(), Box::new(NoSecrets));
        (rec, Controller::new(hub, profiles))
    }

    fn connect_request(port: u16) -> ConnectRequest {
        ConnectRequest {
            host: "127.0.0.1".into(),
            port,
            tls: Default::default(),
            correlation: Default::default(),
            window_size: 10,
            response_timeout_ms: 2000,
            connect_timeout_ms: 3000,
        }
    }

    fn bind_request() -> BindRequest {
        BindRequest {
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

    fn submit(dest: &str) -> SendPduRequest {
        let mut req = SendPduRequest {
            command_id: command_id("submit_sm").unwrap(),
            ..Default::default()
        };
        req.fields.insert("destination_addr".into(), dest.into());
        req.octet_fields
            .insert("short_message".into(), "6869".into());
        req
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
    async fn refuses_to_send_before_connecting() {
        let dir = TempDir::new("ctl-unconnected");
        let (_rec, ctl) = controller(&dir);
        let err = ctl.send_pdu(submit("393331234567")).await.unwrap_err();
        assert!(err.contains("not connected"), "{err}");
    }

    #[tokio::test]
    async fn a_failed_command_surfaces_as_an_event_as_well_as_an_error() {
        let dir = TempDir::new("ctl-report");
        let (rec, ctl) = controller(&dir);

        let out = ctl.report("send_pdu", ctl.send_pdu(submit("393331234567")).await);
        assert!(out.is_err());

        let errors = rec.payloads("error");
        assert_eq!(errors.len(), 1, "nothing told the UI the command failed");
        assert_eq!(errors[0]["where"].as_str(), Some("send_pdu"));
        assert!(errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("not connected"));
    }

    #[tokio::test]
    async fn reports_unbound_and_disconnected_before_connecting() {
        let dir = TempDir::new("ctl-status");
        let (_rec, ctl) = controller(&dir);
        let status = ctl.status();
        assert_eq!(status.state, BindState::Unbound);
        assert!(!status.connected);
        assert_eq!(status.pending_buffered, 0);
        assert_eq!(status.pending_declared, None);
    }

    #[tokio::test]
    async fn records_both_directions_of_a_bind_in_the_trace() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-trace");
        let (rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        until(|| ctl.trace(0, 100).len() >= 2).await;

        let rows = ctl.trace(0, 100);
        assert_eq!(rows[0].direction, "out");
        assert_eq!(rows[0].command_name, "bind_transceiver");
        assert_eq!(rows[1].direction, "in");
        assert_eq!(rows[1].command_name, "bind_transceiver_resp");
        assert_eq!(rows[0].seq, 1, "the trace numbers its own rows from one");

        // The same PDUs reached the webview, batched.
        ctl.flush_events();
        let seen = rec.batched("pdu");
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0]["commandName"].as_str(), Some("bind_transceiver"));
        assert!(
            !rec.payloads("counters").is_empty(),
            "the counters must follow the traffic"
        );
    }

    #[tokio::test]
    async fn announces_the_bind_state_at_once_rather_than_on_the_tick() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-state");
        let (rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        until(|| {
            rec.payloads("state")
                .iter()
                .any(|p| p["state"].as_str() == Some("bound"))
        })
        .await;
    }

    #[tokio::test]
    async fn hands_back_the_exact_bytes_it_recorded() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-hex");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        until(|| ctl.trace(0, 100).len() >= 2).await;

        let hex = ctl.pdu_hex(1);
        assert!(!hex.is_empty());
        assert!(
            hex.starts_with("000000"),
            "a PDU starts with its length, and this one reads {hex}"
        );
        let fields = ctl.pdu_fields(1);
        assert!(
            fields.iter().any(|f| f.name == "system_id"),
            "the decoded fields are missing: {fields:?}"
        );
        assert!(ctl.pdu_hex(9999).is_empty(), "an unknown row has no bytes");
    }

    #[tokio::test]
    async fn a_second_connect_replaces_the_first() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-reconnect");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.connect(connect_request(port)).await.unwrap();
        until(|| smsc.connection_count() >= 2).await;
        assert!(ctl.status().connected);
    }

    #[tokio::test]
    async fn clearing_the_trace_sends_the_emptied_counters() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-clear");
        let (rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        until(|| ctl.trace(0, 100).len() >= 2).await;

        ctl.clear_trace();
        assert_eq!(ctl.trace(0, 100).len(), 0);
        ctl.flush_events();
        let counters = rec.payloads("counters");
        let last = counters
            .last()
            .expect("the cleared counters were never sent");
        assert_eq!(last.as_object().map(|o| o.len()), Some(0));
    }

    #[tokio::test]
    async fn exports_the_trace_in_each_format() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-export");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        until(|| ctl.trace(0, 100).len() >= 2).await;

        for (format, probe) in [
            (ExportFormat::Csv, "bind_transceiver"),
            (ExportFormat::Json, "\"commandName\""),
            (ExportFormat::Text, "bind_transceiver"),
        ] {
            let path = dir.join(&format!("trace.{}", format.extension()));
            let written = ctl.export_trace(format, path.to_str().unwrap()).unwrap();
            let body = std::fs::read_to_string(&written).unwrap();
            assert!(
                body.contains(probe),
                "{format:?} export looks wrong: {body}"
            );
        }
    }

    #[tokio::test]
    async fn keeps_profiles_through_the_store() {
        let dir = TempDir::new("ctl-profiles");
        let (_rec, ctl) = controller(&dir);
        let profile = Profile {
            id: "p1".into(),
            name: "Staging".into(),
            host: "smsc.example.com".into(),
            port: 2775,
            bind: ProfileBind {
                system_id: "esme".into(),
                system_type: String::new(),
                interface_version: InterfaceVersion::V34,
                mode: BindMode::Transceiver,
                addr_ton: 0,
                addr_npi: 0,
                address_range: String::new(),
            },
            tls: ProfileTls {
                enabled: false,
                reject_unauthorized: true,
                servername: None,
                ca_path: None,
                client_cert_path: None,
                client_key_path: None,
            },
            correlation: Default::default(),
        };

        ctl.save_profile(&profile, None, None).unwrap();
        assert_eq!(ctl.list_profiles().len(), 1);
        let secrets = ctl.profile_secrets("p1");
        assert_eq!(secrets.password, None);
        ctl.remove_profile("p1").unwrap();
        assert!(ctl.list_profiles().is_empty());
    }

    #[tokio::test]
    async fn sends_a_submit_and_reports_the_status_the_smsc_gave() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-submit");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        let out = ctl.send_pdu(submit("393331234567")).await.unwrap();
        assert_eq!(out.command_status, 0);
        assert!(out.sequence_number > 0);
    }

    #[tokio::test]
    async fn records_hand_written_bytes_the_codec_cannot_read() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-raw");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.send_raw("deadbeef").await.unwrap();
        until(|| ctl.trace(0, 100).iter().any(|r| r.command_name == "(raw)")).await;

        let row = ctl
            .trace(0, 100)
            .into_iter()
            .find(|r| r.command_name == "(raw)")
            .unwrap();
        assert!(
            row.hex.contains("deadbeef"),
            "the bytes were not kept: {}",
            row.hex
        );
    }

    #[tokio::test]
    async fn disconnecting_leaves_nothing_connected() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-disconnect");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        ctl.disconnect();
        assert!(!ctl.status().connected);
        let err = ctl.send_pdu(submit("393331234567")).await.unwrap_err();
        assert!(err.contains("not connected"), "{err}");
    }

    fn burst_request(count: usize, rate: u32) -> StartBurstRequest {
        StartBurstRequest {
            recipients: crate::app::recipients::RecipientSource::Repeat {
                number: "393331234567".into(),
                count,
            },
            rate_per_sec: rate,
            template: submit("393331234567"),
            text: "load".into(),
            data_coding: 0,
            segmentation: Default::default(),
            placement: Default::default(),
            packing: Default::default(),
            backoff_on_throttle: false,
            await_dlr_ms: 0,
        }
    }

    #[tokio::test]
    async fn runs_a_burst_and_reports_every_row_to_the_renderer() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-burst");
        let (rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        let done = ctl.start_burst(burst_request(5, 200)).await.unwrap();

        assert_eq!(done.stats.messages, 5);
        assert_eq!(done.stats.accepted, 5);
        assert_eq!(ctl.burst_rows(0, 100).len(), 5);
        assert_eq!(ctl.burst_rows(2, 2).len(), 2, "the page is a page");

        // Every row reached the webview, and the finish was announced after
        // them rather than before.
        until(|| rec.batched("burst-row").len() == 5).await;
        until(|| !rec.payloads("burst-done").is_empty()).await;
        let names = rec.names();
        let last_row = names.iter().rposition(|n| n == "burst-row").unwrap();
        let done_at = names.iter().position(|n| n == "burst-done").unwrap();
        assert!(
            last_row < done_at,
            "burst-done arrived before the rows it summarises"
        );
    }

    #[tokio::test]
    async fn the_rows_it_hands_the_renderer_carry_the_status_name() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-rowname");
        let (rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        ctl.start_burst(burst_request(2, 200)).await.unwrap();

        let rows = ctl.burst_rows(0, 10);
        assert_eq!(rows[0].status_name, "ESME_ROK");

        until(|| rec.batched("burst-row").len() == 2).await;
        let sent = rec.batched("burst-row");
        assert_eq!(sent[0]["statusName"].as_str(), Some("ESME_ROK"));
        assert_eq!(sent[0]["recipient"].as_str(), Some("393331234567"));
    }

    #[tokio::test]
    async fn encodes_a_body_so_the_renderer_never_has_to() {
        let dir = TempDir::new("ctl-encode");
        let (_rec, ctl) = controller(&dir);

        let out = ctl.encode_body("hi", 0, Default::default());
        assert_eq!(out.hex, "6869");
        assert!(out.unmappable.is_empty());

        // UCS2 is two octets a character, big-endian.
        let ucs2 = ctl.encode_body("hi", 8, Default::default());
        assert_eq!(ucs2.hex, "00680069");

        // A character the coding cannot hold is named, and still goes out as
        // a question mark rather than stopping the send.
        let snowman = ctl.encode_body("a\u{2744}", 0, Default::default());
        assert_eq!(snowman.unmappable, vec!["\u{2744}".to_string()]);
        assert_eq!(snowman.hex, "613f");
    }

    #[tokio::test]
    async fn exports_the_burst_rows_with_the_status_name_spelt_out() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-burstcsv");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();
        ctl.start_burst(burst_request(3, 200)).await.unwrap();

        let path = dir.join("burst.csv");
        ctl.export_burst(path.to_str().unwrap()).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.lines().count() >= 4, "header plus three rows: {body}");
        assert!(body.contains("statusName"), "{body}");
        assert!(
            body.contains("ESME_ROK"),
            "the status name is missing: {body}"
        );
        assert!(body.contains("393331234567"));
    }

    #[tokio::test]
    async fn runs_a_load_test_and_keeps_its_report() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-load");
        let (rec, ctl) = controller(&dir);

        let report = ctl
            .start_load_test(StartLoadRequest {
                connect: connect_request(port),
                bind: bind_request(),
                connections: 2,
                messages: 10,
                mode: crate::app::loadtest::LoadMode::Fixed { rate_per_sec: 100 },
                template: submit("393331234567"),
                destination: "393331234567".into(),
                text: "load".into(),
                data_coding: 0,
                segmentation: Default::default(),
                placement: Default::default(),
                packing: Default::default(),
            })
            .await
            .unwrap();

        assert_eq!(report.submitted, 10);
        assert_eq!(report.connections, 2);
        // The report survives the call, so the UI can ask for it again.
        assert_eq!(ctl.load_report().map(|r| r.submitted), Some(10));
        until(|| !rec.payloads("load-done").is_empty()).await;
        // The load test runs on its own sockets, so the session trace is
        // untouched by it.
        assert!(ctl.trace(0, 100).is_empty());
    }

    #[tokio::test]
    async fn stopping_a_burst_ends_it_before_the_end() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let dir = TempDir::new("ctl-stop");
        let (_rec, ctl) = controller(&dir);

        ctl.connect(connect_request(port)).await.unwrap();
        ctl.bind(bind_request()).await.unwrap();

        let ctl = Arc::new(ctl);
        let stopper = ctl.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            stopper.stop_burst();
        });

        let done = ctl.start_burst(burst_request(200, 10)).await.unwrap();
        assert_eq!(done.reason, crate::app::burst::BurstDoneReason::Stopped);
        assert!(
            done.stats.messages < 200,
            "it should not have finished: {} sent",
            done.stats.messages
        );
    }

    #[tokio::test]
    async fn has_no_load_report_until_a_load_test_has_run() {
        let dir = TempDir::new("ctl-noreport");
        let (_rec, ctl) = controller(&dir);
        assert!(ctl.load_report().is_none());
    }
}
