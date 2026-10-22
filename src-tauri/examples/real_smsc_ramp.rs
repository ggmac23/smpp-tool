//! A ramp against a real SMSC, to find where it gives way (not product code).
//!
//!   cargo run --release --example real_smsc_ramp -- \
//!     <host> <port> <system_id> <password> \
//!     [connections] [start/s] [step/s] [max/s] [step_secs] [messages] [window]
//!
//! Defaults climb gently. Raise them deliberately: this sends real traffic to
//! somebody's server.

use std::time::Duration;

use smppclient::app::loadtest::{AbandonOn, LoadMode, LoadRunner, LoadTestConfig};
use smppclient::app::session::{BindMode, BindOptions};
use smppclient::smpp::constants::{command_id, DataCoding, InterfaceVersion};
use smppclient::smpp::encode::PduInput;
use smppclient::smpp::segment::BodyPlacement;
use smppclient::smpp::text::{Gsm7Packing, SegmentationMode};

fn arg<T: std::str::FromStr>(args: &[String], i: usize, default: T) -> T {
    args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = arg(&args, 2, 2775);
    let system_id = args.get(3).cloned().unwrap_or_default();
    let password = args.get(4).cloned().unwrap_or_default();

    let connections: usize = arg(&args, 5, 4);
    let start_rate: u32 = arg(&args, 6, 25);
    let step: u32 = arg(&args, 7, 25);
    let max_rate: u32 = arg(&args, 8, 200);
    let step_secs: u64 = arg(&args, 9, 2);
    let messages: usize = arg(&args, 10, 900);
    let window: usize = arg(&args, 11, 50);

    println!(
        "ramping {start_rate} -> {max_rate}/sec in steps of {step}, {step_secs}s each,\n\
         over {connections} connection(s) with a window of {window}, \
         up to {messages} messages"
    );
    println!("giving way at 2% errors or a p95 above 2500ms\n");

    let report = LoadRunner::new()
        .start(LoadTestConfig {
            host,
            port,
            tls: Default::default(),
            bind: BindOptions {
                system_id,
                password,
                system_type: String::new(),
                interface_version: InterfaceVersion::V34,
                mode: BindMode::Transceiver,
                addr_ton: 0,
                addr_npi: 0,
                address_range: String::new(),
            },
            connections,
            messages,
            mode: LoadMode::Ramp {
                start_rate,
                step,
                step_duration: Duration::from_secs(step_secs),
                max_rate,
                abandon_on: AbandonOn {
                    error_share: 0.02,
                    latency_p95_ms: 2500,
                },
            },
            window_size: window,
            response_timeout: Duration::from_secs(15),
            connect_timeout: Duration::from_secs(10),
            template: PduInput::new(command_id("submit_sm").unwrap(), 0)
                .with_num("source_addr_ton", 5)
                .with_str("source_addr", "SMPPTEST"),
            destination: "447700900123".into(),
            text: "load".into(),
            data_coding: DataCoding::GSM_DEFAULT,
            segmentation: SegmentationMode::None,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
        })
        .await
        .expect("the load test");

    println!("requested   achieved  accepted   errors      p95  verdict");
    for s in &report.steps {
        println!(
            "{:<10} {:>8.1} {:>9} {:>7.1}% {:>7}ms  {}",
            format!("{}/s", s.requested_rate),
            s.achieved_rate,
            s.accepted,
            s.error_share * 100.0,
            s.latency_p95,
            s.reason.clone().unwrap_or_else(|| "held".into())
        );
    }

    println!("\nended: {:?}", report.ended);
    println!("sustained: {}/sec", report.sustained_rate_per_sec);
    println!(
        "{} submitted, {} accepted, p50 {}ms p95 {}ms p99 {}ms",
        report.submitted,
        report.accepted,
        report.latency_p50,
        report.latency_p95,
        report.latency_p99
    );
    for (status, count) in &report.errors_by_status {
        println!("  {status}: {count}");
    }
    for c in &report.per_connection {
        println!(
            "  connection {}: {} submitted, {} accepted, p95 {}ms{}",
            c.index,
            c.submitted,
            c.accepted,
            c.latency_p95,
            if c.lost { " (lost)" } else { "" }
        );
    }
}
