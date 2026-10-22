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
      if (label.indexOf("Bound") !== 0) {{ return {{ done: false, label: label }}; }}
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
      // A malformed PDU arrived between two good ones in the same write.
      var trace = app.getTracePanel();
      var decodeError = null, enquires = 0;
      for (var i = 0; i < trace.getRowCount(); i++) {
        var s = trace.getRowSummary(i) || "";
        if (s.indexOf("undecodable") >= 0 || s.indexOf("will not") >= 0) { decodeError = s; }
      }
      for (var j = 0; j < trace.getRowCount(); j++) {
        if ((trace.getRowSummary(j) || "").indexOf("enquire_link") >= 0) { enquires++; }
      }
      if (!decodeError || enquires < 2) { return { done: false, decodeError: decodeError, enquires: enquires }; }
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
        burst.setFormValues({ count: 20, ratePerSec: 100, awaitDlrMs: 1500 });
        window.__p6promise = burst.start();
        return { done: false };
      }
      var stats = burst.getStatsText ? burst.getStatsText() : "";
      if (!stats || stats.indexOf("Accepted") < 0) { return { done: false, stats: stats.slice(0, 40) }; }
      return { done: true, stats: stats, rows: burst.getRowCount ? burst.getRowCount() : null };
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
    try {{
      var app = qx.core.Init.getApplication();
      var shell = app.getShell();
      var out = (function () {{ {body} }})();
      if (out && out.done) {{ report(out); }}
    }} catch (e) {{
      report({{ done: true, thrown: String(e) }});
    }}
  }})();
"#
    )
}

/// Waits for the renderer to report the phase done, polling the script.
fn run_phase(window: &WebviewWindow, rx: &Receiver<String>, phase: usize, port: u16) -> String {
    for _ in 0..40 {
        let _ = window.eval(step(phase, port));
        if let Ok(text) = rx.recv_timeout(Duration::from_millis(500)) {
            return text;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("phase {phase} never completed");
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
            Some(vec![PduInput::new(
                command_id("submit_sm_resp").unwrap(),
                pdu.sequence_number,
            )
            .with_status(status)
            .with_str("message_id", &id)])
        });
    }

    let port = runtime.block_on(smsc.listen());

    let smsc_for_app = smsc.clone();
    let ids_for_app = ids.clone();
    let bursting_for_app = bursting.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Probe { tx: Mutex::new(tx) })
        .setup(move |app| {
            smppclient::commands::install(&app.handle().clone()).expect("install the commands");
            let window: WebviewWindow = app.get_webview_window("main").expect("no main window");
            let smsc = smsc_for_app.clone();
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
                            bursting.store(1, Ordering::SeqCst);
                        }
                        _ => {}
                    }

                    let text = run_phase(&window, &rx, phase, port);
                    println!("PHASE {phase}: {text}");
                    results.push((phase, text));

                    if phase == 6 {
                        bursting.store(0, Ordering::SeqCst);
                    }
                }

                // The last check belongs to the socket, not the screen.
                let raw = smsc.received_raw();
                let deadbeef = raw.iter().any(|bytes| {
                    bytes.len() >= 16 && bytes[..4] == [0xde, 0xad, 0xbe, 0xef]
                });
                println!("SOCKET: deadbeef arrived byte for byte: {deadbeef}");
                println!("SOCKET: submits seen: {}", smsc.received().len());
                println!("VERIFY: done");
                std::process::exit(if deadbeef { 0 } else { 1 });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![probe_report])
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
