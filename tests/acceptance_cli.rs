use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn command(root: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
    command
        .current_dir(root)
        .env("WCODE_STATE_DIR", state)
        .args(["--no-monitor", "--no-semantic", "--read-only"]);
    command
}

#[test]
fn one_configured_repository_with_discovered_package_can_inspect_native_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("crates/member/src")).unwrap();
    for (name, content) in [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/member\"]\nresolver = \"2\"\n",
        ),
        (
            "crates/member/Cargo.toml",
            "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("crates/member/src/lib.rs", "pub fn answer() -> u8 { 42 }\n"),
    ] {
        std::fs::write(root.path().join(name), content).unwrap();
    }
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(root.path())
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--quiet"]);
    git(&[
        "add",
        "Cargo.toml",
        "crates/member/Cargo.toml",
        "crates/member/src/lib.rs",
    ]);
    git(&[
        "-c",
        "user.name=Acceptance Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "Native fixture",
    ]);
    let head = git(&["rev-parse", "HEAD"]);

    for check in [false, true] {
        let mut invocation = command(root.path(), state.path());
        invocation.args([
            "acceptance",
            "inspect",
            "--base",
            &head,
            "--head",
            &head,
            "--json",
        ]);
        if check {
            invocation.arg("--check");
        }
        let output = invocation.output().unwrap();
        let record: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "No native record: {error}; {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(output.status.success(), !check);
        assert_eq!(record["producer"], "wcode/native-acceptance/v1");
        assert_eq!(record["state"], "incomplete");
        assert!(record["policy"].is_null());
        assert!(record["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "policy_inactive"));
        assert!(!record["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "git_capture_incomplete"));
        assert!(
            git(&["status", "--porcelain"]).is_empty(),
            "Inspection changed the repository"
        );
    }
    assert!(
        !root.path().join(".wcode").exists(),
        "Inspection activated project configuration"
    );
}

#[test]
fn explicitly_selected_independent_roots_remain_rejected_before_acceptance_capture() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let output = command(first.path(), state.path())
        .arg("--workspace")
        .arg(first.path())
        .arg("--workspace")
        .arg(second.path())
        .args(["acceptance", "inspect", "--base", &"a".repeat(40), "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Acceptance accepts one --workspace"));
    assert_eq!(std::fs::read_dir(first.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(second.path()).unwrap().count(), 0);
}
