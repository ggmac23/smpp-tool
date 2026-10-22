//! The production wiring, exercised without a window.
//!
//! `lib.rs::run()` is the one place the parity harness cannot reach: it builds
//! its own Tauri app. A command missing from the real handler list, or an
//! `install()` that never runs, would leave the packaged application dead in
//! a way every other test here would miss.
//!
//! The mock runtime does not call `setup`, so `install` is called here the
//! way `run()`'s setup closure calls it.

use std::sync::Arc;

use smppclient::app::controller::Controller;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{App, Manager};

fn app() -> App<MockRuntime> {
    mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(smppclient::commands::handler())
        .build(mock_context(noop_assets()))
        .expect("the application must build with the real handler list")
}

#[test]
fn install_puts_a_controller_where_the_commands_look_for_it() {
    let app = app();
    smppclient::commands::install(&app.handle().clone()).expect("install must succeed");
    assert!(
        app.try_state::<Arc<Controller>>().is_some(),
        "no controller in state: every command would fail at the first call"
    );
}

#[test]
fn the_controller_reports_an_unbound_session_before_anything_happens() {
    let app = app();
    smppclient::commands::install(&app.handle().clone()).expect("install must succeed");

    let controller = app.state::<Arc<Controller>>().inner().clone();
    let status = controller.status();
    assert!(!status.connected);
    assert_eq!(status.pending_buffered, 0);
    assert!(controller.load_report().is_none());
    assert!(controller.trace(0, 10).is_empty());
}

#[test]
fn the_profile_store_is_rooted_somewhere_writable() {
    // `install` asks the platform where configuration goes. If that fails the
    // application cannot save a profile at all, and the failure belongs at
    // start-up rather than at the first save.
    let app = app();
    assert!(
        app.path().app_config_dir().is_ok(),
        "no config directory, so the profile store has nowhere to live"
    );
}

#[test]
fn installing_twice_does_not_panic() {
    // A reload or a second window must not take the application down.
    let app = app();
    smppclient::commands::install(&app.handle().clone()).expect("first install");
    smppclient::commands::install(&app.handle().clone()).expect("second install");
    assert!(app.try_state::<Arc<Controller>>().is_some());
}
