//! The icon set, checked where it is cheap to check.
//!
//! Tauri rejects an RGB PNG outright, and a bundle referring to a file that
//! is not there fails minutes into the build. Both are a second's work here.

use std::path::Path;

fn config() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json"))
        .expect("tauri.conf.json must be valid JSON")
}

#[test]
fn every_icon_the_bundle_names_is_there() {
    let cfg = config();
    for icon in cfg["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon must be a list")
    {
        let path = icon.as_str().expect("an icon path");
        assert!(
            Path::new(path).exists(),
            "the bundle names {path}, which does not exist"
        );
    }
}

#[test]
fn the_pngs_carry_an_alpha_channel() {
    // Tauri refuses an RGB PNG, and the refusal comes late in the build.
    let cfg = config();
    for icon in cfg["bundle"]["icon"].as_array().unwrap() {
        let path = icon.as_str().unwrap();
        if !path.ends_with(".png") {
            continue;
        }
        let bytes = std::fs::read(path).expect("the icon must be readable");
        // IHDR: 8 bytes of signature, 4 of length, 4 of type, then width,
        // height, bit depth, colour type.
        let colour_type = bytes[25];
        assert_eq!(
            colour_type, 6,
            "{path} has colour type {colour_type}; Tauri needs 6 (RGBA)"
        );
    }
}

#[test]
fn names_a_windows_icon_as_well_as_a_mac_one() {
    let cfg = config();
    let icons: Vec<&str> = cfg["bundle"]["icon"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_str().unwrap())
        .collect();
    assert!(icons.iter().any(|i| i.ends_with(".icns")), "{icons:?}");
    assert!(icons.iter().any(|i| i.ends_with(".ico")), "{icons:?}");
}
