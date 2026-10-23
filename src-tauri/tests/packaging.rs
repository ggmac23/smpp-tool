//! What the bundle is, and what it must not contain.
//!
//! These read the configuration rather than building, so they run in a second
//! and fail the moment someone adds a resource that should not ship. The
//! build itself is checked by the release workflow, which runs on all four
//! runners because a bundle is the one thing that cannot be cross-compiled.

use std::path::Path;

fn config() -> serde_json::Value {
    let raw = std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json");
    serde_json::from_str(&raw).expect("tauri.conf.json must be valid JSON")
}

#[test]
fn bundles_for_every_platform_the_release_builds_for() {
    // This test used to assert the targets were `app` and `dmg`, which is to
    // say it asserted the bug: the release workflow builds on four runners,
    // and on two of them the bundler produced nothing at all, so the build
    // went green and the installers never existed. A target list is a list of
    // the platforms this is actually released for.
    let cfg = config();
    let bundle = &cfg["bundle"];
    assert_eq!(bundle["active"], true, "the bundle must be switched on");

    let workflow = std::fs::read_to_string("../.github/workflows/release.yml")
        .expect("the release workflow says which platforms are built for");

    let formats: &[(&str, &str, &[&str])] = &[
        ("macos-", "macOS", &["app", "dmg"]),
        ("ubuntu-", "Linux", &["deb", "rpm", "appimage"]),
        ("windows-", "Windows", &["msi", "nsis"]),
    ];

    for (runner, platform, wanted) in formats {
        if !workflow.contains(runner) {
            continue;
        }
        let covered = match &bundle["targets"] {
            serde_json::Value::String(s) => s == "all",
            serde_json::Value::Array(list) => list
                .iter()
                .filter_map(|t| t.as_str())
                .any(|t| wanted.contains(&t)),
            other => panic!("bundle.targets must be \"all\" or a list, not {other}"),
        };
        assert!(
            covered,
            "the release builds on {runner}* for {platform}, and {} bundles \
             nothing there — that job produces an executable and no installer",
            bundle["targets"]
        );
    }
}

#[test]
fn signs_the_app_bundle_so_macos_does_not_call_it_damaged() {
    // This test used to require signingIdentity to be null, which is to say
    // it required the bug: with no identity Tauri leaves the bundle unsigned,
    // the binary keeps only the ad-hoc signature the linker puts on it, and
    // the bundle has no _CodeSignature at all. macOS then reads the signature
    // as invalid rather than absent, and tells whoever downloaded it that the
    // app "is damaged and can't be opened" — a dead end, because right-click
    // and Open does not get past that one. Signed ad-hoc, the same app is
    // merely from an unidentified developer, which right-click and Open does
    // get past.
    let cfg = config();
    let identity = cfg["bundle"]["macOS"]["signingIdentity"].as_str();
    assert_eq!(
        identity,
        Some("-"),
        "the bundle must be signed ad-hoc (\"-\"), or macOS calls it damaged"
    );
}

#[test]
fn pins_no_certificate_of_anyone_s_own() {
    // Ad-hoc is the one identity that belongs in a repository: it needs no
    // certificate, so it builds on anybody's machine. A real Developer ID
    // here would fail the build everywhere it is not installed.
    let cfg = config();
    let bundle = &cfg["bundle"];
    let identity = bundle["macOS"]["signingIdentity"].as_str().unwrap_or("-");
    assert_eq!(
        identity, "-",
        "only ad-hoc signing may be pinned in the repository, not {identity:?}"
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

#[test]
fn the_tests_carry_a_manifest_so_they_start_on_windows() {
    // Without this, every test executable on Windows loads Common Controls
    // version 5, cannot resolve the `TaskDialogIndirect` that Tauri's dialog
    // code imports, and the process is killed by the loader before the first
    // test runs — the whole suite silently stops covering Windows, which is
    // one of the three platforms this is released for.
    let manifest = std::fs::read_to_string("windows/test.manifest")
        .expect("the test executables need a manifest of their own on Windows");
    assert!(
        manifest.contains("Microsoft.Windows.Common-Controls")
            && manifest.contains("version=\"6.0.0.0\""),
        "the manifest must ask for Common Controls 6: {manifest}"
    );

    let build = std::fs::read_to_string("build.rs").unwrap();
    assert!(
        build.contains("rustc-link-arg-tests") && build.contains("MANIFESTINPUT"),
        "build.rs must embed that manifest into the test executables"
    );
}

#[test]
fn the_three_places_that_carry_a_version_agree() {
    // The version is written in three files and read from a fourth: the
    // bundle takes tauri.conf.json, the crate takes Cargo.toml, and the
    // release notes are written against whatever the tag says. Let them drift
    // and the installer is called one thing while the application reports
    // another, which is the sort of thing nobody notices until a bug report
    // names a version that was never built.
    let bundle = config()["version"]
        .as_str()
        .expect("tauri.conf.json must carry a version")
        .to_string();

    let cargo = std::fs::read_to_string("Cargo.toml").unwrap();
    let crate_version = cargo
        .lines()
        .skip_while(|l| l.trim() != "[package]")
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("Cargo.toml must carry a version");

    let package: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("../package.json").unwrap()).unwrap();
    let npm_version = package["version"]
        .as_str()
        .expect("package.json must carry a version")
        .to_string();

    assert_eq!(
        (crate_version.as_str(), npm_version.as_str()),
        (bundle.as_str(), bundle.as_str()),
        "the version differs between tauri.conf.json, Cargo.toml and package.json"
    );
}
