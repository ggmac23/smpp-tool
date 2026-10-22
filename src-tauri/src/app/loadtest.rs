//! Stress testing an SMSC: several binds at once, and the throughput it will
//! take.
//!
//! The burst engine answers "how did this message fare". This answers a
//! different question — "how much can this SMSC carry" — and that needs
//! several connections, because one session's window is usually the limit long
//! before the SMSC is.
//!
//! Two modes. `Fixed` holds a rate and reports whether it held. `Ramp` climbs
//! until the SMSC gives way and reports the last rate it sustained, which is
//! what "how much does it take" actually means.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::app::recipients::{MAX_BURST, MAX_RATE};
use crate::app::session::{BindOptions, BindState, Session, SessionOptions};
use crate::app::transport::{TlsOptions, Transport, TransportOptions};
use crate::smpp::constants::{command_status_name, ESME_ROK};
use crate::smpp::encode::PduInput;
use crate::smpp::segment::{segment, BodyPlacement, SegmentOptions};
use crate::smpp::text::{Gsm7Packing, SegmentationMode};

/// More binds than this against one SMSC stops being a test and starts being
/// an outage. The limit is this build's, and the refusal says so.
pub const MAX_CONNECTIONS: usize = 16;

const TICK: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbandonOn {
    /// Share of submits coming back with a non-zero status, 0.0 to 1.0.
    pub error_share: f64,
    pub latency_p95_ms: u64,
}

/// `rename_all` names the variants; `rename_all_fields` names their fields.
/// Without the second one the wire form would be snake_case inside a
/// camelCase payload, which compiles and then fails at the button.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum LoadMode {
    Fixed {
        rate_per_sec: u32,
    },
    Ramp {
        start_rate: u32,
        step: u32,
        /// Milliseconds on the wire: the unit belongs in the name.
        #[serde(rename = "stepDurationMs", with = "millis")]
        step_duration: Duration,
        max_rate: u32,
        abandon_on: AbandonOn,
    },
}

mod millis {
    use serde::Deserialize;
    use std::time::Duration;

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}

#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    pub host: String,
    pub port: u16,
    pub tls: TlsOptions,
    pub bind: BindOptions,
    pub connections: usize,
    pub messages: usize,
    pub mode: LoadMode,
    pub window_size: usize,
    pub response_timeout: Duration,
    pub connect_timeout: Duration,
    pub template: PduInput,
    pub destination: String,
    pub text: String,
    pub data_coding: u8,
    pub segmentation: SegmentationMode,
    pub placement: BodyPlacement,
    pub packing: Gsm7Packing,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStats {
    pub index: usize,
    pub bound: bool,
    pub bind_error: Option<String>,
    pub submitted: usize,
    pub accepted: usize,
    pub errors_by_status: BTreeMap<String, usize>,
    pub latency_p50: u64,
    pub latency_p95: u64,
    pub latency_p99: u64,
    pub lost: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RampStep {
    pub requested_rate: u32,
    pub achieved_rate: f64,
    pub submitted: usize,
    pub accepted: usize,
    pub error_share: f64,
    pub latency_p95: u64,
    /// True when this step is the one that gave way.
    pub breached: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadEndReason {
    Complete,
    Stopped,
    /// The SMSC breached a limit the ramp was watching.
    Breached,
    ReachedMaxRate,
    /// Not one connection bound.
    NoConnections,
    /// Every connection dropped.
    ConnectionsLost,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadReport {
    pub connections: usize,
    pub requested_rate_per_sec: u32,
    pub achieved_rate_per_sec: f64,
    /// The highest rate the SMSC carried without breaching a limit.
    pub sustained_rate_per_sec: u32,
    pub messages: usize,
    pub pdus: usize,
    pub submitted: usize,
    pub accepted: usize,
    pub errors_by_status: BTreeMap<String, usize>,
    pub latency_p50: u64,
    pub latency_p95: u64,
    pub latency_p99: u64,
    pub duration_ms: u64,
    pub per_connection: Vec<ConnectionStats>,
    pub steps: Vec<RampStep>,
    pub ended: LoadEndReason,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadProgress {
    pub submitted: usize,
    pub total: usize,
    pub current_rate_per_sec: u32,
    pub achieved_rate_per_sec: f64,
    pub errors: usize,
}

#[derive(Debug, Clone)]
pub enum LoadEvent {
    Progress(LoadProgress),
    Step(RampStep),
    Done(Box<LoadReport>),
}

#[derive(Default)]
struct Tally {
    submitted: AtomicUsize,
    accepted: AtomicUsize,
    errors: Mutex<BTreeMap<String, usize>>,
    latencies: Mutex<Vec<u64>>,
}

impl Tally {
    fn record(&self, latency: u64, status: Option<u32>) {
        self.submitted.fetch_add(1, Ordering::SeqCst);
        self.latencies.lock().unwrap().push(latency);
        match status {
            Some(ESME_ROK) => {
                self.accepted.fetch_add(1, Ordering::SeqCst);
            }
            Some(other) => {
                *self
                    .errors
                    .lock()
                    .unwrap()
                    .entry(command_status_name(other))
                    .or_insert(0) += 1;
            }
            None => {
                *self
                    .errors
                    .lock()
                    .unwrap()
                    .entry("NO_RESPONSE".into())
                    .or_insert(0) += 1;
            }
        }
    }

    /// Latencies come back in the order they were recorded. Sorting here
    /// would destroy the only thing that says which step a sample belongs to.
    fn snapshot(&self) -> (usize, usize, BTreeMap<String, usize>, Vec<u64>) {
        let latencies = self.latencies.lock().unwrap().clone();
        (
            self.submitted.load(Ordering::SeqCst),
            self.accepted.load(Ordering::SeqCst),
            self.errors.lock().unwrap().clone(),
            latencies,
        )
    }
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

struct Connection {
    index: usize,
    session: Option<Arc<Session>>,
    bind_error: Option<String>,
    /// Shared so each send can run as its own task.
    tally: Arc<Tally>,
}

pub struct LoadRunner {
    stopping: Arc<AtomicBool>,
    events: Option<mpsc::UnboundedSender<LoadEvent>>,
}

impl LoadRunner {
    pub fn new() -> Self {
        Self {
            stopping: Arc::new(AtomicBool::new(false)),
            events: None,
        }
    }

    pub fn with_events(mut self, events: mpsc::UnboundedSender<LoadEvent>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    fn check(cfg: &LoadTestConfig) -> Result<u32, String> {
        if cfg.connections > MAX_CONNECTIONS {
            return Err(format!(
                "{} connections is above the {MAX_CONNECTIONS} this build opens at once",
                cfg.connections
            ));
        }
        if cfg.connections < 1 {
            return Err("a load test needs at least one connection".into());
        }
        if cfg.messages > MAX_BURST {
            return Err(format!(
                "{} messages is above the {MAX_BURST} ceiling of this build",
                cfg.messages
            ));
        }
        if cfg.messages < 1 {
            return Err("a load test needs at least one message".into());
        }

        let ceiling = MAX_RATE * cfg.connections as u32;
        let requested = match cfg.mode {
            LoadMode::Fixed { rate_per_sec } => rate_per_sec,
            LoadMode::Ramp { max_rate, .. } => max_rate,
        };
        // A ramp that starts above the ceiling runs its first step there
        // before the maximum ever stops it, so the highest rate it would
        // actually ask for is what gets checked.
        let highest = match cfg.mode {
            LoadMode::Fixed { rate_per_sec } => rate_per_sec,
            LoadMode::Ramp {
                start_rate,
                max_rate,
                ..
            } => start_rate.max(max_rate),
        };
        if highest > ceiling {
            return Err(format!(
                "{highest}/sec is above the {ceiling}/sec these {} connection(s) can carry",
                cfg.connections
            ));
        }
        if requested < 1 {
            return Err("the rate must be at least 1 message per second".into());
        }
        Ok(requested)
    }

    pub async fn start(&self, cfg: LoadTestConfig) -> Result<LoadReport, String> {
        let requested = Self::check(&cfg)?;
        self.stopping.store(false, Ordering::SeqCst);

        let connections = self.open_connections(&cfg).await;
        let live: Vec<usize> = connections
            .iter()
            .enumerate()
            .filter(|(_, c)| c.session.is_some())
            .map(|(i, _)| i)
            .collect();

        let started = Instant::now();

        if live.is_empty() {
            return Ok(self.finish(
                &cfg,
                requested,
                &connections,
                Vec::new(),
                0,
                LoadEndReason::NoConnections,
                started,
                0,
                Duration::ZERO,
            ));
        }

        let sending_total;
        let (steps, ended, sent) = match cfg.mode {
            LoadMode::Fixed { rate_per_sec } => {
                let outcome = self
                    .run_at(&cfg, &connections, &live, rate_per_sec, cfg.messages, None)
                    .await;
                sending_total = outcome.sending;
                let ended = if outcome.lost {
                    LoadEndReason::ConnectionsLost
                } else if self.stopping.load(Ordering::SeqCst) {
                    LoadEndReason::Stopped
                } else {
                    LoadEndReason::Complete
                };
                (Vec::new(), ended, outcome.sent)
            }
            LoadMode::Ramp { .. } => {
                let (steps, ended, sent, sending) = self.run_ramp(&cfg, &connections, &live).await;
                sending_total = sending;
                (steps, ended, sent)
            }
        };

        let sustained = sustained_rate(
            &steps,
            &totals_for_sustained(&connections),
            requested,
            &cfg.mode,
        );
        let report = self.finish(
            &cfg,
            requested,
            &connections,
            steps,
            sustained,
            ended,
            started,
            sent,
            sending_total,
        );
        Ok(report)
    }

    async fn open_connections(&self, cfg: &LoadTestConfig) -> Vec<Connection> {
        let mut opening = Vec::new();
        for index in 0..cfg.connections {
            let cfg = cfg.clone();
            opening.push(async move {
                let opened = Transport::connect(TransportOptions {
                    host: cfg.host.clone(),
                    port: cfg.port,
                    tls: cfg.tls.clone(),
                    connect_timeout: cfg.connect_timeout,
                })
                .await;

                match opened {
                    Err(e) => Connection {
                        index,
                        session: None,
                        bind_error: Some(e),
                        tally: Arc::new(Tally::default()),
                    },
                    Ok((transport, events)) => {
                        let (session, session_events) = Session::new(
                            transport,
                            events,
                            SessionOptions {
                                window_size: cfg.window_size,
                                response_timeout: cfg.response_timeout,
                            },
                        );
                        // Nothing here reads the session's events, but the
                        // channel must be drained or it grows without bound.
                        tokio::spawn(async move {
                            let mut events = session_events;
                            while events.recv().await.is_some() {}
                        });

                        match session.bind(cfg.bind.clone()).await {
                            Ok(_) => Connection {
                                index,
                                session: Some(Arc::new(session)),
                                bind_error: None,
                                tally: Arc::new(Tally::default()),
                            },
                            // A connection that will not bind is reported, not
                            // retried, and the test carries on without it.
                            Err(e) => Connection {
                                index,
                                session: None,
                                bind_error: Some(e),
                                tally: Arc::new(Tally::default()),
                            },
                        }
                    }
                }
            });
        }
        futures_util::future::join_all(opening).await
    }

    /// Send `budget` messages at `rate`, or until `deadline`. Returns how many
    /// were started.
    /// Sends `budget` messages at `rate`, round-robin over `live`. Returns
    /// how many it handed out and whether it stopped because every
    /// connection had gone.
    async fn run_at(
        &self,
        cfg: &LoadTestConfig,
        connections: &[Connection],
        live: &[usize],
        rate: u32,
        budget: usize,
        deadline: Option<Instant>,
    ) -> StepOutcome {
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

        // Each send is its own task, so they run while the pacing loop waits
        // for its next tick and spread across the runtime's worker threads. A
        // plain Vec of futures would only be polled at the end, and the whole
        // load would land in one clump no matter what rate was asked for.
        let mut inflight: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let mut sent = 0usize;
        let mut next = 0usize;
        let mut lost = false;
        let sending_started = Instant::now();
        // Messages owed but not yet whole. A tick is 20ms, so a rate is a
        // fractional number of messages per tick; rounding it each time
        // quantises every rate above 50/sec to a multiple of 50. Carrying the
        // remainder spends it on the next tick instead.
        let mut owed = 0.0f64;
        // When the next tick is due, measured from the start rather than from
        // the end of the last one. Sleeping for "the rest of this tick" lets
        // the cost of the tick itself accumulate, and the run comes out a few
        // per cent short at every rate — which reads as the SMSC and is not.
        let mut due = sending_started;

        while sent < budget {
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            // An SMSC that goes away mid-run must stop the test, not have the
            // rest of the budget poured onto dead sockets and then reported
            // as a load it carried.
            if live.iter().all(|i| {
                connections[*i]
                    .session
                    .as_ref()
                    .map(|s| s.state() != BindState::Bound)
                    .unwrap_or(true)
            }) {
                lost = true;
                break;
            }
            if deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                break;
            }

            let per_tick = rate as f64 * TICK.as_secs_f64();
            let (to_send, wait) = if per_tick >= 1.0 {
                owed += per_tick;
                let whole = owed.floor();
                owed -= whole;
                (whole as usize, TICK)
            } else {
                // Below one a tick, sending one and waiting the whole gap is
                // exact and costs fewer wakeups.
                (1, Duration::from_secs_f64(1.0 / rate.max(1) as f64))
            };

            for _ in 0..to_send {
                if sent >= budget || self.stopping.load(Ordering::SeqCst) {
                    break;
                }
                // Round robin, so one slow connection cannot stall the others:
                // its window fills and its own sends queue behind it.
                let index = live[next % live.len()];
                next += 1;
                sent += 1;

                let Some(session) = connections[index].session.clone() else {
                    continue;
                };
                let tally = connections[index].tally.clone();
                let template = cfg.template.clone();
                let destination = cfg.destination.clone();
                let data_coding = cfg.data_coding;
                let segments = segments.clone();

                inflight.push(tokio::spawn(async move {
                    for seg in &segments {
                        let mut input = template.clone();
                        for (name, value) in &seg.fields {
                            input.fields.insert(name.clone(), value.clone());
                        }
                        input = input
                            .with_str("destination_addr", &destination)
                            .with_num("data_coding", data_coding as u32);
                        input.tlvs.extend(seg.tlvs.clone());

                        let at = Instant::now();
                        match session.send(input).await {
                            Ok(resp) => tally
                                .record(at.elapsed().as_millis() as u64, Some(resp.command_status)),
                            Err(_) => {
                                tally.record(at.elapsed().as_millis() as u64, None);
                                break;
                            }
                        }
                    }
                }));
            }

            due += wait;
            let now = Instant::now();
            if due > now {
                tokio::time::sleep(due - now).await;
            } else if now - due > wait {
                // Too far behind to make up: carry on from here rather than
                // firing a burst to catch a schedule that has run away.
                due = now;
            }
            self.emit_progress(connections, budget, rate);
        }

        let sending = sending_started.elapsed();

        for handle in inflight {
            let _ = handle.await;
        }
        self.emit_progress(connections, budget, rate);
        StepOutcome {
            sent,
            lost,
            sending,
        }
    }

    async fn run_ramp(
        &self,
        cfg: &LoadTestConfig,
        connections: &[Connection],
        live: &[usize],
    ) -> (Vec<RampStep>, LoadEndReason, usize, Duration) {
        let LoadMode::Ramp {
            start_rate,
            step,
            step_duration,
            max_rate,
            abandon_on,
        } = cfg.mode
        else {
            unreachable!("run_ramp is only called for a ramp");
        };

        let mut steps: Vec<RampStep> = Vec::new();
        let mut rate = start_rate.max(1);
        let mut total_sent = 0usize;
        let mut sending_total = Duration::ZERO;
        let mut before = tally_totals(connections);

        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return (steps, LoadEndReason::Stopped, total_sent, sending_total);
            }
            if total_sent >= cfg.messages {
                return (steps, LoadEndReason::Complete, total_sent, sending_total);
            }
            if live.iter().all(|i| {
                connections[*i]
                    .session
                    .as_ref()
                    .map(|s| s.state() != BindState::Bound)
                    .unwrap_or(true)
            }) {
                return (
                    steps,
                    LoadEndReason::ConnectionsLost,
                    total_sent,
                    sending_total,
                );
            }

            let budget = (cfg.messages - total_sent)
                .min(((rate as f64) * step_duration.as_secs_f64()).ceil() as usize)
                .max(1);
            let started = Instant::now();
            let outcome = self
                .run_at(
                    cfg,
                    connections,
                    live,
                    rate,
                    budget,
                    Some(started + step_duration),
                )
                .await;
            total_sent += outcome.sent;
            sending_total += outcome.sending;
            if outcome.lost {
                return (
                    steps,
                    LoadEndReason::ConnectionsLost,
                    total_sent,
                    sending_total,
                );
            }

            let after = tally_totals(connections);
            let measured = measure_step(&before, &after, rate, outcome.sending);
            before = after;

            let short = fell_short(&measured);
            let breached = measured.error_share > abandon_on.error_share
                || measured.latency_p95 > abandon_on.latency_p95_ms
                || short;

            let reason = if !breached {
                None
            } else if measured.error_share > abandon_on.error_share {
                Some(format!(
                    "{:.0}% of submits came back with an error, above the {:.0}% allowed",
                    measured.error_share * 100.0,
                    abandon_on.error_share * 100.0
                ))
            } else if measured.latency_p95 > abandon_on.latency_p95_ms {
                Some(format!(
                    "p95 latency reached {}ms, above the {}ms allowed",
                    measured.latency_p95, abandon_on.latency_p95_ms
                ))
            } else {
                Some(format!(
                    "asked for {}/sec and only {:.1}/sec went out, so the SMSC is not \
                     taking more however few errors it returns",
                    measured.requested_rate, measured.achieved_rate
                ))
            };

            let record = RampStep {
                breached,
                reason,
                ..measured
            };
            if let Some(events) = &self.events {
                let _ = events.send(LoadEvent::Step(record.clone()));
            }
            steps.push(record);

            if breached {
                return (steps, LoadEndReason::Breached, total_sent, sending_total);
            }
            if rate >= max_rate {
                return (
                    steps,
                    LoadEndReason::ReachedMaxRate,
                    total_sent,
                    sending_total,
                );
            }
            rate = (rate + step).min(max_rate);
        }
    }

    fn emit_progress(&self, connections: &[Connection], total: usize, rate: u32) {
        let Some(events) = &self.events else { return };
        let totals = tally_totals(connections);
        let _ = events.send(LoadEvent::Progress(LoadProgress {
            submitted: totals.submitted,
            total,
            current_rate_per_sec: rate,
            achieved_rate_per_sec: 0.0,
            errors: totals.errors.values().sum(),
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        cfg: &LoadTestConfig,
        requested: u32,
        connections: &[Connection],
        steps: Vec<RampStep>,
        sustained: u32,
        ended: LoadEndReason,
        started: Instant,
        messages: usize,
        // Time spent handing work out. The rate is measured over this, not
        // over the whole run: the tail coming back is not sending.
        sending: Duration,
    ) -> LoadReport {
        let elapsed = started.elapsed();
        let totals = tally_totals(connections);
        let sorted_all = totals.sorted_latencies();

        let per_connection = connections
            .iter()
            .map(|c| {
                let (submitted, accepted, errors, latencies) = c.tally.snapshot();
                ConnectionStats {
                    index: c.index,
                    bound: c.session.is_some(),
                    bind_error: c.bind_error.clone(),
                    submitted,
                    accepted,
                    errors_by_status: errors,
                    latency_p50: percentile(&latencies, 50),
                    latency_p95: percentile(&latencies, 95),
                    latency_p99: percentile(&latencies, 99),
                    lost: c
                        .session
                        .as_ref()
                        .map(|s| s.state() != BindState::Bound)
                        .unwrap_or(false),
                }
            })
            .collect();

        // The rate is what went out while sending, not what went out across a
        // run that ends waiting for the last responses.
        let seconds = if sending.is_zero() {
            elapsed.as_secs_f64()
        } else {
            sending.as_secs_f64()
        };
        let report = LoadReport {
            connections: cfg.connections,
            requested_rate_per_sec: requested,
            achieved_rate_per_sec: if seconds > 0.0 {
                totals.submitted as f64 / seconds
            } else {
                0.0
            },
            sustained_rate_per_sec: sustained,
            messages,
            pdus: totals.submitted,
            submitted: totals.submitted,
            accepted: totals.accepted,
            errors_by_status: totals.errors,
            latency_p50: percentile(&sorted_all, 50),
            latency_p95: percentile(&sorted_all, 95),
            latency_p99: percentile(&sorted_all, 99),
            duration_ms: elapsed.as_millis() as u64,
            per_connection,
            steps,
            ended,
        };

        if let Some(events) = &self.events {
            let _ = events.send(LoadEvent::Done(Box::new(report.clone())));
        }
        report
    }
}

impl Default for LoadRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// What one run of the pacing loop did.
struct StepOutcome {
    sent: usize,
    lost: bool,
    /// How long the loop spent handing work out, which is what a rate is
    /// measured over. The time after that is spent waiting for the tail to
    /// come back: a real SMSC answers after a round trip, so counting that
    /// against the rate makes a run that paced perfectly look slow.
    sending: Duration,
}

#[derive(Default, Clone)]
struct Totals {
    submitted: usize,
    accepted: usize,
    errors: BTreeMap<String, usize>,
    /// One list per connection, each in the order its samples were taken.
    /// A ramp step's own samples are the part of each list that was not
    /// there when the step began, so the order has to survive.
    per_connection: Vec<Vec<u64>>,
}

impl Totals {
    /// Every sample, sorted, for the percentiles over a whole run.
    fn sorted_latencies(&self) -> Vec<u64> {
        let mut all: Vec<u64> = self.per_connection.iter().flatten().copied().collect();
        all.sort_unstable();
        all
    }
}

fn tally_totals(connections: &[Connection]) -> Totals {
    let mut totals = Totals::default();
    for c in connections {
        let (submitted, accepted, errors, latencies) = c.tally.snapshot();
        totals.submitted += submitted;
        totals.accepted += accepted;
        for (name, count) in errors {
            *totals.errors.entry(name).or_insert(0) += count;
        }
        totals.per_connection.push(latencies);
    }
    totals
}

fn measure_step(before: &Totals, after: &Totals, rate: u32, elapsed: Duration) -> RampStep {
    let submitted = after.submitted.saturating_sub(before.submitted);
    let accepted = after.accepted.saturating_sub(before.accepted);
    let errors = submitted.saturating_sub(accepted);
    let seconds = elapsed.as_secs_f64();

    // A step's own latencies are what each connection added since the step
    // began. Taken from the combined list by count they would be somebody
    // else's samples, and taken from a sorted list they would be the worst of
    // the whole run — which is how a fast step inherits a slow one's outlier
    // and gets blamed for it.
    let mut sorted: Vec<u64> = Vec::new();
    for (index, after_one) in after.per_connection.iter().enumerate() {
        let already = before
            .per_connection
            .get(index)
            .map(Vec::len)
            .unwrap_or(0)
            .min(after_one.len());
        sorted.extend_from_slice(&after_one[already..]);
    }
    sorted.sort_unstable();

    RampStep {
        requested_rate: rate,
        achieved_rate: if seconds > 0.0 {
            submitted as f64 / seconds
        } else {
            0.0
        },
        submitted,
        accepted,
        error_share: if submitted > 0 {
            errors as f64 / submitted as f64
        } else {
            0.0
        },
        latency_p95: percentile(&sorted, 95),
        breached: false,
        reason: None,
    }
}

/// How far short of its requested rate a step may fall and still count as
/// having held it. A tenth covers the jitter of a tick-based pacer; beyond
/// that the SMSC is simply not taking the traffic, however few errors it
/// returns.
const SHORTFALL_TOLERANCE: f64 = 0.9;

/// Whether a step fell short of the rate it asked for. A step with no errors
/// and good latency that still cannot be fed at the requested rate has not
/// sustained it — the queue is absorbing the difference, and climbing further
/// would only measure the queue.
fn fell_short(step: &RampStep) -> bool {
    step.requested_rate > 0 && step.achieved_rate < step.requested_rate as f64 * SHORTFALL_TOLERANCE
}

/// Totals reduced to what the sustained-rate verdict needs.
fn totals_for_sustained(connections: &[Connection]) -> (usize, usize) {
    let totals = tally_totals(connections);
    (totals.submitted, totals.accepted)
}

/// The highest rate the SMSC carried without breaching a limit. A ramp that
/// breaks on its first step sustained nothing, and a fixed run that was
/// refused throughout did not sustain the rate it asked for — saying it did
/// would be the report asserting the opposite of what happened.
fn sustained_rate(
    steps: &[RampStep],
    (submitted, accepted): &(usize, usize),
    requested: u32,
    mode: &LoadMode,
) -> u32 {
    match mode {
        LoadMode::Fixed { rate_per_sec } => {
            if *submitted == 0 {
                return 0;
            }
            let accepted_share = *accepted as f64 / *submitted as f64;
            // The same bar a ramp uses by default: more than a couple of
            // percent refused is not a rate that was carried.
            if accepted_share >= 0.98 {
                *rate_per_sec
            } else {
                (*rate_per_sec as f64 * accepted_share).floor() as u32
            }
        }
        LoadMode::Ramp { .. } => steps
            .iter()
            .filter(|s| !s.breached)
            .map(|s| s.requested_rate)
            .max()
            .unwrap_or(0)
            .min(requested),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session::BindMode;
    use crate::smpp::constants::{command_id, command_status, DataCoding, InterfaceVersion};
    use crate::smpp::encode::PduInput;
    use crate::smpp::segment::BodyPlacement;
    use crate::smpp::text::{Gsm7Packing, SegmentationMode};
    use crate::testing::fake_smsc::FakeSmsc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn bind_opts() -> BindOptions {
        BindOptions {
            system_id: "esme".into(),
            password: "pw".into(),
            system_type: String::new(),
            interface_version: InterfaceVersion::V34,
            mode: BindMode::Transceiver,
            addr_ton: 0,
            addr_npi: 0,
            address_range: String::new(),
        }
    }

    fn config(port: u16, connections: usize, messages: usize, mode: LoadMode) -> LoadTestConfig {
        LoadTestConfig {
            host: "127.0.0.1".into(),
            port,
            tls: Default::default(),
            bind: bind_opts(),
            connections,
            messages,
            mode,
            window_size: 20,
            response_timeout: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(3),
            template: PduInput::new(command_id("submit_sm").unwrap(), 0)
                .with_str("source_addr", "SENDER"),
            destination: "393331234567".into(),
            text: "load".into(),
            data_coding: DataCoding::GSM_DEFAULT,
            segmentation: SegmentationMode::None,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
        }
    }

    fn fixed(rate: u32) -> LoadMode {
        LoadMode::Fixed { rate_per_sec: rate }
    }

    fn ramp(start: u32, step: u32, max: u32, error_share: f64) -> LoadMode {
        LoadMode::Ramp {
            start_rate: start,
            step,
            step_duration: Duration::from_millis(300),
            max_rate: max,
            abandon_on: AbandonOn {
                error_share,
                latency_p95_ms: 60_000,
            },
        }
    }

    #[test]
    fn reads_the_modes_exactly_as_the_renderer_sends_them() {
        // These two payloads are what `buildLoadMode` produces. Field names
        // that look right in Rust and wrong on the wire are invisible until
        // someone presses the button, so they are pinned here.
        let fixed: LoadMode =
            serde_json::from_value(serde_json::json!({ "kind": "fixed", "ratePerSec": 200 }))
                .expect("the fixed mode must deserialise");
        assert!(matches!(fixed, LoadMode::Fixed { rate_per_sec: 200 }));

        let ramp: LoadMode = serde_json::from_value(serde_json::json!({
            "kind": "ramp",
            "startRate": 50,
            "step": 50,
            "stepDurationMs": 10000,
            "maxRate": 500,
            "abandonOn": { "errorShare": 0.02, "latencyP95Ms": 2000 }
        }))
        .expect("the ramp mode must deserialise");

        match ramp {
            LoadMode::Ramp {
                start_rate,
                step,
                step_duration,
                max_rate,
                abandon_on,
            } => {
                assert_eq!(start_rate, 50);
                assert_eq!(step, 50);
                assert_eq!(step_duration, Duration::from_secs(10));
                assert_eq!(max_rate, 500);
                assert_eq!(abandon_on.error_share, 0.02);
                assert_eq!(abandon_on.latency_p95_ms, 2000);
            }
            other => panic!("expected a ramp, got {other:?}"),
        }
    }

    fn totals_with(per_connection: Vec<Vec<u64>>) -> Totals {
        let mut totals = Totals::default();
        for latencies in per_connection {
            totals.submitted += latencies.len();
            totals.accepted += latencies.len();
            totals.per_connection.push(latencies);
        }
        totals
    }

    #[test]
    fn a_step_that_could_not_keep_up_did_not_sustain_its_rate() {
        // Found against a real SMSC: the ramp asked for 20/sec, achieved
        // 13.3/sec, saw no errors and no latency breach, and reported a
        // sustained rate of 20. A fake SMSC answers instantly, so achieved
        // always equals requested and this could never show up there.
        let before = totals_with(vec![vec![150; 10]]);
        let mut after = before.clone();
        after.per_connection[0].extend(std::iter::repeat_n(150u64, 13));
        after.submitted += 13;
        after.accepted += 13;

        let step = measure_step(&before, &after, 20, Duration::from_secs(1));
        assert!(
            (step.achieved_rate - 13.0).abs() < 0.5,
            "achieved {}",
            step.achieved_rate
        );
        assert!(
            fell_short(&step),
            "13 of 20 is a shortfall this check should not tolerate"
        );
    }

    #[test]
    fn a_step_that_kept_up_is_not_called_short() {
        let before = totals_with(vec![vec![10; 10]]);
        let mut after = before.clone();
        after.per_connection[0].extend(std::iter::repeat_n(10u64, 20));
        after.submitted += 20;
        after.accepted += 20;

        let step = measure_step(&before, &after, 20, Duration::from_secs(1));
        assert!(!fell_short(&step), "20 of 20 is not a shortfall");
    }

    #[test]
    fn a_steps_latency_is_its_own_samples_not_the_runs_worst() {
        // The first step was slow throughout — a cold SMSC, a TLS handshake,
        // a cache that had not warmed. The second step is fast. If a step's
        // samples are taken as a count from the end of a sorted list, the
        // second step inherits the first one's latencies wholesale and is
        // blamed for them: the ramp then calls a rate broken that the SMSC
        // was carrying comfortably, and reports a sustained rate one step too
        // low. That number is the whole point of the feature.
        let before = totals_with(vec![vec![900; 100], vec![900; 100]]);

        let mut after = before.clone();
        for connection in &mut after.per_connection {
            connection.extend(std::iter::repeat_n(8u64, 100));
        }
        after.submitted += 200;
        after.accepted += 200;

        let step = measure_step(&before, &after, 100, Duration::from_secs(2));
        assert_eq!(step.submitted, 200, "the step's own submits");
        assert_eq!(
            step.latency_p95, 8,
            "the second step ran at 8 ms throughout; {} ms is the first step's",
            step.latency_p95
        );
    }

    #[test]
    fn a_steps_latency_still_sees_its_own_spike() {
        // The other direction: a step that really did slow down must say so.
        let before = totals_with(vec![vec![8; 100], vec![8; 100]]);
        let mut after = before.clone();
        for connection in &mut after.per_connection {
            connection.extend(std::iter::repeat_n(900u64, 100));
        }
        after.submitted += 200;
        after.accepted += 200;

        let step = measure_step(&before, &after, 100, Duration::from_secs(2));
        assert_eq!(step.latency_p95, 900);
    }

    #[tokio::test]
    async fn says_the_connections_were_lost_rather_than_claiming_it_finished() {
        // The SMSC goes away mid-run. A fixed-rate test used to spend the
        // rest of its budget on dead sockets and then report Complete, which
        // tells the user their SMSC carried a load it never saw.
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = Arc::new(LoadRunner::new());

        let hangup = smsc.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            hangup.drop_connections();
        });

        let report = runner
            .start(config(port, 2, 4000, fixed(80)))
            .await
            .unwrap();

        assert_eq!(report.ended, LoadEndReason::ConnectionsLost);
        assert!(
            report.submitted < 4000,
            "it should have stopped early, not spent the whole budget on dead sockets"
        );
    }

    #[test]
    fn checks_the_ramp_ceiling_against_where_it_starts_as_well_as_where_it_ends() {
        // A ramp that starts above the ceiling runs its first step there
        // before the maximum ever stops it, which is the one thing the
        // refusal claims cannot happen.
        let runner = LoadRunner::new();
        let mut cfg = config(1, 1, 10, fixed(10));
        cfg.mode = LoadMode::Ramp {
            start_rate: MAX_RATE * 10,
            step: 50,
            step_duration: Duration::from_millis(100),
            max_rate: 100,
            abandon_on: AbandonOn {
                error_share: 0.02,
                latency_p95_ms: 2000,
            },
        };
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(runner.start(cfg))
            .unwrap_err();
        assert!(err.contains("above"), "{err}");
    }

    #[tokio::test]
    async fn a_fixed_run_does_not_claim_to_have_sustained_a_rate_it_never_held() {
        // Every submit comes back throttled. Reporting the requested rate as
        // sustained would be the report asserting the opposite of what
        // happened.
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", move |p| {
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RTHROTTLED").unwrap())])
        });
        let port = smsc.listen().await;

        let report = LoadRunner::new()
            .start(config(port, 1, 20, fixed(200)))
            .await
            .unwrap();
        assert_eq!(report.accepted, 0);
        assert_eq!(
            report.sustained_rate_per_sec, 0,
            "nothing was carried, so nothing was sustained"
        );
    }

    #[tokio::test]
    async fn the_done_event_carries_the_same_message_count_as_the_report() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let runner = LoadRunner::new().with_events(tx);

        let report = runner.start(config(port, 1, 10, fixed(200))).await.unwrap();
        assert_eq!(report.messages, 10);

        let mut done = None;
        while let Ok(event) = rx.try_recv() {
            if let LoadEvent::Done(r) = event {
                done = Some(r);
            }
        }
        let done = done.expect("a done event");
        assert_eq!(
            done.messages, report.messages,
            "the event is what the UI draws; it cannot disagree with the report"
        );
    }

    #[tokio::test]
    async fn measures_the_rate_over_the_sending_not_over_the_waiting_afterwards() {
        // A real SMSC answers after a round trip, so when the last message
        // has been handed out there are still several in flight. Counting
        // the time spent waiting for those against the rate makes a run that
        // paced perfectly look slow — and with the shortfall check in place,
        // makes a healthy step look like one the SMSC refused.
        let smsc = FakeSmsc::new();
        smsc.set_reply_delay(Duration::from_millis(200));
        let port = smsc.listen().await;

        let report = LoadRunner::new()
            .start(config(port, 1, 20, fixed(20)))
            .await
            .unwrap();

        assert_eq!(report.submitted, 20);
        // Twenty at twenty a second is one second of sending. The 200ms spent
        // waiting for the tail is not part of that second.
        assert!(
            report.achieved_rate_per_sec > 17.0,
            "20/sec paced correctly came out as {:.1}/sec, which is the wait, not the rate",
            report.achieved_rate_per_sec
        );
    }

    #[tokio::test]
    async fn sends_the_rate_that_was_asked_for_between_the_tick_multiples() {
        // The pacer works in 20ms ticks, so a rate is a number of messages
        // per tick. Rounding that number quantises every rate above 50/sec to
        // a multiple of 50: a request for 60 goes out at 50, and one for 75
        // at 100. For a tool whose job is to state a rate exactly, the
        // measurement would then be of the pacer rather than the SMSC.
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;

        let report = LoadRunner::new()
            .start(config(port, 2, 120, fixed(60)))
            .await
            .unwrap();

        assert_eq!(report.submitted, 120);
        assert!(
            (report.achieved_rate_per_sec - 60.0).abs() < 6.0,
            "asked for 60/sec and sent at {:.1}/sec",
            report.achieved_rate_per_sec
        );
    }

    #[tokio::test]
    async fn holds_the_rate_without_drifting_behind_its_own_clock() {
        // Sleeping for "the rest of this tick" lets the cost of the tick
        // itself accumulate: every tick runs a little long, and the run comes
        // out a few per cent short of the rate at every rate. Against a real
        // SMSC that showed as a uniform 8% shortfall from 25/sec to 150/sec,
        // which looks like the server and is not.
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;

        let report = LoadRunner::new()
            .start(config(port, 2, 200, fixed(100)))
            .await
            .unwrap();

        assert_eq!(report.submitted, 200);
        assert!(
            report.achieved_rate_per_sec > 96.0,
            "asked for 100/sec and held {:.1}/sec; the drift is the pacer's own",
            report.achieved_rate_per_sec
        );
    }

    #[tokio::test]
    async fn binds_every_connection_it_was_asked_for() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 3, 30, fixed(100))).await.unwrap();

        assert_eq!(report.per_connection.len(), 3);
        assert!(report.per_connection.iter().all(|c| c.bound));
        assert_eq!(smsc.connection_count(), 3);
    }

    #[tokio::test]
    async fn spreads_the_messages_across_the_connections() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 3, 30, fixed(200))).await.unwrap();

        assert_eq!(report.submitted, 30);
        // No connection did all the work, and none sat idle.
        assert!(report.per_connection.iter().all(|c| c.submitted > 0));
        assert!(report.per_connection.iter().all(|c| c.submitted < 30));
    }

    #[tokio::test]
    async fn holds_a_fixed_rate_and_reports_what_it_achieved() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 2, 40, fixed(100))).await.unwrap();

        assert_eq!(report.requested_rate_per_sec, 100);
        assert_eq!(report.submitted, 40);
        assert!(
            report.achieved_rate_per_sec > 20.0,
            "{}",
            report.achieved_rate_per_sec
        );
        assert_eq!(report.ended, LoadEndReason::Complete);
    }

    #[tokio::test]
    async fn paces_the_sending_instead_of_bursting_it_at_the_end() {
        // 10 messages at 10/sec must be spread over about a second. Lazy
        // futures queued by the pacing loop and only polled at the end would
        // satisfy every rate assertion while putting the whole load on the
        // wire in one clump — the exact opposite of a load test.
        let smsc = FakeSmsc::new();
        let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let seen = arrivals.clone();
        smsc.on("submit_sm", move |_p| {
            seen.lock().unwrap().push(Instant::now());
            None
        });
        let port = smsc.listen().await;

        let runner = LoadRunner::new();
        let report = runner.start(config(port, 1, 10, fixed(10))).await.unwrap();
        assert_eq!(report.submitted, 10);

        let at = arrivals.lock().unwrap().clone();
        assert_eq!(at.len(), 10, "the SMSC did not see every submit");
        let spread = at.last().unwrap().duration_since(*at.first().unwrap());
        assert!(
            spread >= Duration::from_millis(600),
            "the submits arrived within {spread:?} of each other, so nothing was paced"
        );
    }

    #[tokio::test]
    async fn counts_errors_by_status_across_every_connection() {
        let smsc = FakeSmsc::new();
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        smsc.on("submit_sm", move |p| {
            let i = seen.fetch_add(1, Ordering::SeqCst) + 1;
            let status = if i.is_multiple_of(2) {
                command_status("ESME_RTHROTTLED").unwrap()
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
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 2, 20, fixed(200))).await.unwrap();

        assert_eq!(report.submitted, 20);
        assert_eq!(report.accepted, 10);
        assert_eq!(report.errors_by_status.get("ESME_RTHROTTLED"), Some(&10));
    }

    #[tokio::test]
    async fn keeps_a_per_connection_breakdown() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 2, 20, fixed(200))).await.unwrap();

        let total: usize = report.per_connection.iter().map(|c| c.submitted).sum();
        assert_eq!(total, report.submitted);
        assert!(report
            .per_connection
            .iter()
            .all(|c| c.latency_p95 >= c.latency_p50));
    }

    #[tokio::test]
    async fn refuses_more_connections_than_the_ceiling_before_binding_anything() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();

        let err = runner
            .start(config(port, MAX_CONNECTIONS + 1, 10, fixed(100)))
            .await
            .unwrap_err();
        assert!(err.contains(&MAX_CONNECTIONS.to_string()), "{err}");
        assert_eq!(smsc.connection_count(), 0);
    }

    #[tokio::test]
    async fn refuses_a_rate_above_what_the_connections_can_carry() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();

        let ceiling = MAX_RATE * 2;
        let err = runner
            .start(config(port, 2, 10, fixed(ceiling + 1)))
            .await
            .unwrap_err();
        assert!(err.contains(&ceiling.to_string()), "{err}");
    }

    #[tokio::test]
    async fn refuses_more_messages_than_the_ceiling() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let err = runner
            .start(config(port, 1, MAX_BURST + 1, fixed(100)))
            .await
            .unwrap_err();
        assert!(err.contains(&MAX_BURST.to_string()), "{err}");
    }

    #[tokio::test]
    async fn ends_with_no_connections_when_every_bind_is_refused() {
        let smsc = FakeSmsc::new();
        smsc.on("bind_transceiver", |p| {
            Some(vec![PduInput::new(
                command_id("bind_transceiver_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(command_status("ESME_RBINDFAIL").unwrap())])
        });
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 2, 10, fixed(100))).await.unwrap();

        assert_eq!(report.ended, LoadEndReason::NoConnections);
        assert_eq!(report.submitted, 0);
        assert!(report.per_connection.iter().all(|c| !c.bound));
        assert!(report.per_connection.iter().all(|c| c.bind_error.is_some()));
    }

    #[tokio::test]
    async fn climbs_until_the_error_share_breaches_the_threshold() {
        let smsc = FakeSmsc::new();
        // Everything above a modest pace comes back throttled.
        smsc.on("submit_sm", |p| {
            let throttle = p.sequence_number % 2 == 0;
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(if throttle {
                command_status("ESME_RTHROTTLED").unwrap()
            } else {
                0
            })
            .with_str("message_id", "M")])
        });
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner
            .start(config(port, 1, 2000, ramp(20, 20, 400, 0.2)))
            .await
            .unwrap();

        assert_eq!(report.ended, LoadEndReason::Breached);
        assert!(!report.steps.is_empty());
        assert!(report.steps.last().unwrap().breached);
    }

    #[tokio::test]
    async fn reports_the_last_sustained_rate_rather_than_the_one_that_broke() {
        let smsc = FakeSmsc::new();
        smsc.on("submit_sm", |p| {
            let throttle = p.sequence_number % 2 == 0;
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                p.sequence_number,
            )
            .with_status(if throttle {
                command_status("ESME_RTHROTTLED").unwrap()
            } else {
                0
            })
            .with_str("message_id", "M")])
        });
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner
            .start(config(port, 1, 2000, ramp(20, 20, 400, 0.2)))
            .await
            .unwrap();

        let breaking = report.steps.last().unwrap().requested_rate;
        assert!(report.sustained_rate_per_sec < breaking);
    }

    #[tokio::test]
    async fn stops_climbing_at_the_maximum_even_when_nothing_breaks() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner
            .start(config(port, 1, 5000, ramp(50, 50, 150, 0.5)))
            .await
            .unwrap();

        assert_eq!(report.ended, LoadEndReason::ReachedMaxRate);
        assert_eq!(report.steps.last().unwrap().requested_rate, 150);
        assert!(!report.steps.iter().any(|s| s.breached));
        // The ceiling held, so the ceiling is what it sustained.
        assert_eq!(report.sustained_rate_per_sec, 150);
    }

    #[tokio::test]
    async fn measures_the_achieved_rate_over_the_sending() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = LoadRunner::new();
        let report = runner.start(config(port, 2, 20, fixed(100))).await.unwrap();

        // Twenty messages at 100/sec is about 0.2s. Anything near zero would
        // mean the clock covered more than the sending.
        assert!(
            report.achieved_rate_per_sec > 20.0,
            "{}",
            report.achieved_rate_per_sec
        );
        assert!(report.duration_ms >= 100);
    }

    #[tokio::test]
    async fn stops_where_it_was_when_asked_to() {
        let smsc = FakeSmsc::new();
        let port = smsc.listen().await;
        let runner = Arc::new(LoadRunner::new());

        let stopper = runner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            stopper.stop();
        });

        let report = runner
            .start(config(port, 1, 5000, fixed(100)))
            .await
            .unwrap();
        assert_eq!(report.ended, LoadEndReason::Stopped);
        assert!(report.submitted < 5000);
    }
}
