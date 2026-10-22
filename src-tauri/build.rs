fn main() {
    link_the_common_controls_manifest_into_the_tests();
    tauri_build::build();
}

/// `tauri_build` links the Windows resource, and with it the application
/// manifest, into binaries only. A test executable therefore has no manifest,
/// loads Common Controls version 5, and cannot resolve `TaskDialogIndirect`
/// — Windows refuses to start the process, so not one test runs. Embedding a
/// manifest of our own into the test executables is what keeps the suite
/// honest on the platform the application ships to.
fn link_the_common_controls_manifest_into_the_tests() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows || !msvc {
        return;
    }

    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("windows")
        .join("test.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
