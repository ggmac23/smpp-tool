//! Verification against a real SMSC (not product code, not part of the suite).
//!
//! Run by hand with the host and credentials as arguments. It is deliberately
//! small: bind, ask once, send one message, read what comes back, unbind. A
//! burst or a load test against somebody else's server is a decision for the
//! person who owns it, not something a verification run should take on its
//! own.
//!
//!   cargo run --example against_real_smsc -- <host> <port> <system_id> <password>

use std::time::Duration;

use smppclient::app::session::{BindMode, BindOptions, Session, SessionEvent, SessionOptions};
use smppclient::app::transport::{Transport, TransportOptions};
use smppclient::smpp::constants::{command_id, command_status_name, InterfaceVersion};
use smppclient::smpp::encode::PduInput;
use smppclient::smpp::summary::{describe, summarise};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(2775);
    let system_id = args.get(3).cloned().unwrap_or_default();
    let password = args.get(4).cloned().unwrap_or_default();

    println!("connecting to {host}:{port}");
    let (transport, events) = match Transport::connect(TransportOptions {
        host: host.clone(),
        port,
        tls: Default::default(),
        connect_timeout: Duration::from_secs(8),
    })
    .await
    {
        Ok(pair) => pair,
        Err(e) => {
            println!("FAILED to connect: {e}");
            return;
        }
    };
    println!("connected");

    let (session, mut session_events) = Session::new(
        transport,
        events,
        SessionOptions {
            window_size: 10,
            response_timeout: Duration::from_secs(10),
        },
    );

    // Everything the session sees, as the trace would show it.
    tokio::spawn(async move {
        while let Some(event) = session_events.recv().await {
            match event {
                SessionEvent::PduIn { pdu, .. } => {
                    println!(
                        "  <- {} [{}]",
                        summarise(&pdu),
                        command_status_name(pdu.command_status)
                    )
                }
                SessionEvent::PduOut { pdu, .. } => println!("  -> {}", summarise(&pdu)),
                SessionEvent::DecodeError { message, raw } => {
                    println!("  <! decode error: {message} ({} bytes)", raw.len())
                }
                SessionEvent::State { state, mode } => println!("  == {state:?} {mode:?}"),
                SessionEvent::Timeout { sequence_number } => {
                    println!("  !! no answer to sequence {sequence_number}")
                }
                SessionEvent::Orphan(pdu) => println!("  <? unmatched {}", summarise(&pdu)),
                SessionEvent::RawOut { bytes, .. } => println!("  -> {} raw bytes", bytes.len()),
            }
        }
    });

    match session
        .bind(BindOptions {
            system_id: system_id.clone(),
            password,
            system_type: String::new(),
            interface_version: InterfaceVersion::V34,
            mode: BindMode::Transceiver,
            addr_ton: 0,
            addr_npi: 0,
            address_range: String::new(),
        })
        .await
    {
        Ok(resp) => {
            println!(
                "BIND ok, status {}",
                command_status_name(resp.command_status)
            );
            for field in describe(&resp) {
                println!("  {} = {}", field.name, field.value);
            }
        }
        Err(e) => {
            println!("BIND FAILED: {e}");
            return;
        }
    }

    match session
        .send(PduInput::new(command_id("enquire_link").unwrap(), 0))
        .await
    {
        Ok(resp) => println!(
            "ENQUIRE_LINK -> {}",
            command_status_name(resp.command_status)
        ),
        Err(e) => println!("ENQUIRE_LINK failed: {e}"),
    }

    let submit = PduInput::new(command_id("submit_sm").unwrap(), 0)
        .with_str("service_type", "")
        .with_num("source_addr_ton", 5)
        .with_num("source_addr_npi", 0)
        .with_str("source_addr", "SMPPTEST")
        .with_num("dest_addr_ton", 1)
        .with_num("dest_addr_npi", 1)
        .with_str("destination_addr", "447700900123")
        .with_num("registered_delivery", 1)
        .with_num("data_coding", 0)
        .with_bytes("short_message", b"hello from the rust client".to_vec());

    match session.send(submit).await {
        Ok(resp) => {
            println!("SUBMIT_SM -> {}", command_status_name(resp.command_status));
            for field in describe(&resp) {
                println!("  {} = {}", field.name, field.value);
            }
        }
        Err(e) => println!("SUBMIT_SM failed: {e}"),
    }

    // Give a delivery receipt a moment to arrive before we go.
    tokio::time::sleep(Duration::from_secs(3)).await;

    match session.unbind().await {
        Ok(resp) => println!("UNBIND -> {}", command_status_name(resp.command_status)),
        Err(e) => println!("UNBIND failed: {e}"),
    }
    session.disconnect();
    tokio::time::sleep(Duration::from_millis(200)).await;
    println!("done");
}
