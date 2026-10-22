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

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LoadMode {
    Fixed {
        rate_per_sec: u32,
    },
    Ramp {
        start_rate: u32,
        step: u32,
        #[serde(with = "millis")]
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

    fn snapshot(&self) -> (usize, usize, BTreeMap<String, usize>, Vec<u64>) {
        let mut latencies = self.latencies.lock().unwrap().clone();
        latencies.sort_unstable();
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
        if requested > ceiling {
            return Err(format!(
                "{requested}/sec is above the {ceiling}/sec these {} connection(s) can carry",
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
            ));
        }

        let (steps, ended, sent) = match cfg.mode {
            LoadMode::Fixed { rate_per_sec } => {
                let sent = self
                    .run_at(&cfg, &connections, &live, rate_per_sec, cfg.messages, None)
                    .await;
                let ended = if self.stopping.load(Ordering::SeqCst) {
                    LoadEndReason::Stopped
                } else {
                    LoadEndReason::Complete
                };
                (Vec::new(), ended, sent)
            }
            LoadMode::Ramp { .. } => self.run_ramp(&cfg, &connections, &live).await,
        };

        let sustained = sustained_rate(&steps, requested, &cfg.mode);
        let mut report = self.finish(
            &cfg,
            requested,
            &connections,
            steps,
            sustained,
            ended,
            started,
        );
        report.messages = sent;
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
    async fn run_at(
        &self,
        cfg: &LoadTestConfig,
        connections: &[Connection],
        live: &[usize],
        rate: u32,
        budget: usize,
        deadline: Option<Instant>,
    ) -> usize {
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

        while sent < budget {
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            if deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                break;
            }

            let per_tick = rate as f64 * TICK.as_secs_f64();
            let (to_send, wait) = if per_tick >= 1.0 {
                (per_tick.round() as usize, TICK)
            } else {
                (1, Duration::from_secs_f64(1.0 / rate.max(1) as f64))
            };

            let tick_started = Instant::now();
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

            let spent = tick_started.elapsed();
            if spent < wait {
                tokio::time::sleep(wait - spent).await;
            }
            self.emit_progress(connections, budget, rate);
        }

        for handle in inflight {
            let _ = handle.await;
        }
        self.emit_progress(connections, budget, rate);
        sent
    }

    async fn run_ramp(
        &self,
        cfg: &LoadTestConfig,
        connections: &[Connection],
        live: &[usize],
    ) -> (Vec<RampStep>, LoadEndReason, usize) {
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
        let mut before = tally_totals(connections);

        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return (steps, LoadEndReason::Stopped, total_sent);
            }
            if total_sent >= cfg.messages {
                return (steps, LoadEndReason::Complete, total_sent);
            }
            if live.iter().all(|i| {
                connections[*i]
                    .session
                    .as_ref()
                    .map(|s| s.state() != BindState::Bound)
                    .unwrap_or(true)
            }) {
                return (steps, LoadEndReason::ConnectionsLost, total_sent);
            }

            let budget = (cfg.messages - total_sent)
                .min(((rate as f64) * step_duration.as_secs_f64()).ceil() as usize)
                .max(1);
            let started = Instant::now();
            let sent = self
                .run_at(
                    cfg,
                    connections,
                    live,
                    rate,
                    budget,
                    Some(started + step_duration),
                )
                .await;
            total_sent += sent;

            let after = tally_totals(connections);
            let measured = measure_step(&before, &after, rate, started.elapsed());
            before = after;

            let breached = measured.error_share > abandon_on.error_share
                || measured.latency_p95 > abandon_on.latency_p95_ms;

            let reason = if !breached {
                None
            } else if measured.error_share > abandon_on.error_share {
                Some(format!(
                    "{:.0}% of submits came back with an error, above the {:.0}% allowed",
                    measured.error_share * 100.0,
                    abandon_on.error_share * 100.0
                ))
            } else {
                Some(format!(
                    "p95 latency reached {}ms, above the {}ms allowed",
                    measured.latency_p95, abandon_on.latency_p95_ms
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
                return (steps, LoadEndReason::Breached, total_sent);
            }
            if rate >= max_rate {
                return (steps, LoadEndReason::ReachedMaxRate, total_sent);
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
    ) -> LoadReport {
        let elapsed = started.elapsed();
        let totals = tally_totals(connections);

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

        let seconds = elapsed.as_secs_f64();
        let report = LoadReport {
            connections: cfg.connections,
            requested_rate_per_sec: requested,
            achieved_rate_per_sec: if seconds > 0.0 {
                totals.submitted as f64 / seconds
            } else {
                0.0
            },
            sustained_rate_per_sec: sustained,
            messages: 0,
            pdus: totals.submitted,
            submitted: totals.submitted,
            accepted: totals.accepted,
            errors_by_status: totals.errors,
            latency_p50: percentile(&totals.latencies, 50),
            latency_p95: percentile(&totals.latencies, 95),
            latency_p99: percentile(&totals.latencies, 99),
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

#[derive(Default, Clone)]
struct Totals {
    submitted: usize,
    accepted: usize,
    errors: BTreeMap<String, usize>,
    latencies: Vec<u64>,
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
        totals.latencies.extend(latencies);
    }
    totals.latencies.sort_unstable();
    totals
}

fn measure_step(before: &Totals, after: &Totals, rate: u32, elapsed: Duration) -> RampStep {
    let submitted = after.submitted.saturating_sub(before.submitted);
    let accepted = after.accepted.saturating_sub(before.accepted);
    let errors = submitted.saturating_sub(accepted);
    let seconds = elapsed.as_secs_f64();

    // The step's own latencies are the tail of the combined list.
    let fresh: Vec<u64> =
        after.latencies[before.latencies.len().min(after.latencies.len())..].to_vec();
    let mut sorted = fresh;
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

/// The highest rate the SMSC carried without breaching a limit. A ramp that
/// breaks on its first step sustained nothing.
fn sustained_rate(steps: &[RampStep], requested: u32, mode: &LoadMode) -> u32 {
    match mode {
        LoadMode::Fixed { rate_per_sec } => *rate_per_sec,
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
