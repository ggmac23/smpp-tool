//! Verification harness (not product code).
//!
//! Starts the real Tauri application and asks the renderer what happened,
//! through the same command bridge the product will use.

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use tauri::{Manager, State, WebviewWindow};

struct Report(Mutex<Sender<String>>);

#[tauri::command]
fn probe_report(text: String, report: State<'_, Report>) {
    let _ = report.0.lock().unwrap().send(text);
}

const PROBE: &str = r#"
  (function () {
    var out = {};
    try {
      var app = (typeof qx === 'object' && qx.core && qx.core.Init)
        ? qx.core.Init.getApplication() : null;
      var shell = app && app.getShell ? app.getShell() : null;
      out.started = !!app;
      out.tabs = shell ? shell.getTabs().getChildren().map(function (p) { return p.getLabel(); }) : [];
      out.widgets = document.querySelectorAll('div').length;
      out.bodyText = (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 40);
      out.errors = (window.__probeErrors || []).slice(0, 6); // populated only when the boot diagnostic is installed
    } catch (e) {
      out.thrown = String(e);
    }
    window.__TAURI__.core.invoke('probe_report', { text: JSON.stringify(out) });
  })();
"#;

fn main() {
    let (tx, rx) = channel::<String>();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Report(Mutex::new(tx)))
        .invoke_handler(tauri::generate_handler![probe_report])
        .setup(move |app| {
            let window: WebviewWindow = app.get_webview_window("main").expect("no main window");
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for attempt in 0..8 {
                    std::thread::sleep(std::time::Duration::from_millis(900));
                    let _ = window.eval(PROBE);
                    if let Ok(text) = rx.recv_timeout(std::time::Duration::from_millis(1500)) {
                        if attempt >= 7 {
                            println!("TAURICHECK:{text}");
                            handle.exit(0);
                            return;
                        }
                        println!("TAURIPARTIAL:{attempt}:{text}");
                    }
                }
                println!("TAURICHECK:{{\"error\":\"the renderer never reported\"}}");
                handle.exit(1);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start");
}
