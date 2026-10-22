//! Verification against a real SMSC (not product code, not part of the suite).
//!
//! Exercises the three things a fake SMSC cannot really vouch for:
//! concatenation as a real SMSC reassembles it, delivery-receipt correlation
//! against ids a real SMSC chooses, and the burst and load engines against a
//! peer with real latency.
//!
//! Volumes are deliberately tiny. Pointing the load test at somebody else's
//! server in earnest is their decision, not a verification run's.
//!
//!   cargo run --example real_smsc_features -- <host> <port> <system_id> <password>

use std::sync::Arc;
use std::time::Duration;

use smppclient::app::burst::{BurstConfig, BurstRunner};
use smppclient::app::dlr::CorrelationConfig;
use smppclient::app::loadtest::{LoadMode, LoadRunner, LoadTestConfig};
use smppclient::app::recipients::RecipientSource;
use smppclient::app::session::{BindMode, BindOptions, Session, SessionEvent, SessionOptions};
use smppclient::app::transport::{Transport, TransportOptions};
use smppclient::smpp::constants::{command_id, command_status_name, DataCoding, InterfaceVersion};
use smppclient::smpp::encode::PduInput;
use smppclient::smpp::segment::{segment, BodyPlacement, SegmentOptions};
use smppclient::smpp::summary::summarise;
use smppclient::smpp::text::{Gsm7Packing, SegmentationMode};

const DESTINATION: &str = "447700900123";

fn bind_opts(system_id: &str, password: &str) -> BindOptions {
    BindOptions {
        system_id: system_id.into(),
        password: password.into(),
        system_type: String::new(),
        interface_version: InterfaceVersion::V34,
        mode: BindMode::Transceiver,
        addr_ton: 0,
        addr_npi: 0,
        address_range: String::new(),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(2775);
    let system_id = args.get(3).cloned().unwrap_or_default();
    let password = args.get(4).cloned().unwrap_or_default();

    let (transport, events) = Transport::connect(TransportOptions {
        host: host.clone(),
        port,
        tls: Default::default(),
        connect_timeout: Duration::from_secs(8),
    })
    .await
    .expect("connect");

    let (session, mut session_events) = Session::new(
        transport,
        events,
        SessionOptions {
            window_size: 10,
            response_timeout: Duration::from_secs(10),
        },
    );
    let session = Arc::new(session);

    let watching = session.clone();
    let runner_for_events: Arc<std::sync::Mutex<Option<Arc<BurstRunner>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let observer = runner_for_events.clone();
    tokio::spawn(async move {
        let _ = &watching;
        while let Some(event) = session_events.recv().await {
            if let SessionEvent::PduIn { pdu, .. } = event {
                if let Some(runner) = observer.lock().unwrap().clone() {
                    runner.observe(&pdu);
                }
                println!("  <- {}", summarise(&pdu));
            }
        }
    });

    session
        .bind(bind_opts(&system_id, &password))
        .await
        .expect("bind");
    println!("bound\n");

    /* ------------------------------------------------------------- */
    println!("== a three-part concatenated message ==");
    let text = "This message is deliberately longer than one hundred and sixty GSM 03.38 \
                septets so that the client has to split it, and it keeps going past three \
                hundred and six so a third part is needed as well. The euro sign \u{20ac} costs two \
                septets, which is how an escape pair comes to sit near a segment boundary \
                instead of safely in the middle of a part. It carries on for a while yet, \
                because three parts is the point and two would not prove the sequence number \
                ever reaches three.";
    let parts = segment(&SegmentOptions {
        text: text.to_string(),
        data_coding: DataCoding::GSM_DEFAULT,
        mode: SegmentationMode::Udh,
        placement: BodyPlacement::ShortMessage,
        packing: Gsm7Packing::Unpacked,
        user_udh: None,
        ref_num: None,
        esm_class: 0,
    });
    println!("composed {} parts", parts.len());

    for (i, part) in parts.iter().enumerate() {
        let mut input = PduInput::new(command_id("submit_sm").unwrap(), 0)
            .with_num("source_addr_ton", 5)
            .with_str("source_addr", "SMPPTEST")
            .with_num("dest_addr_ton", 1)
            .with_num("dest_addr_npi", 1)
            .with_str("destination_addr", DESTINATION)
            .with_num("registered_delivery", 1)
            .with_num("data_coding", DataCoding::GSM_DEFAULT as u32);
        for (name, value) in &part.fields {
            input.fields.insert(name.clone(), value.clone());
        }
        match session.send(input).await {
            Ok(resp) => println!(
                "  part {}/{} -> {} id={}",
                i + 1,
                parts.len(),
                command_status_name(resp.command_status),
                resp.str_field("message_id").unwrap_or_default()
            ),
            Err(e) => println!("  part {} failed: {e}", i + 1),
        }
    }

    /* ------------------------------------------------------------- */
    println!("\n== a burst of 5 at 5/sec, waiting for receipts ==");
    let runner = Arc::new(BurstRunner::new(
        session.clone(),
        CorrelationConfig::default(),
    ));
    *runner_for_events.lock().unwrap() = Some(runner.clone());

    let done = runner
        .start(BurstConfig {
            recipients: RecipientSource::Repeat {
                number: DESTINATION.into(),
                count: 5,
            },
            rate_per_sec: 5,
            template: PduInput::new(command_id("submit_sm").unwrap(), 0)
                .with_num("source_addr_ton", 5)
                .with_str("source_addr", "SMPPTEST")
                .with_num("dest_addr_ton", 1)
                .with_num("dest_addr_npi", 1)
                .with_num("registered_delivery", 1),
            text: "burst from the rust client".into(),
            data_coding: DataCoding::GSM_DEFAULT,
            segmentation: SegmentationMode::None,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
            backoff_on_throttle: true,
            await_dlr: Duration::from_secs(6),
        })
        .await
        .expect("burst");

    println!(
        "  {:?}: {} messages, {} accepted, {} receipts, {} unmatched, {:.1}/s achieved",
        done.reason,
        done.stats.messages,
        done.stats.accepted,
        done.stats.dlr_received,
        done.stats.dlr_unmatched,
        done.stats.achieved_rate_per_sec
    );
    println!(
        "  latency p50 {} ms, p95 {} ms",
        done.stats.latency_p50, done.stats.latency_p95
    );
    for (status, count) in &done.stats.errors_by_status {
        println!("  {status}: {count}");
    }
    println!(
        "  (receipts for the concatenated parts above belong to no burst row, so they are \
         counted as unmatched rather than quietly attributed)"
    );
    *runner_for_events.lock().unwrap() = None;

    let _ = session.unbind().await;
    session.disconnect();

    /* ------------------------------------------------------------- */
    println!("\n== a load test: 2 connections, ramping 5 -> 20/sec ==");
    let report = LoadRunner::new()
        .start(LoadTestConfig {
            host,
            port,
            tls: Default::default(),
            bind: bind_opts(&system_id, &password),
            connections: 2,
            messages: 40,
            // A ramp: it climbs until the SMSC gives way, or until the top.
            mode: LoadMode::Ramp {
                start_rate: 5,
                step: 5,
                step_duration: Duration::from_secs(1),
                max_rate: 20,
                abandon_on: smppclient::app::loadtest::AbandonOn {
                    error_share: 0.02,
                    latency_p95_ms: 3000,
                },
            },
            window_size: 10,
            response_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(8),
            template: PduInput::new(command_id("submit_sm").unwrap(), 0)
                .with_num("source_addr_ton", 5)
                .with_str("source_addr", "SMPPTEST"),
            destination: DESTINATION.into(),
            text: "load from the rust client".into(),
            data_coding: DataCoding::GSM_DEFAULT,
            segmentation: SegmentationMode::None,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
        })
        .await
        .expect("load test");

    println!(
        "  {:?}: {} submitted, {} accepted, sustained {}/s, achieved {:.1}/s",
        report.ended,
        report.submitted,
        report.accepted,
        report.sustained_rate_per_sec,
        report.achieved_rate_per_sec
    );
    println!(
        "  latency p50 {} ms, p95 {} ms, p99 {} ms over {} ms",
        report.latency_p50, report.latency_p95, report.latency_p99, report.duration_ms
    );
    for step in &report.steps {
        println!(
            "  step {}/s: {:.1}/s achieved, {:.1}% errors, p95 {} ms{}",
            step.requested_rate,
            step.achieved_rate,
            step.error_share * 100.0,
            step.latency_p95,
            if step.breached { " -- gave way" } else { "" }
        );
    }
    for c in &report.per_connection {
        println!(
            "  connection {}: bound {}, {} submitted, {} accepted, p95 {} ms",
            c.index, c.bound, c.submitted, c.accepted, c.latency_p95
        );
    }
    println!("done");
}
