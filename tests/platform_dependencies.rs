//! The menu-bar event loop is macOS-only; unrelated window backends must not
//! silently add Linux font/rendering packages to the published dependency graph.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

#[test]
fn macos_event_loop_keeps_native_handles_without_linux_default_backends() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert!(manifest["dependencies"].get("winit").is_none());
    let dependency = &manifest["target"]["cfg(target_os = \"macos\")"]["dependencies"]["winit"];
    assert_eq!(
        dependency.get("default-features").and_then(Item::as_bool),
        Some(false),
        "macOS does not need the Wayland/X11 defaults"
    );
    let features: BTreeSet<_> = dependency
        .get("features")
        .and_then(Item::as_array)
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(
        features.contains("rwh_06"),
        "native window handle support stays enabled"
    );
    for unused in ["x11", "wayland", "wayland-dlopen", "wayland-csd-adwaita"] {
        assert!(!features.contains(unused), "unexpected backend: {unused}");
    }
}

#[test]
fn lockfile_has_no_unused_linux_window_font_dependency_chain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    let names: BTreeSet<_> = lock["package"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect();
    for unused in ["sctk-adwaita", "ab_glyph", "owned_ttf_parser", "ttf-parser"] {
        assert!(
            !names.contains(unused),
            "unused menu-bar backend still brings {unused}"
        );
    }
    assert!(
        names.contains("winit"),
        "do not remove the menu-bar feature to satisfy this test"
    );
}
