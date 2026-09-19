use std::fs;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[test]
fn adversarial_release_keeps_full_ci_coverage_while_supporting_local_shards() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let audit = fs::read_to_string(root.join("tests/release_audit.cjs")).unwrap();
    let webkit = fs::read_to_string(root.join("tests/unit/ui/browser_webkit.swift")).unwrap();
    let adversarial_workflow =
        fs::read_to_string(root.join(".github/workflows/adversarial.yml")).unwrap();
    let release_workflow = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();

    assert!(audit.contains("--rounds="));
    assert!(audit.contains("release-adversarial-30"));
    assert!(audit.contains("wcode-adversarial-30.json"));
    assert!(audit.contains("release_metadata_versions_match_the_cargo_package"));
    assert!(webkit.contains("--cases="));
    assert!(webkit.contains("\"total_cases\":96"));
    assert!(adversarial_workflow.contains("node tests/release_audit.cjs --require-clean"));
    assert!(adversarial_workflow.contains("swift tests/unit/ui/browser_webkit.swift"));
    assert!(release_workflow.contains("release_metadata_versions_match_the_cargo_package"));
    assert!(
        !adversarial_workflow.contains("--rounds=") && !adversarial_workflow.contains("--cases="),
        "CI must keep the complete unsharded release audit"
    );
}

#[test]
fn release_metadata_versions_match_the_cargo_package() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let package = cargo.split("[dependencies]").next().unwrap();
    let version = package
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = \"")?.strip_suffix('"'))
        .expect("package version");

    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        lock.contains(&format!(
            "[[package]]\nname = \"wcode\"\nversion = \"{version}\""
        )),
        "Cargo.lock wcode package must match Cargo.toml"
    );

    for (path, pointer) in [
        ("marketplace.json", "/plugins/0/version"),
        ("plugin/marketplace.json", "/plugins/0/version"),
        ("plugin/plugin.json", "/version"),
        ("plugin/.claude-plugin/plugin.json", "/version"),
        ("plugin/.codex-plugin/plugin.json", "/version"),
        ("plugin/.zcode-plugin/plugin.json", "/version"),
    ] {
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(path)).unwrap()).unwrap();
        assert_eq!(
            value.pointer(pointer).and_then(serde_json::Value::as_str),
            Some(version),
            "{path} must match Cargo.toml package version"
        );
    }
}

#[test]
fn release_windows_packaging_stops_after_each_native_failure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap(),
    )
    .unwrap();
    let steps = workflow["jobs"]["build"]["steps"].as_sequence().unwrap();
    let script = steps
        .iter()
        .find(|step| step["name"].as_str() == Some("Build Windows"))
        .unwrap()["run"]
        .as_str()
        .unwrap();
    for command in [
        "cargo build --release --locked",
        "& dist/package/wcode.exe --version",
        "& dist/package/wcode.exe --help",
    ] {
        let after = script
            .split_once(command)
            .unwrap_or_else(|| panic!("missing {command}"))
            .1;
        let guard = after.lines().find(|line| !line.trim().is_empty()).unwrap();
        assert!(
            guard.trim().starts_with("if ($LASTEXITCODE -ne 0)"),
            "{command} must check its native exit code before using or packaging the binary"
        );
    }
}

#[test]
fn release_ci_runs_rustsec_dependency_audit() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap(),
    )
    .unwrap();
    let steps = workflow["jobs"]["quality"]["steps"].as_sequence().unwrap();
    let audit = steps
        .iter()
        .find(|step| step["name"].as_str() == Some("Audit Rust dependencies"))
        .expect("release quality gate must audit Rust dependencies");
    assert_eq!(audit["uses"].as_str(), Some("actions-rust-lang/audit@v1"));
    assert_eq!(audit["with"]["createIssues"].as_bool(), Some(false));
    assert!(root.join(".cargo/audit.toml").is_file());
}

#[test]
fn release_ci_installs_the_javascript_behavior_runtime() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap(),
    )
    .unwrap();
    let steps = workflow["jobs"]["test"]["steps"].as_sequence().unwrap();
    let node_index = steps
        .iter()
        .position(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/setup-node@"))
        })
        .expect("install Node explicitly");
    let test_index = steps
        .iter()
        .position(|step| step["run"].as_str() == Some("cargo test --locked"))
        .unwrap();
    assert!(node_index < test_index);
    assert_eq!(
        steps[node_index]["with"]["node-version"].as_str(),
        Some("24")
    );
    assert_eq!(
        steps[node_index]["with"]["package-manager-cache"].as_bool(),
        Some(false)
    );
}

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
        "cp LICENSE dist/package/LICENSE",
        "cp LICENSE dist/package-universal/LICENSE",
        "cp LICENSE dist/package-arm64/LICENSE",
        "cp LICENSE dist/package-x86_64/LICENSE",
        "Copy-Item LICENSE dist/package/LICENSE",
        "test \"$(dist/package/wcode --version)\" = \"wcode ${package_version}\"",
        "dist/package-x86_64/wcode",
        "codesign --force --sign - dist/package-arm64/wcode",
        "codesign --force --sign - dist/package-x86_64/wcode",
        "codesign --force --sign - dist/package-universal/wcode",
        "codesign --verify --strict --verbose=2 \"$binary\"",
        "for binary in \\",
        "test \"$(\"$binary\" --version)\" = \"wcode ${package_version}\"",
        "\"$binary\" --help >/dev/null",
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
        "wcode setup",
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

#[test]
fn windows_installer_stages_and_smoke_tests_before_replacement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let installer = fs::read_to_string(root.join("install.ps1")).unwrap();
    for required in [
        "$InstallTemp = Join-Path $InstallDir",
        "& $InstallTemp --version",
        "& $InstallTemp --help",
        "Move-Item -Force $InstallTemp $InstallPath",
        "wcode setup",
    ] {
        assert!(
            installer.contains(required),
            "Windows installer must keep the staged update contract: {required}"
        );
    }
    let smoke = installer.find("& $InstallTemp --help").unwrap();
    let replace = installer
        .find("Move-Item -Force $InstallTemp $InstallPath")
        .unwrap();
    assert!(
        smoke < replace,
        "Windows installer must smoke-test before replacement"
    );
}
