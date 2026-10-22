//! SMPP client backend.
//!
//! `smpp` is the protocol layer: codec, text, segmentation, receipts. It is
//! pure — no sockets, no disk, no Tauri — so it can be tested as a library.
//!
//! `app` owns everything with side effects: the socket, the session, the
//! trace, the burst engine and the profile store.
//!
//! The renderer never builds or parses a PDU. It sends intents through the
//! Tauri commands and receives events; bytes cross as lowercase hex strings.

pub mod app;
pub mod commands;
pub mod smpp;
pub mod testing;

/// Start the application. Kept here rather than in `main.rs` so integration
/// tests can link the library without launching a window.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            commands::install(&tauri::Manager::app_handle(app).clone())?;
            Ok(())
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("the SMPP client failed to start");
}
