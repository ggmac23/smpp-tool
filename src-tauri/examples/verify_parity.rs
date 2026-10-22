//! Verification harness (not product code).
//!
//! Drives the real application against a fake SMSC and checks the same list
//! the Electron build was held to. Each phase is a thing a person does; the
//! renderer reports what it sees, and this file checks what arrived at the
//! socket. Compiling proves nothing about either.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use smppclient::smpp::constants::{command_id, command_status, ESME_ROK};
use smppclient::smpp::encode::PduInput;
use smppclient::testing::fake_smsc::FakeSmsc;
use tauri::{Manager, State, WebviewWindow};

struct Probe {
    tx: Mutex<Sender<String>>,
}

#[tauri::command]
fn probe_report(text: String, probe: State<'_, Probe>) {
    let _ = probe.tx.lock().unwrap().send(text);
}

/// What the renderer is asked to do, one step at a time. Each returns a JSON
/// object; `done` false means "not ready yet, ask again".
fn step(phase: usize, port: u16) -> String {
    let body = match phase {
        0 => format!(
            r#"
      shell.getConnectionPanel().setFormValues({{
        host: "127.0.0.1", port: {port}, tlsEnabled: false,
        systemId: "esme", password: "pw", systemType: "SMPP"
      }});
      if (!window.__p0) {{ window.__p0 = true; shell.getConnectionPanel().bind(); }}
      var label = shell.getConnectionPanel().getStateLabel();
      if (label.indexOf("Bound") !== 0) {{
        // If the trace is empty too, every subscription made at boot is
        // dead, and the problem is not this panel.
        return {{
          done: false,
          label: label,
          notice: shell.getNoticeText(),
          traceRows: app.getTracePanel().getRowCount(),
          apiAvailable: smppclient.Api.getInstance().available()
        }};
      }}
      return {{ done: true, state: label, counters: shell.getConnectionPanel().getCountersText() }};
"#
        ),
        1 => r#"
      var submit = app.getSubmitForm();
      if (!window.__p1) {
        window.__p1 = true;
        // The three-part GSM sample, chosen the way a person chooses it.
        var preset = SmppForms.SAMPLE_PRESETS.filter(function (p) { return p.key === "gsm3"; })[0];
        submit.setFormValues({
          text: SmppForms.SAMPLES[preset.key],
          dataCodingLabel: preset.dataCodingLabel,
          segmentationLabel: preset.segmentationLabel,
          destinationAddr: "393331234567"
        });
        window.__p1counter = submit.getCounterText();
        submit.send();
        return { done: false };
      }
      return { done: true, counter: window.__p1counter, trace: app.getTracePanel().getRowCount() };
"#
        .to_string(),
        2 => r#"
      // The receipt was pushed by the SMSC. It must show up decoded.
      var trace = app.getTracePanel();
      for (var i = 0; i < trace.getRowCount(); i++) {
        if ((trace.getRowSummary(i) || "").indexOf("DELIVRD") >= 0) {
          return { done: true, summary: trace.getRowSummary(i) };
        }
      }
      return { done: false, rows: trace.getRowCount() };
"#
        .to_string(),
        3 => r#"
      // A malformed PDU arrived after a good one in the same write. The good
      // one must survive it, and the failure must be visible rather than
      // silent.
      var trace = app.getTracePanel();
      var decodeError = null, enquires = 0;
      for (var i = 0; i < trace.getRowCount(); i++) {
        var s = trace.getRowSummary(i) || "";
        if (s.indexOf("decode error") >= 0) { decodeError = s; }
        if (s.indexOf("enquire_link") >= 0) { enquires++; }
      }
      if (!decodeError || enquires < 2) {
        return { done: false, decodeError: decodeError, enquires: enquires };
      }
      return { done: true, decodeError: decodeError, enquires: enquires };
"#
        .to_string(),
        4 => r#"
      var trace = app.getTracePanel();
      if (!window.__p4) { window.__p4 = true; trace.selectRow(0); return { done: false }; }
      var hex = trace.getHexText() || "", fields = trace.getFieldsText() || "";
      if (hex.length === 0 || fields.length === 0) { return { done: false, hex: hex.length, fields: fields.length }; }
      return { done: true, hexHead: hex.slice(0, 24), fieldsHead: fields.slice(0, 60) };
"#
        .to_string(),
        5 => r#"
      var trace = app.getTracePanel();
      if (!window.__p5) { window.__p5 = true; trace.showFlow(); return { done: false }; }
      var flow = trace.getFlowDiagram();
      if (!flow) { return { done: false }; }
      var svg = flow.getSvg() || "";
      var arrows = (svg.match(/<line/g) || []).length;
      var texts = (svg.match(/<text/g) || []).length;
      if (arrows === 0) { return { done: false, svg: svg.length }; }
      return { done: true, arrows: arrows, texts: texts, rows: trace.getRowCount() };
"#
        .to_string(),
        6 => r#"
      var burst = app.getPanels().burst;
      if (!window.__p6) {
        window.__p6 = true;
        // A one-part body: the three-part sample is still in the form from
        // the earlier phase, and 20 messages of it would be 60 PDUs.
        app.getSubmitForm().setFormValues({
          text: "burst",
          dataCodingLabel: "0 \u2014 GSM 03.38 default",
          segmentationLabel: "None \u2014 one PDU"
        });
        burst.setFormValues({ count: 20, ratePerSec: 100, awaitDlrMs: 2000 });
        burst.start();
        return { done: false };
      }
      var stats = burst.getStatsText() || "";
      // Wait for the run to finish rather than reading it mid-flight.
      if (stats.indexOf("Messages          20") < 0) {
        return { done: false, stats: stats.split("\n")[0] };
      }
      return { done: true, stats: stats, rows: burst.getRowCount() };
"#
        .to_string(),
        7 => r#"
      var hex = app.getPanels().hex;
      if (!window.__p7) {
        window.__p7 = true;
        // command_length 0xdeadbeef, enquire_link, sequence 1. Nonsense on
        // purpose: it must reach the socket exactly as typed.
        hex.setHex("deadbeef 00000015 00000000 00000001");
        return { done: false };
      }
      if (!window.__p7sent) {
        window.__p7sent = true;
        window.__p7summary = hex.getSummaryText();
        window.__p7enabled = hex.isSendEnabled();
        hex.send();
        return { done: false };
      }
      return { done: true, summary: window.__p7summary, sendEnabled: window.__p7enabled };
"#
        .to_string(),
        _ => "return { done: true, finished: true };".to_string(),
    };

    format!(
        r#"
  (function () {{
    function report(o) {{
      o.phase = {phase};
      window.__TAURI__.core.invoke('probe_report', {{ text: JSON.stringify(o) }});
    }}
    // The page may not have booted yet. Silence, not a verdict.
    if (typeof qx === 'undefined' || !qx.core || !qx.core.Init) {{ return; }}
    var app = qx.core.Init.getApplication();
    if (!app || !app.getShell) {{ return; }}

    try {{
      var shell = app.getShell();
      // Always report: a phase that will not finish has to be able to say
      // what it is looking at.
      report((function () {{ {body} }})() || {{ done: false }});
    }} catch (e) {{
      report({{ done: true, thrown: String(e) }});
    }}
  }})();
"#
    )
}

/// Waits for the renderer to report the phase done, polling the script.
fn run_phase(window: &WebviewWindow, rx: &Receiver<String>, phase: usize, port: u16) -> String {
    let mut last = String::from("nothing was reported at all");
    for _ in 0..60 {
        let _ = window.eval(step(phase, port));
        if let Ok(text) = rx.recv_timeout(Duration::from_millis(500)) {
            if text.contains("\"done\":true") {
                return text;
            }
            last = text;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("PHASE {phase}: NEVER FINISHED, last saw {last}");
    std::process::exit(1);
}

fn main() {
    let (tx, rx) = channel::<String>();

    let runtime = tokio::runtime::Runtime::new().expect("a runtime for the fake SMSC");
    let smsc = Arc::new(FakeSmsc::new());

    // Burst phase: one submit in five is throttled, and the rest get a
    // receipt. Before the burst every submit is accepted.
    let submits = Arc::new(AtomicUsize::new(0));
    let bursting = Arc::new(AtomicUsize::new(0));
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let submits = submits.clone();
        let bursting = bursting.clone();
        let ids = ids.clone();
        smsc.on("submit_sm", move |pdu| {
            let n = submits.fetch_add(1, Ordering::SeqCst);
            let throttling = bursting.load(Ordering::SeqCst) == 1;
            let status = if throttling && n.is_multiple_of(5) {
                command_status("ESME_RTHROTTLED").unwrap()
            } else {
                ESME_ROK
            };
            let id = format!("id{n}");
            if status == ESME_ROK {
                ids.lock().unwrap().push(id.clone());
            }

            let mut replies = vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                pdu.sequence_number,
            )
            .with_status(status)
            .with_str("message_id", &id)];

            // During the burst, every accepted message gets its receipt in
            // the same breath as the response. That is the race the
            // correlator has to survive, so it is worth exercising here.
            if throttling && status == ESME_ROK {
                replies.push(receipt(&id));
            }
            Some(replies)
        });
    }

    let port = runtime.block_on(smsc.listen());

    let smsc_for_app = smsc.clone();
    let submits_counter = submits.clone();
    let ids_for_app = ids.clone();
    let bursting_for_app = bursting.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Probe { tx: Mutex::new(tx) })
        .setup(move |app| {
            smppclient::commands::install(&app.handle().clone()).expect("install the commands");
            let window: WebviewWindow = app.get_webview_window("main").expect("no main window");
            let smsc = smsc_for_app.clone();
            let submits_counter = submits_counter.clone();
            let ids = ids_for_app.clone();
            let bursting = bursting_for_app.clone();

            std::thread::spawn(move || {
                let mut results: Vec<(usize, String)> = Vec::new();

                for phase in 0..8 {
                    // The SMSC's side of each phase, done before the renderer
                    // is asked to look.
                    match phase {
                        2 => {
                            let id = ids.lock().unwrap().last().cloned().unwrap_or_default();
                            smsc.send(receipt(&id));
                        }
                        3 => {
                            // Two good PDUs with a malformed one between them,
                            // in a single write: the good ones must survive.
                            let mut bytes = Vec::new();
                            bytes.extend_from_slice(&enquire_bytes(0x1001));
                            bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x08, 0xff, 0xff, 0xff, 0xff]);
                            bytes.extend_from_slice(&enquire_bytes(0x1002));
                            smsc.send_raw(&bytes);
                        }
                        6 => {
                            // The throttle counts from the start of the burst,
                            // so one in five of the burst is throttled rather
                            // than one in five of everything sent so far.
                            submits_counter.store(0, Ordering::SeqCst);
                            bursting.store(1, Ordering::SeqCst);
                        }
                        _ => {}
                    }

                    let text = run_phase(&window, &rx, phase, port);

                    if phase == 1 {
                        let parts: Vec<_> = smsc
                            .received()
                            .into_iter()
                            .filter(|p| p.command_id == command_id("submit_sm").unwrap())
                            .collect();
                        println!("SOCKET: submit_sm parts: {}", parts.len());
                        let mut refs = Vec::new();
                        for part in &parts {
                            let body = part.bytes_field("short_message").unwrap_or_default();
                            let len = part.u32_field("sm_length").unwrap_or(0);
                            let esm = part.u32_field("esm_class").unwrap_or(0);
                            // UDH: 05 00 03 <ref> <total> <seq>
                            if body.len() >= 6 && body[0] == 0x05 && body[1] == 0x00 {
                                refs.push((body[3], body[4], body[5]));
                            }
                            println!(
                                "SOCKET:   sm_length {len}, esm_class 0x{esm:02x}, body {} bytes",
                                body.len()
                            );
                            assert!(len < 255, "sm_length {len} does not fit one octet");
                        }
                        let shared = refs.iter().all(|r| r.0 == refs[0].0);
                        println!(
                            "SOCKET: concatenation refs {refs:?} shared: {shared}"
                        );
                    }
                    println!("PHASE {phase}: {text}");
                    results.push((phase, text));

                    if phase == 6 {
                        bursting.store(0, Ordering::SeqCst);
                    }
                }

                // The last check belongs to the socket, not the screen.
                let raw = smsc.received_raw();
                let wanted: [u8; 16] = [
                    0xde, 0xad, 0xbe, 0xef, 0x00, 0x00, 0x00, 0x15, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x01,
                ];
                let deadbeef = raw.windows(16).any(|w| w == wanted);
                println!("SOCKET: deadbeef arrived byte for byte: {deadbeef}");
                println!("SOCKET: submits seen: {}", smsc.received().len());
                println!("VERIFY: done");
                std::process::exit(if deadbeef { 0 } else { 1 });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            probe_report,
            smppclient::commands::connect,
            smppclient::commands::disconnect,
            smppclient::commands::bind,
            smppclient::commands::unbind,
            smppclient::commands::send_pdu,
            smppclient::commands::send_raw,
            smppclient::commands::compose_submit,
            smppclient::commands::preview_pdu,
            smppclient::commands::encode_body,
            smppclient::commands::measure,
            smppclient::commands::start_burst,
            smppclient::commands::stop_burst,
            smppclient::commands::burst_rows,
            smppclient::commands::start_load_test,
            smppclient::commands::stop_load_test,
            smppclient::commands::load_report,
            smppclient::commands::trace,
            smppclient::commands::pdu_fields,
            smppclient::commands::pdu_hex,
            smppclient::commands::clear_trace,
            smppclient::commands::set_trace_cap,
            smppclient::commands::export_trace,
            smppclient::commands::export_burst,
            smppclient::commands::list_profiles,
            smppclient::commands::save_profile,
            smppclient::commands::remove_profile,
            smppclient::commands::profile_secrets,
            smppclient::commands::pick_file,
            smppclient::commands::status
        ])
        .run(tauri::generate_context!())
        .expect("the harness failed to start");
}

fn receipt(id: &str) -> PduInput {
    let text = format!(
        "id:{id} sub:001 dlvrd:001 submit date:2610221200 done date:2610221201 stat:DELIVRD err:000 Text:hello"
    );
    PduInput::new(command_id("deliver_sm").unwrap(), 0)
        .with_num("esm_class", 0x04)
        .with_str("source_addr", "393331234567")
        .with_num("sm_length", text.len() as u32)
        .with_bytes("short_message", text.into_bytes())
}

fn enquire_bytes(sequence: u32) -> Vec<u8> {
    smppclient::smpp::encode::encode_pdu(&PduInput::new(
        command_id("enquire_link").unwrap(),
        sequence,
    ))
}
