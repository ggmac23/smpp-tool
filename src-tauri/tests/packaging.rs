//! What the bundle is, and what it must not contain.
//!
//! These read the configuration rather than building, so they run in a second
//! and fail the moment someone adds a resource that should not ship. The
//! actual build is checked by hand and recorded in the ledger, because it
//! takes minutes and needs a Mac.

use std::path::Path;

fn config() -> serde_json::Value {
    let raw = std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json");
    serde_json::from_str(&raw).expect("tauri.conf.json must be valid JSON")
}

#[test]
fn bundles_for_macos_without_signing() {
    let cfg = config();
    let bundle = &cfg["bundle"];

    assert_eq!(bundle["active"], true, "the bundle must be switched on");

    let targets: Vec<&str> = bundle["targets"]
        .as_array()
        .expect("bundle.targets must be a list")
        .iter()
        .map(|t| t.as_str().unwrap_or_default())
        .collect();
    assert!(
        targets.contains(&"app"),
        "a .app is what gets launched: {targets:?}"
    );
    assert!(
        targets.contains(&"dmg"),
        "a .dmg is what gets handed to someone: {targets:?}"
    );

    // Signing is the user's to arrange with their own certificate. A pinned
    // identity here would fail the build on every other machine.
    assert!(
        bundle["macOS"]["signingIdentity"].is_null(),
        "no signing identity may be pinned in the repository"
    );
    assert!(
        bundle["macOS"]["providerShortName"].is_null(),
        "no notarisation account may be pinned in the repository"
    );
}

#[test]
fn does_not_package_the_test_certificates() {
    let cfg = config();

    // Nothing is declared as a bundled resource at all, which is the simplest
    // way for a private key never to end up inside the app.
    let resources = &cfg["bundle"]["resources"];
    assert!(
        resources.is_null() || resources.as_array().map(|r| r.is_empty()).unwrap_or(false),
        "the bundle declares resources ({resources}); a private key must not ride along"
    );

    // The frontend directory is copied wholesale, so nothing key-shaped may
    // be sitting in it.
    let dist = cfg["build"]["frontendDist"]
        .as_str()
        .expect("build.frontendDist");
    let dist = Path::new("..").join(dist.trim_start_matches("../"));
    if dist.exists() {
        let mut offenders = Vec::new();
        walk(&dist, &mut |path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if name.ends_with(".key") || name.ends_with(".pem") || name.ends_with(".crt") {
                offenders.push(path.display().to_string());
            }
        });
        assert!(
            offenders.is_empty(),
            "these would be copied into the app: {offenders:?}"
        );
    }
}

#[test]
fn keeps_the_fixtures_out_of_the_crate_the_bundle_is_built_from() {
    // The test certificates live beside the TypeScript tests, outside
    // src-tauri, so `cargo build` cannot reach them even by accident.
    assert!(
        !Path::new("tests/fixtures").exists(),
        "the TLS fixtures must stay outside the Rust crate"
    );
    assert!(
        Path::new("../tests/fixtures/tls/ca.key").exists(),
        "the fixtures moved; this test is checking the wrong place"
    );
}

fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}
