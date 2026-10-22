//! Verification harness (not product code).
//!
//! Runs the real application against a fake SMSC, drives the load-test panel
//! the way a person would, and reports what the report pane ended up saying.
//! Compiling is not working: this is the check that the whole chain — widget,
//! command, engine, sockets, events, report — actually joins up.

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

use smppclient::testing::fake_smsc::FakeSmsc;
use tauri::{Manager, State, WebviewWindow};

struct Report(Mutex<Sender<String>>);

#[tauri::command]
fn probe_report(text: String, report: State<'_, Report>) {
    let _ = report.0.lock().unwrap().send(text);
}

/// Runs two load tests the way a person would: a fixed-rate one, then a ramp
/// against an SMSC that throttles. Everything it reads is what is on screen.
fn script(port: u16) -> String {
    format!(
        r#"
  (function () {{
    function report(o) {{
      window.__TAURI__.core.invoke('probe_report', {{ text: JSON.stringify(o) }});
    }}
    try {{
      var app = qx.core.Init.getApplication();
      var shell = app.getShell();
      var load = app.getPanels().load;
      var phase = window.__probePhase || 0;

      if (phase === 0) {{
        window.__probePhase = 1;
        shell.getConnectionPanel().setFormValues({{
          host: "127.0.0.1",
          port: {port},
          tlsEnabled: false,
          systemId: "esme",
          password: "pw",
          systemType: "SMPP"
        }});
        load.setFormValues({{
          connections: 2,
          messages: 20,
          ratePerSec: 50,
          destination: "393331234567"
        }});
        window.__probeStartEnabled = load.isStartEnabled();
        window.__probeLimits = load.getLimitsText();
        load.start();
        return;
      }}

      var text = load.getReportText() || "";
      if (text.indexOf("Running") === 0 || text === "Nothing run yet.") {{
        return;
      }}

      if (phase === 1) {{
        window.__probeFixed = {{
          report: text,
          rows: load.getConnectionRows(),
          steps: load.getStepText()
        }};
        window.__probePhase = 2;
        // Now the ramp. The SMSC throttles one submit in three, so the first
        // step is already past the 2% the form calls giving way.
        load.setFormValues({{
          modeLabel: "Ramp until it breaks",
          messages: 600,
          startRate: 60,
          step: 60,
          maxRate: 240,
          stepSeconds: 1,
          errorSharePercent: 2,
          latencyP95Ms: 2000
        }});
        window.__probeRampEnabled = load.isStartEnabled();
        load.start();
        return;
      }}

      report({{
        tabs: shell.getTabs().getChildren().map(function (p) {{ return p.getLabel(); }}),
        startEnabled: window.__probeStartEnabled,
        limits: window.__probeLimits,
        rampEnabled: window.__probeRampEnabled,
        fixed: window.__probeFixed,
        ramp: {{ report: text, steps: load.getStepText(), rows: load.getConnectionRows() }}
      }});
    }} catch (e) {{
      report({{ thrown: String(e) }});
    }}
  }})();
"#
    )
}

fn main() {
    let (tx, rx) = channel::<String>();

    let runtime = tokio::runtime::Runtime::new().expect("a runtime for the fake SMSC");
    let smsc = FakeSmsc::new();
    // One submit in three is throttled, so a ramp has something to break on.
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    smsc.on("submit_sm", move |pdu| {
        use std::sync::atomic::Ordering;
        let n = seen.fetch_add(1, Ordering::SeqCst);
        let status = if n % 3 == 2 {
            smppclient::smpp::constants::command_status("ESME_RTHROTTLED").unwrap()
        } else {
            0
        };
        Some(vec![smppclient::smpp::encode::PduInput::new(
            smppclient::smpp::constants::command_id("submit_sm_resp").unwrap(),
            pdu.sequence_number,
        )
        .with_status(status)
        .with_str("message_id", &format!("id{n}"))])
    });
    let port = runtime.block_on(smsc.listen());
    // Keep the listener alive for the life of the process.
    std::mem::forget(smsc);
    std::mem::forget(runtime);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Report(Mutex::new(tx)))
        .setup(move |app| {
            smppclient::commands::install(&app.handle().clone()).expect("install the commands");
            let window: WebviewWindow = app.get_webview_window("main").expect("no main window");
            std::thread::spawn(move || {
                for _ in 0..60 {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    let _ = window.eval(script(port));
                    if let Ok(text) = rx.recv_timeout(std::time::Duration::from_millis(600)) {
                        println!("VERIFY:{text}");
                        std::process::exit(0);
                    }
                }
                println!("VERIFY:{{\"error\":\"the report pane never settled\"}}");
                std::process::exit(1);
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
