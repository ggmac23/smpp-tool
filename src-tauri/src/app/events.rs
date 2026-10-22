//! Events on their way to the webview, and the batching that keeps 500 PDUs
//! a second from being 500 messages a second.
//!
//! Batching may delay an event. It never drops one and never reorders one —
//! a trace that lies about what crossed the wire is worse than no trace. The
//! cumulative kinds (counters, load-test progress) keep only their newest
//! snapshot, because an older total carries nothing the newer one lacks.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::app::wire::WirePdu;

/// The seam over Tauri's `emit`, so the batching is testable without a window.
pub trait Emitter: Send + Sync + 'static {
    fn emit(&self, name: &str, payload: serde_json::Value);
}

pub const DEFAULT_BATCH: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Buffered {
    pdus: Vec<serde_json::Value>,
    rows: Vec<serde_json::Value>,
    counters: Option<serde_json::Value>,
    load_progress: Option<serde_json::Value>,
}

pub struct EventHub {
    emitter: Arc<dyn Emitter>,
    buf: Mutex<Buffered>,
}

impl EventHub {
    pub fn new(emitter: Arc<dyn Emitter>) -> Self {
        Self {
            emitter,
            buf: Mutex::new(Buffered::default()),
        }
    }

    /// Start the flush tick. Held by the caller; dropping the handle stops it.
    pub fn start(hub: &Arc<Self>, every: Duration) -> tokio::task::JoinHandle<()> {
        let hub = hub.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                hub.flush();
            }
        })
    }

    pub fn push_pdu(&self, pdu: WirePdu) {
        if let Ok(v) = serde_json::to_value(pdu) {
            self.buf.lock().unwrap().pdus.push(v);
        }
    }

    pub fn push_burst_row<T: Serialize>(&self, row: T) {
        if let Ok(v) = serde_json::to_value(row) {
            self.buf.lock().unwrap().rows.push(v);
        }
    }

    /// Counters are cumulative, so only the newest snapshot is kept.
    pub fn push_counters<T: Serialize>(&self, counters: T) {
        if let Ok(v) = serde_json::to_value(counters) {
            self.buf.lock().unwrap().counters = Some(v);
        }
    }

    /// Load-test progress is a running tally, so only the newest is kept: at
    /// 400 messages a second nobody reads the one before.
    pub fn push_load_progress<T: Serialize>(&self, progress: T) {
        if let Ok(v) = serde_json::to_value(progress) {
            self.buf.lock().unwrap().load_progress = Some(v);
        }
    }

    /// For the events that must not wait for the tick: a bind that completed
    /// cannot look pending for another 100 ms.
    pub fn emit_now<T: Serialize>(&self, name: &str, payload: &T) {
        match serde_json::to_value(payload) {
            Ok(v) => self.emitter.emit(name, v),
            Err(e) => self.emitter.emit(
                "error",
                serde_json::json!({ "message": e.to_string(), "where": name }),
            ),
        }
    }

    pub fn flush(&self) {
        let (pdus, rows, counters, load) = {
            let mut buf = self.buf.lock().unwrap();
            (
                std::mem::take(&mut buf.pdus),
                std::mem::take(&mut buf.rows),
                buf.counters.take(),
                buf.load_progress.take(),
            )
        };

        if !pdus.is_empty() {
            self.emitter.emit("pdu", serde_json::Value::Array(pdus));
        }
        if !rows.is_empty() {
            self.emitter
                .emit("burst-row", serde_json::Value::Array(rows));
        }
        if let Some(counters) = counters {
            self.emitter.emit("counters", counters);
        }
        if let Some(load) = load {
            self.emitter.emit("load-progress", load);
        }
    }

    /// Hands the result back untouched, and sends a failure on as an `error`
    /// event too: a command never throws across the boundary, and the UI can
    /// log what went wrong even when nobody awaited the call.
    pub fn report<T>(&self, which: &str, result: Result<T, String>) -> Result<T, String> {
        if let Err(message) = &result {
            self.emitter.emit(
                "error",
                serde_json::json!({ "message": message, "where": which }),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::trace::Counter;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[derive(Default)]
    struct Recorder {
        sent: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl Emitter for Recorder {
        fn emit(&self, name: &str, payload: serde_json::Value) {
            self.sent.lock().unwrap().push((name.to_string(), payload));
        }
    }

    impl Recorder {
        fn names(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|(n, _)| n.clone())
                .collect()
        }
        fn payloads(&self, name: &str) -> Vec<serde_json::Value> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, p)| p.clone())
                .collect()
        }
    }

    fn hub() -> (Arc<Recorder>, EventHub) {
        let rec = Arc::new(Recorder::default());
        (rec.clone(), EventHub::new(rec))
    }

    fn pdu(seq: u64) -> WirePdu {
        WirePdu {
            seq,
            at: seq,
            direction: "out".into(),
            command_name: "submit_sm".into(),
            command_status: 0,
            status_name: "ESME_ROK".into(),
            sequence_number: seq as u32,
            summary: String::new(),
            hex: String::new(),
            decode_error: None,
        }
    }

    fn counters(sent: u64) -> BTreeMap<String, Counter> {
        BTreeMap::from([("submit_sm".to_string(), Counter { sent, received: 0 })])
    }

    #[test]
    fn batches_pdu_events_instead_of_one_per_pdu() {
        let (rec, hub) = hub();
        for i in 1..=5 {
            hub.push_pdu(pdu(i));
        }
        assert_eq!(
            rec.names(),
            Vec::<String>::new(),
            "nothing goes out before a flush"
        );
        hub.flush();
        assert_eq!(rec.names(), vec!["pdu"], "five PDUs should be one message");
        assert_eq!(rec.payloads("pdu")[0].as_array().unwrap().len(), 5);
    }

    #[test]
    fn preserves_pdu_order_inside_a_batch() {
        let (rec, hub) = hub();
        for i in 1..=20 {
            hub.push_pdu(pdu(i));
        }
        hub.flush();
        let seqs: Vec<u64> = rec.payloads("pdu")[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["seq"].as_u64().unwrap())
            .collect();
        assert_eq!(seqs, (1..=20).collect::<Vec<_>>());
    }

    #[test]
    fn never_drops_a_pdu_when_batching() {
        let (rec, hub) = hub();
        // Flushing part-way through is what a real run does: the timer fires
        // while the session is still pushing.
        for i in 1..=2000 {
            hub.push_pdu(pdu(i));
            if i % 37 == 0 {
                hub.flush();
            }
        }
        hub.flush();

        let seen: Vec<u64> = rec
            .payloads("pdu")
            .iter()
            .flat_map(|b| b.as_array().unwrap().clone())
            .map(|p| p["seq"].as_u64().unwrap())
            .collect();
        assert_eq!(seen.len(), 2000, "a PDU went missing");
        assert_eq!(seen, (1..=2000).collect::<Vec<_>>(), "the order changed");
        assert!(
            rec.payloads("pdu").len() > 1,
            "the test did not actually batch"
        );
    }

    #[test]
    fn batches_counters_latest_snapshot_wins() {
        let (rec, hub) = hub();
        hub.push_counters(counters(1));
        hub.push_counters(counters(2));
        hub.push_counters(counters(9));
        hub.flush();
        let sent = rec.payloads("counters");
        assert_eq!(
            sent.len(),
            1,
            "counters are cumulative; only the newest is worth sending"
        );
        assert_eq!(sent[0]["submit_sm"]["sent"].as_u64(), Some(9));
    }

    #[test]
    fn keeps_only_the_newest_load_progress() {
        // A load test at 400/sec produces progress faster than anyone can
        // read it, and an older tally says nothing the newer one does not.
        let (rec, hub) = hub();
        hub.push_load_progress(serde_json::json!({ "submitted": 10 }));
        hub.push_load_progress(serde_json::json!({ "submitted": 4000 }));
        hub.flush();
        let sent = rec.payloads("load-progress");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["submitted"].as_u64(), Some(4000));
    }

    #[test]
    fn flushing_an_empty_buffer_sends_nothing() {
        let (rec, hub) = hub();
        hub.flush();
        hub.flush();
        assert_eq!(rec.names(), Vec::<String>::new());
    }

    #[test]
    fn sends_state_at_once_rather_than_on_the_tick() {
        // A bind that completed must not look pending for another 100 ms.
        let (rec, hub) = hub();
        hub.emit_now("state", &serde_json::json!({ "state": "bound" }));
        assert_eq!(rec.names(), vec!["state"]);
    }

    #[test]
    fn a_failing_command_returns_an_error_not_a_panic() {
        let (rec, hub) = hub();
        let out: Result<u8, String> = hub.report("send_pdu", Err("not connected".into()));
        assert_eq!(out, Err("not connected".to_string()));

        let errors = rec.payloads("error");
        assert_eq!(
            errors.len(),
            1,
            "the failure should also surface as an event"
        );
        assert_eq!(errors[0]["message"].as_str(), Some("not connected"));
        assert_eq!(
            errors[0]["where"].as_str(),
            Some("send_pdu"),
            "the renderer needs to know which command failed"
        );
    }

    #[test]
    fn a_succeeding_command_reports_nothing() {
        let (rec, hub) = hub();
        let out: Result<u8, String> = hub.report("status", Ok(7));
        assert_eq!(out, Ok(7));
        assert_eq!(rec.names(), Vec::<String>::new());
    }

    #[test]
    fn batched_rows_and_pdus_travel_on_their_own_channels() {
        let (rec, hub) = hub();
        hub.push_pdu(pdu(1));
        hub.push_burst_row(serde_json::json!({ "id": "r0" }));
        hub.flush();
        let mut names = rec.names();
        names.sort();
        assert_eq!(names, vec!["burst-row", "pdu"]);
    }
}
