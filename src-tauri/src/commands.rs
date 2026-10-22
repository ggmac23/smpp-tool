//! The Tauri command surface: one command per thing the renderer can ask for.
//!
//! Each one is a thin shim. It takes the renderer's plain data, hands it to
//! the controller, and reports a failure as an `Err` plus an `error` event —
//! a command never panics across the boundary, and the UI can log what went
//! wrong even when nobody awaited the call.
//!
//! The file dialogs live here rather than in the controller. They need a
//! window, so keeping them out of the controller is what lets the controller
//! be tested without one; the controller takes a path and nothing else.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::app::burst::BurstDone;
use crate::app::compose::{
    compose_submit as compose, preview_pdu as preview, ComposedSubmit, SubmitFormValues,
};
use crate::app::controller::{BindAccepted, Controller, EncodedBody, ProfileSecrets, SendOutcome};
use crate::app::events::{Emitter, EventHub, DEFAULT_BATCH};
use crate::app::loadtest::LoadReport;
use crate::app::profiles::{KeyringSecrets, Profile, ProfileStore};
use crate::app::trace::Counter;
use crate::app::wire::{
    BindRequest, ConnectRequest, ExportFormat, MeasureRequest, PduPreview, SendPduRequest,
    StartBurstRequest, StartLoadRequest, StatusReport, WireBurstRow, WirePdu,
};
use crate::smpp::summary::DescribedField;
use crate::smpp::text::{Gsm7Packing, Measurement};

/// Every command the renderer may call. Kept beside the handler list, and a
/// test asserts the two agree: a command missing from the handler list is a
/// button that does nothing.
pub const COMMAND_NAMES: &[&str] = &[
    "connect",
    "disconnect",
    "bind",
    "unbind",
    "send_pdu",
    "send_raw",
    "compose_submit",
    "preview_pdu",
    "measure",
    "encode_body",
    "start_burst",
    "stop_burst",
    "burst_rows",
    "start_load_test",
    "stop_load_test",
    "load_report",
    "trace",
    "pdu_fields",
    "pdu_hex",
    "clear_trace",
    "set_trace_cap",
    "export_trace",
    "export_burst",
    "list_profiles",
    "save_profile",
    "remove_profile",
    "profile_secrets",
    "pick_file",
    "status",
];

type Ctl<'a> = State<'a, Arc<Controller>>;

/* -------------------------------------------------------------------- */
/* connection                                                           */

#[tauri::command]
pub async fn connect(ctl: Ctl<'_>, req: ConnectRequest) -> Result<(), String> {
    ctl.report("connect", ctl.connect(req).await)
}

#[tauri::command]
pub async fn disconnect(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.disconnect();
    Ok(())
}

#[tauri::command]
pub async fn bind(ctl: Ctl<'_>, req: BindRequest) -> Result<BindAccepted, String> {
    ctl.report("bind", ctl.bind(req).await)
}

#[tauri::command]
pub async fn unbind(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.report("unbind", ctl.unbind().await)
}

#[tauri::command]
pub async fn status(ctl: Ctl<'_>) -> Result<StatusReport, String> {
    Ok(ctl.status())
}

/* -------------------------------------------------------------------- */
/* sending                                                              */

#[tauri::command]
pub async fn send_pdu(ctl: Ctl<'_>, req: SendPduRequest) -> Result<SendOutcome, String> {
    ctl.report("send_pdu", ctl.send_pdu(req).await)
}

#[tauri::command]
pub async fn send_raw(ctl: Ctl<'_>, hex: String) -> Result<(), String> {
    ctl.report("send_raw", ctl.send_raw(&hex).await)
}

/// The submit form, composed into the PDUs it means. Pure: it touches no
/// socket, so it is safe to call on every keystroke for the preview.
#[tauri::command]
pub async fn compose_submit(values: SubmitFormValues) -> Result<ComposedSubmit, String> {
    Ok(compose(&values))
}

#[tauri::command]
pub async fn preview_pdu(hex: String) -> Result<PduPreview, String> {
    Ok(preview(&hex))
}

/// Encodes a message body for the forms that are not the submit form, so no
/// renderer has to carry a codec.
#[tauri::command]
pub async fn encode_body(
    ctl: Ctl<'_>,
    text: String,
    data_coding: u8,
    packing: Gsm7Packing,
) -> Result<EncodedBody, String> {
    Ok(ctl.encode_body(&text, data_coding, packing))
}

#[tauri::command]
pub async fn measure(ctl: Ctl<'_>, req: MeasureRequest) -> Result<Measurement, String> {
    Ok(ctl.measure(&req))
}

/* -------------------------------------------------------------------- */
/* burst and load test                                                  */

#[tauri::command]
pub async fn start_burst(ctl: Ctl<'_>, req: StartBurstRequest) -> Result<BurstDone, String> {
    ctl.report("start_burst", ctl.start_burst(req).await)
}

#[tauri::command]
pub async fn stop_burst(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.stop_burst();
    Ok(())
}

#[tauri::command]
pub async fn burst_rows(
    ctl: Ctl<'_>,
    from: usize,
    limit: usize,
) -> Result<Vec<WireBurstRow>, String> {
    Ok(ctl.burst_rows(from, limit))
}

#[tauri::command]
pub async fn start_load_test(ctl: Ctl<'_>, req: StartLoadRequest) -> Result<LoadReport, String> {
    ctl.report("start_load_test", ctl.start_load_test(req).await)
}

#[tauri::command]
pub async fn stop_load_test(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.stop_load_test();
    Ok(())
}

#[tauri::command]
pub async fn load_report(ctl: Ctl<'_>) -> Result<Option<LoadReport>, String> {
    Ok(ctl.load_report())
}

/* -------------------------------------------------------------------- */
/* trace                                                                */

#[tauri::command]
pub async fn trace(ctl: Ctl<'_>, from: u64, limit: usize) -> Result<Vec<WirePdu>, String> {
    Ok(ctl.trace(from, limit))
}

#[tauri::command]
pub async fn pdu_fields(ctl: Ctl<'_>, seq: u64) -> Result<Vec<DescribedField>, String> {
    Ok(ctl.pdu_fields(seq))
}

#[tauri::command]
pub async fn pdu_hex(ctl: Ctl<'_>, seq: u64) -> Result<String, String> {
    Ok(ctl.pdu_hex(seq))
}

#[tauri::command]
pub async fn clear_trace(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.clear_trace();
    Ok(())
}

#[tauri::command]
pub async fn set_trace_cap(ctl: Ctl<'_>, cap: usize) -> Result<(), String> {
    ctl.set_trace_cap(cap);
    Ok(())
}

/// Asks for a destination and writes the trace there. `None` means the user
/// closed the dialog, which is not an error.
#[tauri::command]
pub async fn export_trace(
    app: AppHandle,
    ctl: Ctl<'_>,
    format: ExportFormat,
) -> Result<Option<String>, String> {
    let Some(path) = ask_save(&app, &format!("trace.{}", format.extension())).await else {
        return Ok(None);
    };
    ctl.report("export_trace", ctl.export_trace(format, &path))
        .map(Some)
}

#[tauri::command]
pub async fn export_burst(app: AppHandle, ctl: Ctl<'_>) -> Result<Option<String>, String> {
    let Some(path) = ask_save(&app, "burst.csv").await else {
        return Ok(None);
    };
    ctl.report("export_burst", ctl.export_burst(&path))
        .map(Some)
}

/* -------------------------------------------------------------------- */
/* profiles and files                                                   */

#[tauri::command]
pub async fn list_profiles(ctl: Ctl<'_>) -> Result<Vec<Profile>, String> {
    Ok(ctl.list_profiles())
}

#[tauri::command]
pub async fn save_profile(
    ctl: Ctl<'_>,
    profile: Profile,
    password: Option<String>,
    key_passphrase: Option<String>,
) -> Result<(), String> {
    ctl.report(
        "save_profile",
        ctl.save_profile(&profile, password.as_deref(), key_passphrase.as_deref()),
    )
}

#[tauri::command]
pub async fn remove_profile(ctl: Ctl<'_>, id: String) -> Result<(), String> {
    ctl.report("remove_profile", ctl.remove_profile(&id))
}

#[tauri::command]
pub async fn profile_secrets(ctl: Ctl<'_>, id: String) -> Result<ProfileSecrets, String> {
    Ok(ctl.profile_secrets(&id))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    pub path: String,
    /// Only a CSV's contents come back. A key's bytes have no business in the
    /// renderer: the profile keeps the path and the backend reads it at
    /// connect time.
    pub text: Option<String>,
}

#[tauri::command]
pub async fn pick_file(app: AppHandle, kind: String) -> Result<Option<PickedFile>, String> {
    let filters: &[(&str, &[&str])] = match kind.as_str() {
        "csv" => &[("Recipient list", &["csv", "txt"])],
        "ca" => &[("Certificate", &["pem", "crt", "cer"])],
        "cert" => &[("Certificate", &["pem", "crt", "cer"])],
        "key" => &[("Private key", &["pem", "key"])],
        other => return Err(format!("there is no file picker for \"{other}\"")),
    };

    let Some(path) = ask_open(&app, filters).await else {
        return Ok(None);
    };

    let text = if kind == "csv" {
        Some(std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?)
    } else {
        None
    };
    Ok(Some(PickedFile { path, text }))
}

/* -------------------------------------------------------------------- */

async fn ask_open(app: &AppHandle, filters: &[(&str, &[&str])]) -> Option<String> {
    let mut builder = app.dialog().file();
    for (name, extensions) in filters {
        builder = builder.add_filter(*name, extensions);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    rx.await.ok().flatten().map(|p| p.to_string())
}

async fn ask_save(app: &AppHandle, suggested: &str) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(suggested)
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    rx.await.ok().flatten().map(|p| p.to_string())
}

/// The handler list. It lives beside `COMMAND_NAMES` on purpose: a command
/// declared but never registered compiles cleanly and then does nothing at
/// runtime, and a test here checks the two lists against each other.
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        connect,
        disconnect,
        bind,
        unbind,
        send_pdu,
        send_raw,
        compose_submit,
        preview_pdu,
        measure,
        encode_body,
        start_burst,
        stop_burst,
        burst_rows,
        start_load_test,
        stop_load_test,
        load_report,
        trace,
        pdu_fields,
        pdu_hex,
        clear_trace,
        set_trace_cap,
        export_trace,
        export_burst,
        list_profiles,
        save_profile,
        remove_profile,
        profile_secrets,
        pick_file,
        status
    ]
}

/// The production emitter: straight to the webview.
struct TauriEmitter {
    app: AppHandle,
}

impl Emitter for TauriEmitter {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        use tauri::Emitter as _;
        // A window that has gone away is not an error worth reporting: there
        // is nobody left to report it to.
        let _ = self.app.emit(name, payload);
    }
}

/// Assembles the controller and hands it to Tauri's state.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let hub = Arc::new(EventHub::new(Arc::new(TauriEmitter { app: app.clone() })));
    tauri::async_runtime::spawn(hub.clone().flush_loop(DEFAULT_BATCH));

    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not find a place to keep the profiles: {e}"))?;
    let profiles = ProfileStore::new(dir, Box::new(KeyringSecrets));

    app.manage(Arc::new(Controller::new(hub, profiles)));
    Ok(())
}

/// A counters snapshot, named so the renderer's type is reachable from here.
pub type CountersSnapshot = std::collections::BTreeMap<String, Counter>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The handler list as written in this file. A command declared but not
    /// registered compiles cleanly and then does nothing at runtime, so the
    /// two lists are checked against each other rather than trusted.
    const SOURCE: &str = include_str!("commands.rs");

    fn handler_block() -> String {
        let start = SOURCE
            .find("generate_handler![")
            .expect("the handler list moved out of this file");
        let end = SOURCE[start..]
            .find(']')
            .expect("an unterminated handler list")
            + start;
        SOURCE[start..end].to_string()
    }

    #[test]
    fn every_command_the_renderer_calls_is_registered() {
        let block = handler_block();
        for name in COMMAND_NAMES {
            assert!(
                block.contains(&format!("{name},")) || block.contains(&format!("{name}\n")),
                "`{name}` is in COMMAND_NAMES but not registered with Tauri"
            );
        }
    }

    #[test]
    fn every_registered_command_is_one_the_renderer_knows_about() {
        let block = handler_block();
        let registered: Vec<&str> = block
            .trim_start_matches("generate_handler![")
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        for name in &registered {
            assert!(
                COMMAND_NAMES.contains(name),
                "`{name}` is registered but missing from COMMAND_NAMES"
            );
        }
        assert_eq!(
            registered.len(),
            COMMAND_NAMES.len(),
            "the two lists are different lengths: {registered:?}"
        );
    }

    #[test]
    fn names_no_command_twice() {
        let mut sorted = COMMAND_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), COMMAND_NAMES.len());
    }
}
