use std::fs;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[test]
fn release_workflow_has_one_publish_trigger_and_smokes_distributed_binaries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
    let trigger_section = workflow.split("permissions:").next().unwrap();

    assert!(trigger_section.contains("tags: [\"v*\"]"));
    assert!(trigger_section.contains("workflow_dispatch:"));
    assert!(
        !trigger_section.contains("\n  release:\n"),
        "publishing a GitHub release must not start a second release workflow for the same tag"
    );

    for required in [
        "codesign --force --sign - dist/package-arm64/wcode",
        "codesign --force --sign - dist/package-x86_64/wcode",
        "codesign --force --sign - dist/package-universal/wcode",
        "codesign --verify --strict --verbose=2 \"$binary\"",
        "dist/package-arm64/wcode --help >/dev/null",
        "dist/package-universal/wcode --help >/dev/null",
        "WCODE_BASE_URL=\"file://$PWD/dist\"",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow must keep the distribution smoke gate: {required}"
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_installer_has_valid_shell_syntax() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("sh")
        .arg("-n")
        .arg(root.join("install.sh"))
        .output()
        .expect("sh -n install.sh must run");
    assert!(
        output.status.success(),
        "install.sh syntax error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unix_installer_uses_tested_macos_asset_and_replaces_only_after_smoke_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let installer = fs::read_to_string(root.join("install.sh")).unwrap();

    for required in [
        "archive=\"wcode-macos-universal.tar.gz\"",
        "codesign --verify --strict \"$install_tmp\"",
        "\"$install_tmp\" --version >/dev/null",
        "\"$install_tmp\" --help >/dev/null",
        "mv -f \"$install_tmp\" \"$install_path\"",
    ] {
        assert!(
            installer.contains(required),
            "installer must keep the verified atomic-install contract: {required}"
        );
    }

    let smoke = installer
        .find("\"$install_tmp\" --help >/dev/null")
        .unwrap();
    let replace = installer
        .find("mv -f \"$install_tmp\" \"$install_path\"")
        .unwrap();
    assert!(
        smoke < replace,
        "installer must smoke-test before replacement"
    );
}
