//! The scaffold holds the two promises everything else rests on: Tauri serves
//! the qooxdoo bundle that already exists, and that bundle is where we think.

#[test]
fn tauri_config_serves_the_qooxdoo_bundle() {
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    assert_eq!(conf["build"]["frontendDist"], "../compiled/build");
}

#[test]
fn tauri_config_builds_the_renderer_first() {
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    assert_eq!(conf["build"]["beforeBuildCommand"], "npm run build:ui");
}

#[test]
fn the_renderer_bundle_exists() {
    assert!(std::path::Path::new("../compiled/build/index.html").exists());
    assert!(std::path::Path::new("../compiled/build/forms.js").exists());
}
