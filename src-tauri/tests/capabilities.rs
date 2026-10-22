//! The capability that lets the webview listen at all.
//!
//! Tauri v2 denies a core plugin command unless a capability grants it, and
//! `event|listen` is one. Without it every command still works and not one
//! event ever arrives: the trace stays empty, the counters never move, burst
//! rows never appear and the load test reports nothing until it returns. It
//! fails silently, which is why it is checked here.

use std::path::Path;

fn capability() -> serde_json::Value {
    let path = Path::new("capabilities/default.json");
    assert!(
        path.exists(),
        "without a capability file the webview may not listen for events"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .expect("capabilities/default.json must be valid JSON")
}

fn permissions() -> Vec<String> {
    capability()["permissions"]
        .as_array()
        .expect("the capability must list permissions")
        .iter()
        .map(|p| {
            p.as_str()
                .map(str::to_string)
                .or_else(|| p["identifier"].as_str().map(str::to_string))
                .expect("a permission is a name or an object with an identifier")
        })
        .collect()
}

#[test]
fn the_webview_may_listen_for_events() {
    let granted = permissions();
    assert!(
        granted.iter().any(|p| p == "core:default" || p == "core:event:default"),
        "nothing grants event:listen, so no event would ever reach the renderer: {granted:?}"
    );
}

#[test]
fn the_file_dialogs_are_allowed() {
    let granted = permissions();
    assert!(
        granted.iter().any(|p| p.starts_with("dialog:")),
        "export and pick-file open a dialog, which needs its own permission: {granted:?}"
    );
}

#[test]
fn the_capability_covers_the_window_the_app_opens() {
    let cap = capability();
    let windows: Vec<&str> = cap["windows"]
        .as_array()
        .expect("the capability must name its windows")
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect();

    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    let label = cfg["app"]["windows"][0]["label"]
        .as_str()
        .unwrap_or("main");

    assert!(
        windows.contains(&label) || windows.contains(&"*"),
        "the capability covers {windows:?}, but the app opens {label:?}"
    );
}
