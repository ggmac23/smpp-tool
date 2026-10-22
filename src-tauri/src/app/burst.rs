//! Sending a burst of messages at a requested rate and recording what came
//! back.
//!
//! On ESME_RTHROTTLED the default is to record the error and hold the
//! requested rate — the point of the exercise is to find where the SMSC
//! actually breaks. If the bind drops the burst stops and says so; it never
//! reconnects. `rows` is never capped: the trace may lose history under
//! pressure, the result table may not.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::sync::mpsc;

use crate::app::dlr::{CorrelationConfig, DlrCorrelator};
use crate::app::recipients::{resolve_recipients, RecipientSource, MAX_RATE};
use crate::app::session::{BindState, Session};
use crate::smpp::constants::{command_status_name, ESME_ROK};
use crate::smpp::decode::Pdu;
use crate::smpp::encode::PduInput;
use crate::smpp::receipt::parse_receipt;
use crate::smpp::segment::{segment, BodyPlacement, SegmentOptions};
use crate::smpp::text::{Gsm7Packing, SegmentationMode};

const TICK: Duration = Duration::from_millis(20);
/// A status of our own for a request that never came back.
pub const STATUS_NO_RESPONSE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct BurstConfig {
    pub recipients: RecipientSource,
    pub rate_per_sec: u32,
    /// The submit template: everything except the destination and the body.
    pub template: PduInput,
    pub text: String,
    pub data_coding: u8,
    pub segmentation: SegmentationMode,
    pub placement: BodyPlacement,
    pub packing: Gsm7Packing,
    /// On ESME_RTHROTTLED: halve the rate, instead of holding it.
    pub backoff_on_throttle: bool,
    /// How long to keep waiting for receipts once every submit is answered.
    pub await_dlr: Duration,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BurstRow {
    pub id: String,
    pub recipient: String,
    pub body: String,
    /// PDUs this message took. A three-part message is one row with pdus 3.
    pub pdus: usize,
    pub message_id: Option<String>,
    pub command_status: Option<u32>,
    pub submit_latency_ms: Option<u64>,
    pub dlr_state: Option<String>,
    pub dlr_latency_ms: Option<u64>,
    /// When the first part went out. The runner's own bookkeeping; public so
    /// a test elsewhere can build a row, and skipped by serde so it never
    /// crosses to the renderer.
    #[serde(skip)]
    pub sent_at: Option<Instant>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BurstStats {
    /// Messages the user asked for.
    pub messages: usize,
    /// PDUs those messages took.
    pub pdus: usize,
    pub submitted: usize,
    pub accepted: usize,
    pub errors_by_status: BTreeMap<String, usize>,
    pub achieved_rate_per_sec: f64,
    pub latency_p50: u64,
    pub latency_p95: u64,
    pub latency_p99: u64,
    pub dlr_received: usize,
    pub dlr_unmatched: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BurstDoneReason {
    Complete,
    Stopped,
    ConnectionLost,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BurstDone {
    pub reason: BurstDoneReason,
    pub stats: BurstStats,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BurstProgress {
    pub sent: usize,
    pub total: usize,
    pub stats: BurstStats,
}

#[derive(Debug, Clone)]
pub enum BurstEvent {
    Row(BurstRow),
    Progress(BurstProgress),
    Done(BurstDone),
}

fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((p * sorted.len()).div_ceil(100))
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

#[derive(Default)]
struct State {
    rows: Vec<BurstRow>,
    latencies: Vec<u64>,
    errors: BTreeMap<String, usize>,
    messages: usize,
    pdus: usize,
    submitted: usize,
    accepted: usize,
    dlr_received: usize,
    dlr_unmatched: usize,
    parts_done: BTreeMap<String, usize>,
    rate: u32,
    stopping: bool,
    lost_connection: bool,
    started_at: Option<Instant>,
    sending_ended_at: Option<Instant>,
}

pub struct BurstRunner {
    session: Arc<Session>,
    correlation: CorrelationConfig,
    state: Arc<Mutex<State>>,
    correlator: Arc<Mutex<DlrCorrelator>>,
    events: Option<mpsc::UnboundedSender<BurstEvent>>,
}

impl BurstRunner {
    pub fn new(session: Arc<Session>, correlation: CorrelationConfig) -> Self {
        Self {
            session,
            correlation,
            state: Arc::new(Mutex::new(State::default())),
            correlator: Arc::new(Mutex::new(DlrCorrelator::new(correlation))),
            events: None,
        }
    }

    pub fn with_events(mut self, events: mpsc::UnboundedSender<BurstEvent>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn rows(&self) -> Vec<BurstRow> {
        self.state.lock().unwrap().rows.clone()
    }

    /// The rate currently in force: the requested one unless backoff kicked in.
    pub fn current_rate_per_sec(&self) -> u32 {
        self.state.lock().unwrap().rate
    }

    pub fn stop(&self) {
        self.state.lock().unwrap().stopping = true;
    }

    pub fn stats(&self) -> BurstStats {
        stats_of(&self.state.lock().unwrap())
    }

    /// Feed an inbound PDU in, so a receipt can find its row. The controller
    /// forwards everything the session receives while a burst is running.
    pub fn observe(&self, pdu: &Pdu) {
        let Some(receipt) = parse_receipt(pdu) else {
            return;
        };
        let token = self.correlator.lock().unwrap().match_receipt(&receipt);
        let Some(token) = token else {
            // A receipt for something this burst never sent — or one that beat
            // its own registration. Counted, never attributed to a row it does
            // not belong to; `register` can still claim it later.
            self.state.lock().unwrap().dlr_unmatched += 1;
            return;
        };
        self.apply_receipt(&token, &receipt, false);
    }

    /// Record a receipt against its row. `was_unmatched` is true when the
    /// receipt had been counted as orphaned and is now being claimed.
    fn apply_receipt(
        &self,
        token: &str,
        receipt: &crate::smpp::receipt::Receipt,
        was_unmatched: bool,
    ) {
        let row = {
            let mut state = self.state.lock().unwrap();
            if was_unmatched {
                state.dlr_unmatched = state.dlr_unmatched.saturating_sub(1);
            }
            let Some(row) = state.rows.iter_mut().find(|r| r.id == token) else {
                return;
            };
            row.dlr_state = receipt.state.clone();
            row.dlr_latency_ms = row.sent_at.map(|t| t.elapsed().as_millis() as u64);
            let row = row.clone();
            state.dlr_received += 1;
            row
        };

        if let Some(events) = &self.events {
            let _ = events.send(BurstEvent::Row(row));
        }
    }

    pub async fn start(&self, cfg: BurstConfig) -> Result<BurstDone, String> {
        if self.session.state() != BindState::Bound {
            return Err(format!(
                "session is not bound (state is {:?})",
                self.session.state()
            ));
        }
        if cfg.rate_per_sec > MAX_RATE {
            return Err(format!(
                "{}/sec is above the {MAX_RATE}/sec ceiling of this build",
                cfg.rate_per_sec
            ));
        }
        if cfg.rate_per_sec < 1 {
            return Err("the rate must be at least 1 message per second".into());
        }

        // Resolving first means an over-ceiling count is refused before a
        // single PDU goes out.
        let resolved = resolve_recipients(&cfg.recipients)?;

        {
            let mut state = self.state.lock().unwrap();
            *state = State {
                rate: cfg.rate_per_sec,
                started_at: Some(Instant::now()),
                ..Default::default()
            };
        }
        *self.correlator.lock().unwrap() = DlrCorrelator::new(self.correlation);

        self.pump(&cfg, &resolved.numbers).await;
        self.state.lock().unwrap().sending_ended_at = Some(Instant::now());
        self.await_dlrs(cfg.await_dlr).await;

        let (reason, stats) = {
            let state = self.state.lock().unwrap();
            let reason = if state.lost_connection {
                BurstDoneReason::ConnectionLost
            } else if state.stopping {
                BurstDoneReason::Stopped
            } else {
                BurstDoneReason::Complete
            };
            (reason, stats_of(&state))
        };

        let done = BurstDone {
            reason,
            stats,
            warnings: resolved.warnings,
        };
        if let Some(events) = &self.events {
            let _ = events.send(BurstEvent::Done(done.clone()));
        }
        Ok(done)
    }

    /// Pace the submits and wait for them to settle.
    async fn pump(&self, cfg: &BurstConfig, numbers: &[String]) {
        // The sends must run while the loop waits for its next tick. A plain
        // Vec of futures would only be polled at the end, and the whole burst
        // would leave in one clump no matter what rate was asked for.
        let mut inflight = FuturesUnordered::new();
        let mut i = 0usize;

        while i < numbers.len() {
            if self.state.lock().unwrap().stopping {
                break;
            }
            if self.session.state() != BindState::Bound {
                let mut state = self.state.lock().unwrap();
                state.lost_connection = true;
                state.stopping = true;
                break;
            }

            // Above one message per tick, send a batch each tick. Below it,
            // send one message and wait the whole gap — otherwise the floor
            // would be one per tick and a request for 5/sec would quietly go
            // out at 50/sec.
            let rate = self.state.lock().unwrap().rate.max(1);
            let per_tick = rate as f64 * TICK.as_secs_f64();
            let (to_send, wait) = if per_tick >= 1.0 {
                (per_tick.round() as usize, TICK)
            } else {
                (1, Duration::from_secs_f64(1.0 / rate as f64))
            };

            let tick_started = Instant::now();
            for _ in 0..to_send {
                if i >= numbers.len() || self.state.lock().unwrap().stopping {
                    break;
                }
                inflight.push(self.send_one(cfg, numbers[i].clone(), i));
                i += 1;
            }

            let spent = tick_started.elapsed();
            drive(&mut inflight, wait.saturating_sub(spent)).await;

            self.emit_progress(numbers.len());
        }

        while inflight.next().await.is_some() {}
        self.emit_progress(numbers.len());
    }

    async fn send_one(&self, cfg: &BurstConfig, recipient: String, index: usize) {
        let row_id = format!("r{index}");
        let segments = segment(&SegmentOptions {
            text: cfg.text.clone(),
            data_coding: cfg.data_coding,
            mode: cfg.segmentation,
            placement: cfg.placement,
            packing: cfg.packing,
            user_udh: None,
            ref_num: None,
            esm_class: 0,
        });

        {
            let mut state = self.state.lock().unwrap();
            state.rows.push(BurstRow {
                id: row_id.clone(),
                recipient: recipient.clone(),
                body: cfg.text.clone(),
                pdus: segments.len(),
                message_id: None,
                command_status: None,
                submit_latency_ms: None,
                dlr_state: None,
                dlr_latency_ms: None,
                sent_at: None,
            });
            state.messages += 1;
        }

        for seg in &segments {
            if self.state.lock().unwrap().stopping {
                break;
            }

            let mut input = cfg.template.clone();
            for (name, value) in &seg.fields {
                input.fields.insert(name.clone(), value.clone());
            }
            input = input
                .with_str("destination_addr", &recipient)
                .with_num("data_coding", cfg.data_coding as u32);
            input.tlvs.extend(seg.tlvs.clone());

            let sent_at = Instant::now();
            {
                let mut state = self.state.lock().unwrap();
                if let Some(row) = state.rows.iter_mut().find(|r| r.id == row_id) {
                    if row.sent_at.is_none() {
                        row.sent_at = Some(sent_at);
                    }
                }
            }

            match self.session.send(input).await {
                Ok(resp) => {
                    let latency = sent_at.elapsed().as_millis() as u64;
                    let mut register: Option<String> = None;

                    {
                        let mut state = self.state.lock().unwrap();
                        state.pdus += 1;
                        state.submitted += 1;
                        state.latencies.push(latency);

                        let done_before = *state.parts_done.get(&row_id).unwrap_or(&0);
                        state.parts_done.insert(row_id.clone(), done_before + 1);

                        let total_parts = segments.len();
                        if let Some(row) = state.rows.iter_mut().find(|r| r.id == row_id) {
                            if row.submit_latency_ms.is_none() {
                                row.submit_latency_ms = Some(latency);
                            }
                            if resp.command_status == ESME_ROK {
                                let id = resp.str_field("message_id").unwrap_or_default();
                                if row.message_id.is_none() && !id.is_empty() {
                                    row.message_id = Some(id.clone());
                                    register = Some(id);
                                }
                                // A row is accepted only once every part is.
                                if row.command_status.is_none() && done_before + 1 == total_parts {
                                    row.command_status = Some(ESME_ROK);
                                }
                            } else {
                                // The first non-zero status is the row's
                                // verdict and is never overwritten.
                                if row.command_status.is_none()
                                    || row.command_status == Some(ESME_ROK)
                                {
                                    row.command_status = Some(resp.command_status);
                                }
                            }
                        }

                        if resp.command_status != ESME_ROK {
                            *state
                                .errors
                                .entry(command_status_name(resp.command_status))
                                .or_insert(0) += 1;
                            if resp.command_status
                                == crate::smpp::constants::command_status("ESME_RTHROTTLED")
                                    .unwrap_or(0)
                                && cfg.backoff_on_throttle
                            {
                                state.rate = (state.rate / 2).max(1);
                            }
                        }
                    }

                    if let Some(id) = register {
                        // A receipt may already have arrived for this id: an
                        // SMSC that answers and delivers together beats us to
                        // it. `register` hands it back rather than losing it.
                        let early = self.correlator.lock().unwrap().register(&id, &row_id);
                        if let Some(receipt) = early {
                            self.apply_receipt(&row_id, &receipt, true);
                        }
                    }
                }
                Err(e) => {
                    // A timeout or a lost connection: recorded, never retried
                    // behind the user's back.
                    let timed_out = e.to_lowercase().contains("timed out");
                    let name = if timed_out {
                        "TIMEOUT"
                    } else {
                        "CONNECTION_LOST"
                    };
                    let mut state = self.state.lock().unwrap();
                    // A bind lost while the tail of the burst was still in
                    // flight used to be reported as a clean completion,
                    // because only the pacing loop's pre-tick check set this.
                    if !timed_out {
                        state.lost_connection = true;
                        state.stopping = true;
                    }
                    *state.errors.entry(name.to_string()).or_insert(0) += 1;
                    state.submitted += 1;
                    if let Some(row) = state.rows.iter_mut().find(|r| r.id == row_id) {
                        if row.command_status.is_none() {
                            row.command_status = Some(STATUS_NO_RESPONSE);
                        }
                    }
                    break;
                }
            }
        }

        let row = {
            let mut state = self.state.lock().unwrap();
            let accepted = state
                .rows
                .iter()
                .find(|r| r.id == row_id)
                .map(|r| r.command_status == Some(ESME_ROK))
                .unwrap_or(false);
            if accepted {
                state.accepted += 1;
            }
            state.rows.iter().find(|r| r.id == row_id).cloned()
        };

        if let (Some(events), Some(row)) = (&self.events, row) {
            let _ = events.send(BurstEvent::Row(row));
        }
    }

    fn emit_progress(&self, total: usize) {
        let Some(events) = &self.events else { return };
        let state = self.state.lock().unwrap();
        let _ = events.send(BurstEvent::Progress(BurstProgress {
            sent: state.submitted,
            total,
            stats: stats_of(&state),
        }));
    }

    async fn await_dlrs(&self, how_long: Duration) {
        if how_long.is_zero() {
            return;
        }
        let deadline = Instant::now() + how_long;
        while Instant::now() < deadline {
            {
                let state = self.state.lock().unwrap();
                if state.dlr_received >= state.messages || state.lost_connection {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Lets the queued sends run for `how_long`, which is how the burst is paced:
/// the loop hands out work and then spends the rest of the tick letting it
/// happen, rather than piling futures up for a single flush at the end.
async fn drive<F>(inflight: &mut FuturesUnordered<F>, how_long: Duration)
where
    F: std::future::Future<Output = ()>,
{
    let sleep = tokio::time::sleep(how_long);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return,
            _ = inflight.next(), if !inflight.is_empty() => {}
        }
    }
}

fn stats_of(state: &State) -> BurstStats {
    let mut sorted = state.latencies.clone();
    sorted.sort_unstable();

    // The rate is measured over the sending, never including the wait for
    // receipts, which would dilute the figure the burst exists to produce.
    let elapsed = match (state.started_at, state.sending_ended_at) {
        (Some(start), Some(end)) => end.duration_since(start).as_secs_f64(),
        (Some(start), None) => start.elapsed().as_secs_f64(),
        _ => 0.0,
    };

    BurstStats {
        messages: state.messages,
        pdus: state.pdus,
        submitted: state.submitted,
        accepted: state.accepted,
        errors_by_status: state.errors.clone(),
        achieved_rate_per_sec: if elapsed > 0.0 {
            state.submitted as f64 / elapsed
        } else {
            0.0
        },
        latency_p50: percentile(&sorted, 50),
        latency_p95: percentile(&sorted, 95),
        latency_p99: percentile(&sorted, 99),
        dlr_received: state.dlr_received,
        dlr_unmatched: state.dlr_unmatched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dlr::CorrelationConfig;
    use crate::app::session::{BindMode, BindOptions, Session, SessionOptions};
    use crate::smpp::constants::{command_id, command_status, DataCoding, InterfaceVersion};
    use crate::smpp::encode::PduInput;
    use crate::smpp::segment::BodyPlacement;
    use crate::smpp::text::{Gsm7Packing, SegmentationMode};
    use crate::testing::fake_smsc::FakeSmsc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    /// Build a runner and feed it the session's inbound PDUs, which is the
    /// wiring the controller does in production.
    fn runner_for(
        session: Arc<Session>,
        mut events: mpsc::UnboundedReceiver<crate::app::session::SessionEvent>,
    ) -> Arc<BurstRunner> {
        let runner = Arc::new(BurstRunner::new(session, CorrelationConfig::default()));
        let observer = runner.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let crate::app::session::SessionEvent::PduIn { pdu, .. } = event {
                    observer.observe(&pdu);
                }
            }
        });
        runner
    }

    async fn bound(
        smsc: &FakeSmsc,
        window: usize,
    ) -> (
        Arc<Session>,
        mpsc::UnboundedReceiver<crate::app::session::SessionEvent>,
    ) {
        let port = smsc.listen().await;
        let (transport, events) =
            crate::app::transport::Transport::connect(crate::app::transport::TransportOptions {
                host: "127.0.0.1".into(),
                port,
                tls: Default::default(),
                connect_timeout: Duration::from_secs(3),
            })
            .await
            .unwrap();
        let (session, session_events) = Session::new(
            transport,
            events,
            SessionOptions {
                window_size: window,
                response_timeout: Duration::from_secs(2),
            },
        );
        session
            .bind(BindOptions {
                system_id: "esme".into(),
                password: "pw".into(),
                system_type: String::new(),
                interface_version: InterfaceVersion::V34,
                mode: BindMode::Transceiver,
                addr_ton: 0,
                addr_npi: 0,
                address_range: String::new(),
            })
            .await
            .unwrap();
        (Arc::new(session), session_events)
    }

    fn config(count: usize, rate: u32) -> BurstConfig {
        BurstConfig {
            recipients: crate::app::recipients::RecipientSource::Repeat {
                number: "3933".into(),
                count,
            },
            rate_per_sec: rate,
            template: PduInput::new(id("submit_sm"), 0).with_str("source_addr", "SENDER"),
            text: "hello".into(),
            data_coding: DataCoding::GSM_DEFAULT,
            segmentation: SegmentationMode::Udh,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
            backoff_on_throttle: false,
            await_dlr: Duration::from_millis(300),
        }
    }

    #[tokio::test]
    async fn paces_the_sending_instead_of_bursting_it_at_the_end() {
        // Ten messages at 10/sec must reach the SMSC spread over about a
        // second. A pacing loop that queues work without letting it run would
        // satisfy every rate assertion and still put the whole burst on the
        // wire in one clump.
        let smsc = FakeSmsc::new();
        let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let seen = arrivals.clone();
        smsc.on("submit_sm", move |_p| {
            seen.lock().unwrap().push(Instant::now());
            None
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);

        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(10, 10)
            })
            .await
            .unwrap();
        assert_eq!(done.stats.submitted, 10);

        let at = arrivals.lock().unwrap().clone();
        assert_eq!(at.len(), 10, "the SMSC did not see every submit");
        let spread = at.last().unwrap().duration_since(*at.first().unwrap());
        assert!(
            spread >= Duration::from_millis(600),
            "the submits arrived within {spread:?} of each other, so nothing was paced"
        );
    }

    #[tokio::test]
    async fn says_the_connection_was_lost_even_when_it_goes_in_the_tail() {
        // The pacing loop hands out all the work and then drains. A bind lost
        // during that drain used to be reported as Complete, because only the
        // loop's pre-tick check set the flag — so the verdict said the burst
        // finished while every row underneath it read CONNECTION_LOST.
        let smsc = FakeSmsc::new();
        // The SMSC answers nothing, so every send is still in flight when the
        // loop has finished handing them out.
        smsc.on("submit_sm", |_| None);
        let (session, events) = bound(&smsc, 50).await;
        let runner = runner_for(session, events);

        let killer = smsc.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            killer.drop_connections();
        });

        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(20, 500)
            })
            .await
            .unwrap();

        assert_eq!(done.reason, BurstDoneReason::ConnectionLost);
    }

    #[tokio::test]
    async fn sends_one_submit_sm_per_recipient() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);

        let done = runner
            .start(BurstConfig {
                recipients: crate::app::recipients::RecipientSource::Range {
                    from: "393331000000".into(),
                    count: 5,
                },
                await_dlr: Duration::ZERO,
                ..config(5, 200)
            })
            .await
            .unwrap();

        assert_eq!(done.stats.submitted, 5);
        assert_eq!(runner.rows().len(), 5);
        let recipients: Vec<String> = runner.rows().iter().map(|r| r.recipient.clone()).collect();
        assert_eq!(recipients[0], "393331000000");
        assert_eq!(recipients[4], "393331000004");
    }

    #[tokio::test]
    async fn records_the_message_id_and_latency_on_each_row() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(2, 200)
            })
            .await
            .unwrap();

        let rows = runner.rows();
        assert!(rows.iter().all(|r| r.message_id.is_some()));
        assert_ne!(rows[0].message_id, rows[1].message_id);
        assert!(rows.iter().all(|r| r.command_status == Some(0)));
        assert!(rows.iter().all(|r| r.submit_latency_ms.is_some()));
    }

    #[tokio::test]
    async fn counts_errors_by_status() {
        let smsc = FakeSmsc::new();
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        smsc.on("submit_sm", move |p| {
            let i = seen.fetch_add(1, Ordering::SeqCst) + 1;
            let status = if i.is_multiple_of(2) {
                command_status("ESME_RINVDSTADR").unwrap()
            } else {
                0
            };
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(status)
            .with_str("message_id", &format!("M{i}"))])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(4, 200)
            })
            .await
            .unwrap();

        assert_eq!(done.stats.submitted, 4);
        assert_eq!(done.stats.accepted, 2);
        assert_eq!(done.stats.errors_by_status.get("ESME_RINVDSTADR"), Some(&2));
    }

    #[tokio::test]
    async fn holds_the_requested_rate_when_the_smsc_throttles() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RTHROTTLED").unwrap())])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(6, 300)
            })
            .await
            .unwrap();

        assert_eq!(runner.current_rate_per_sec(), 300);
        assert_eq!(done.stats.submitted, 6);
        assert_eq!(done.stats.accepted, 0);
        assert_eq!(done.stats.errors_by_status.get("ESME_RTHROTTLED"), Some(&6));
    }

    #[tokio::test]
    async fn backs_off_when_asked_to() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RTHROTTLED").unwrap())])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        runner
            .start(BurstConfig {
                backoff_on_throttle: true,
                await_dlr: Duration::ZERO,
                ..config(6, 300)
            })
            .await
            .unwrap();

        assert!(runner.current_rate_per_sec() < 300);
    }

    #[tokio::test]
    async fn refuses_a_rate_above_the_ceiling_before_sending_anything() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);

        let err = runner
            .start(config(5, crate::app::recipients::MAX_RATE + 1))
            .await
            .unwrap_err();
        assert!(
            err.contains(&crate::app::recipients::MAX_RATE.to_string()),
            "{err}"
        );
        assert!(runner.rows().is_empty());
        assert!(smsc
            .received()
            .iter()
            .all(|p| p.command_id != id("submit_sm")));
    }

    #[tokio::test]
    async fn measures_the_achieved_rate_over_the_sending_not_the_wait_for_receipts() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);

        // No receipts will come, so await_dlr is spent entirely waiting.
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::from_millis(1000),
                ..config(10, 100)
            })
            .await
            .unwrap();

        // Ten messages at 100/sec is about 0.1s of sending. If the second of
        // waiting counted, the figure would collapse to about 9/sec.
        assert!(
            done.stats.achieved_rate_per_sec > 30.0,
            "{}",
            done.stats.achieved_rate_per_sec
        );
    }

    #[tokio::test]
    async fn honours_a_rate_slower_than_one_message_per_pacing_tick() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);

        let started = std::time::Instant::now();
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::ZERO,
                ..config(5, 10)
            })
            .await
            .unwrap();
        let elapsed = started.elapsed();

        // Five messages at 10/sec is about 400ms of sending. A pacer that
        // cannot go below one per 20ms tick would finish in ~100ms.
        assert!(elapsed > Duration::from_millis(250), "{elapsed:?}");
        assert!(done.stats.achieved_rate_per_sec < 20.0);
    }

    #[tokio::test]
    async fn correlates_a_deliver_sm_back_to_its_row() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            let message_id = format!("ID{}", p.sequence_number);
            Some(vec![
                PduInput::new(command_id("submit_sm_resp").unwrap(), p.sequence_number)
                    .with_str("message_id", &message_id),
                PduInput::new(command_id("deliver_sm").unwrap(), 9000 + p.sequence_number)
                    .with_num("esm_class", 4)
                    .with_bytes(
                        "short_message",
                        format!("id:{message_id} stat:DELIVRD err:000").into_bytes(),
                    ),
            ])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::from_millis(1500),
                ..config(3, 200)
            })
            .await
            .unwrap();

        let states: Vec<Option<String>> =
            runner.rows().iter().map(|r| r.dlr_state.clone()).collect();
        assert_eq!(states, vec![Some("DELIVRD".into()); 3]);
        assert_eq!(done.stats.dlr_received, 3);
        assert_eq!(done.stats.dlr_unmatched, 0);
    }

    #[tokio::test]
    async fn measures_dlr_latency_from_the_rows_own_submit() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            let message_id = format!("ID{}", p.sequence_number);
            Some(vec![
                PduInput::new(command_id("submit_sm_resp").unwrap(), p.sequence_number)
                    .with_str("message_id", &message_id),
                PduInput::new(command_id("deliver_sm").unwrap(), 9000 + p.sequence_number)
                    .with_num("esm_class", 4)
                    .with_bytes(
                        "short_message",
                        format!("id:{message_id} stat:DELIVRD err:000").into_bytes(),
                    ),
            ])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        // A slow burst, so later rows are submitted long after the start.
        runner
            .start(BurstConfig {
                await_dlr: Duration::from_millis(1500),
                ..config(6, 5)
            })
            .await
            .unwrap();

        let latencies: Vec<u64> = runner
            .rows()
            .iter()
            .filter_map(|r| r.dlr_latency_ms)
            .collect();
        assert_eq!(latencies.len(), 6);
        // Each receipt comes back at once. Measured from the burst start, the
        // last rows would read around a second.
        assert!(latencies.iter().all(|l| *l < 500), "{latencies:?}");
    }

    #[tokio::test]
    async fn counts_an_unmatched_receipt_without_touching_a_row() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            Some(vec![
                PduInput::new(command_id("submit_sm_resp").unwrap(), p.sequence_number)
                    .with_str("message_id", "MINE"),
                PduInput::new(command_id("deliver_sm").unwrap(), 9000)
                    .with_num("esm_class", 4)
                    .with_bytes(
                        "short_message",
                        b"id:SOMEONE-ELSE stat:DELIVRD err:000".to_vec(),
                    ),
            ])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                await_dlr: Duration::from_millis(600),
                ..config(1, 200)
            })
            .await
            .unwrap();

        assert_eq!(done.stats.dlr_unmatched, 1);
        assert_eq!(done.stats.dlr_received, 0);
        assert!(runner.rows()[0].dlr_state.is_none());
    }

    #[tokio::test]
    async fn reports_a_three_part_message_as_one_row_with_three_pdus() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                text: "a".repeat(400),
                await_dlr: Duration::ZERO,
                ..config(2, 200)
            })
            .await
            .unwrap();

        assert_eq!(runner.rows().len(), 2);
        assert!(runner.rows().iter().all(|r| r.pdus == 3));
        assert_eq!(done.stats.messages, 2);
        assert_eq!(done.stats.pdus, 6);
    }

    #[tokio::test]
    async fn marks_the_row_with_the_first_non_zero_status() {
        let smsc = FakeSmsc::new();
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        smsc.on("submit_sm", move |p| {
            let i = seen.fetch_add(1, Ordering::SeqCst) + 1;
            let status = if i == 2 {
                command_status("ESME_RSUBMITFAIL").unwrap()
            } else {
                0
            };
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(status)
            .with_str("message_id", &format!("M{i}"))])
        });
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                text: "a".repeat(400),
                await_dlr: Duration::ZERO,
                ..config(1, 200)
            })
            .await
            .unwrap();

        assert_eq!(runner.rows()[0].pdus, 3);
        assert_eq!(
            runner.rows()[0].command_status,
            Some(command_status("ESME_RSUBMITFAIL").unwrap())
        );
        assert_eq!(done.stats.accepted, 0);
    }

    #[tokio::test]
    async fn reports_the_recipient_warnings_rather_than_swallowing_them() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 20).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                recipients: crate::app::recipients::RecipientSource::Csv {
                    text: "msisdn\n393331111111\nnot a number!\n".into(),
                    column: None,
                },
                await_dlr: Duration::ZERO,
                ..config(2, 200)
            })
            .await
            .unwrap();

        assert_eq!(done.warnings.len(), 1);
        assert!(done.warnings[0].to_lowercase().contains("row 3"));
    }

    #[tokio::test]
    async fn keeps_every_row_for_a_thousand_messages() {
        let smsc = FakeSmsc::new();
        let (session, events) = bound(&smsc, 100).await;
        let runner = runner_for(session, events);
        let done = runner
            .start(BurstConfig {
                recipients: crate::app::recipients::RecipientSource::Range {
                    from: "393330000000".into(),
                    count: 1000,
                },
                rate_per_sec: crate::app::recipients::MAX_RATE,
                await_dlr: Duration::ZERO,
                ..config(1000, crate::app::recipients::MAX_RATE)
            })
            .await
            .unwrap();

        assert_eq!(runner.rows().len(), 1000);
        assert_eq!(done.stats.submitted, 1000);
    }
}
