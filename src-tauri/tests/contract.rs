//! What the backend actually sends, written down where the renderer's tests
//! can read it.
//!
//! The three worst defects in the bridge were all the same shape: a widget
//! read or wrote a field the backend does not have. `result.path` off a bare
//! string, `status.pending` where the report says `pendingBuffered`, three
//! TLS paths placed beside `tls` instead of inside it. Each compiled
//! cleanly, passed every test, and failed at the button.
//!
//! This dumps one sample of every type that crosses, and fails if the dump
//! drifts from the committed one. The renderer's own suite then checks its
//! declarations against the same file, so a rename on either side has to be
//! a rename on both.

use std::collections::BTreeMap;

use smppclient::app::burst::{BurstDone, BurstDoneReason, BurstRow, BurstStats};
use smppclient::app::controller::{EncodedBody, ProfileSecrets, SendOutcome, WrittenFile};
use smppclient::app::loadtest::{
    ConnectionStats, LoadEndReason, LoadProgress, LoadReport, RampStep,
};
use smppclient::app::profiles::{Profile, ProfileBind, ProfileTls};
use smppclient::app::session::{BindMode, BindState};
use smppclient::app::trace::Counter;
use smppclient::app::wire::{to_wire_burst_row, StatusReport, WirePdu};
use smppclient::smpp::summary::DescribedField;
use smppclient::smpp::text::Measurement;

const FIXTURE: &str = "../tests/fixtures/wire-contract.json";

fn sample() -> serde_json::Value {
    let row = BurstRow {
        id: "r0".into(),
        recipient: "393331234567".into(),
        body: "hello".into(),
        pdus: 1,
        message_id: Some("AB12".into()),
        command_status: Some(0),
        submit_latency_ms: Some(14),
        dlr_state: Some("DELIVRD".into()),
        dlr_latency_ms: Some(220),
        ..Default::default()
    };

    let stats = BurstStats {
        messages: 20,
        pdus: 20,
        submitted: 20,
        accepted: 16,
        errors_by_status: BTreeMap::from([("ESME_RTHROTTLED".to_string(), 4)]),
        achieved_rate_per_sec: 44.4,
        latency_p50: 1,
        latency_p95: 2,
        latency_p99: 4,
        dlr_received: 16,
        dlr_unmatched: 0,
    };

    let connection = ConnectionStats {
        index: 0,
        bound: true,
        bind_error: None,
        submitted: 10,
        accepted: 10,
        errors_by_status: BTreeMap::new(),
        latency_p50: 1,
        latency_p95: 2,
        latency_p99: 3,
        lost: false,
    };

    let step = RampStep {
        requested_rate: 60,
        achieved_rate: 45.6,
        submitted: 46,
        accepted: 30,
        error_share: 0.35,
        latency_p95: 1,
        breached: true,
        reason: Some("error share above the limit".into()),
    };

    let report = LoadReport {
        connections: 2,
        requested_rate_per_sec: 240,
        achieved_rate_per_sec: 45.6,
        sustained_rate_per_sec: 0,
        messages: 46,
        pdus: 46,
        submitted: 46,
        accepted: 30,
        errors_by_status: BTreeMap::from([("ESME_RTHROTTLED".to_string(), 16)]),
        latency_p50: 1,
        latency_p95: 2,
        latency_p99: 3,
        duration_ms: 1010,
        per_connection: vec![connection.clone()],
        steps: vec![step.clone()],
        ended: LoadEndReason::Breached,
    };

    let profile = Profile {
        id: "p1".into(),
        name: "Staging".into(),
        host: "smsc.example.com".into(),
        port: 2775,
        bind: ProfileBind {
            system_id: "esme".into(),
            system_type: "SMPP".into(),
            interface_version: 0x34,
            mode: BindMode::Transceiver,
            addr_ton: 0,
            addr_npi: 0,
            address_range: "^44".into(),
        },
        tls: ProfileTls {
            enabled: true,
            reject_unauthorized: true,
            servername: Some("smsc.example.com".into()),
            ca_path: Some("/etc/ssl/ca.pem".into()),
            client_cert_path: Some("/etc/ssl/client.crt".into()),
            client_key_path: Some("/etc/ssl/client.key".into()),
        },
        correlation: Default::default(),
    };

    serde_json::json!({
        "WirePdu": WirePdu {
            seq: 1,
            at: 1_700_000_000_000,
            direction: "out".into(),
            command_name: "submit_sm".into(),
            command_status: 0,
            status_name: "ESME_ROK".into(),
            sequence_number: 2,
            summary: "submit_sm -> 4477".into(),
            hex: "00000021".into(),
            decode_error: None,
        },
        "BurstRow": to_wire_burst_row(&row),
        "BurstStats": stats,
        "BurstDone": BurstDone {
            reason: BurstDoneReason::Complete,
            stats: BurstStats::default(),
            warnings: vec!["a warning".into()],
        },
        "StatusReport": StatusReport {
            state: BindState::Bound,
            mode: Some(BindMode::Transceiver),
            connected: true,
            dropped: 0,
            pending_buffered: 20,
            pending_declared: Some(64),
            inflight: 3,
        },
        "Measurement": Measurement {
            chars: 5,
            bits: 35,
            bytes: 5,
            parts: 1,
            unmappable: Vec::new(),
        },
        "SendOutcome": SendOutcome { sequence_number: 2, command_status: 0 },
        "ProfileSecrets": ProfileSecrets { password: None, key_passphrase: None },
        "WrittenFile": WrittenFile { path: "/tmp/trace.csv".into() },
        "EncodedBody": EncodedBody { hex: "6869".into(), unmappable: Vec::new() },
        "Profile": profile,
        "LoadReport": report,
        "ConnectionStats": connection,
        "RampStep": step,
        "LoadProgress": LoadProgress {
            submitted: 10,
            total: 20,
            current_rate_per_sec: 60,
            achieved_rate_per_sec: 0.0,
            errors: 1,
        },
        "Counter": Counter { sent: 1, received: 1 },
        "DescribedField": DescribedField {
            name: "command_length".into(),
            value: "33".into(),
        },
    })
}

#[test]
fn the_dump_the_renderer_checks_against_is_current() {
    let current = serde_json::to_string_pretty(&sample()).expect("serialise") + "\n";
    // Compared without the carriage returns a Windows checkout may add: the
    // test is about the shapes, and a newline is not one of them.
    let committed = std::fs::read_to_string(FIXTURE)
        .unwrap_or_default()
        .replace("\r\n", "\n");

    if committed != current {
        std::fs::write(FIXTURE, &current).expect("write the fixture");
        panic!(
            "the shapes crossing to the renderer changed, and {FIXTURE} has been rewritten.\n\
             Check the renderer reads the new names, then commit it."
        );
    }
}

#[test]
fn no_shape_carries_a_secret() {
    let dump = serde_json::to_string(&sample()).unwrap();
    for secret in ["password\":\"", "passphrase\":\"", "BEGIN "] {
        assert!(
            !dump.contains(secret),
            "a sample carries something secret-shaped: {secret}"
        );
    }
}
