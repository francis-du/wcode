use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const EVENT_KEY: &str = "integration-webhook-test-key-not-a-live-secret";

#[cfg(unix)]
#[test]
fn github_app_credentials_cli_selects_key_source_before_network() {
    let temp = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(temp.path())
        .args([
            "github",
            "preflight",
            "--config-root",
            config.path().to_str().unwrap(),
            "--pull",
            "7",
            "--json",
        ])
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .env(
            "WCODE_GITHUB_APP_KEY_FILE",
            temp.path().join("missing-offline-fixture.pem"),
        )
        .env("WCODE_GITHUB_INSTALLATION_ID", "789")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("signing key is invalid or unsafe"),
        "renewable credential source was not selected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn github_app_credentials_cli_in_memory_key_is_portable_and_source_exclusive() {
    let temp = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    for add_file_source in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
        command
            .current_dir(temp.path())
            .args([
                "github",
                "preflight",
                "--config-root",
                config.path().to_str().unwrap(),
                "--pull",
                "7",
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env("WCODE_GITHUB_INSTALLATION_ID", "789")
            .env("WCODE_GITHUB_APP_PRIVATE_KEY", "OFFLINE-INLINE-NOT-A-KEY");
        if add_file_source {
            command.env(
                "WCODE_GITHUB_APP_PRIVATE_KEY_FILE",
                config.path().join("never-read.pem"),
            );
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if add_file_source {
                "exactly one signing-key source"
            } else {
                "unsupported signing key encoding"
            }),
            "{error}"
        );
        assert!(!error.contains("OFFLINE-INLINE-NOT-A-KEY"));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn github_app_credentials_cli_rejects_ambiguity_and_invalid_installation_before_access() {
    let temp = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    for (id, token, expected) in [
        (None, false, "positive trusted installation"),
        (Some("0"), false, "positive trusted installation"),
        (Some(" 789"), false, "positive trusted installation"),
        (Some("789"), true, "sources are ambiguous"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
        command
            .current_dir(temp.path())
            .args([
                "github",
                "preflight",
                "--config-root",
                config.path().to_str().unwrap(),
                "--pull",
                "7",
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env_remove("WCODE_GITHUB_INSTALLATION_ID")
            .env(
                "WCODE_GITHUB_APP_KEY_FILE",
                config.path().join("never-read.pem"),
            );
        if let Some(id) = id {
            command.env("WCODE_GITHUB_INSTALLATION_ID", id);
        }
        if token {
            command.env("WCODE_GITHUB_PUBLISHER_TOKEN", "OFFLINE-SECRET\ninvalid");
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("OFFLINE-SECRET"));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn github_publication_rejects_authority_state_inside_candidate_before_credentials() {
    let root = tempfile::tempdir().unwrap();
    let candidate = root.path().join("candidate");
    let config = root.path().join("config");
    std::fs::create_dir_all(&candidate).unwrap();
    std::fs::create_dir_all(config.join(".wcode")).unwrap();
    std::fs::write(
        config.join(".wcode/github-publisher.yaml"),
        "schema_version: 1\nrepository: owner/repo\nrepository_id: 123\napp_id: 456\ncheck_name: gate\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(&candidate)
        .args([
            "-w",
            candidate.to_str().unwrap(),
            "github",
            "publish",
            "--config-root",
            config.to_str().unwrap(),
            "--pull",
            "7",
            "--workspace-id",
            "fixture",
            "--base",
            &"a".repeat(40),
            "--head",
            &"b".repeat(40),
        ])
        .env("WCODE_STATE_DIR", &candidate)
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .env_remove("WCODE_GITHUB_APP_PRIVATE_KEY")
        .env_remove("WCODE_GITHUB_APP_PRIVATE_KEY_FILE")
        .env_remove("WCODE_GITHUB_APP_KEY_FILE")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("authority state must not overlap"),
        "{error}"
    );
}

#[test]
fn github_app_credentials_cli_publication_checks_safety_and_key_location_before_access() {
    let temp = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    for restricted in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
        command.current_dir(temp.path());
        if restricted {
            command.arg("--read-only");
        }
        command
            .args([
                "github",
                "publish",
                "--config-root",
                config.path().to_str().unwrap(),
                "--pull",
                "7",
                "--workspace-id",
                "fixture",
                "--base",
                &"a".repeat(40),
                "--head",
                &"b".repeat(40),
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env(
                "WCODE_GITHUB_APP_KEY_FILE",
                temp.path().canonicalize().unwrap().join("never-read.pem"),
            )
            .env("WCODE_GITHUB_INSTALLATION_ID", "789");
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if restricted {
                "publication is disabled"
            } else {
                "outside candidate and inbox"
            }),
            "{error}"
        );
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn github_app_credentials_cli_event_authentication_and_installation_precede_key_access() {
    let temp = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    let body = event_body();
    for valid in [false, true] {
        let signature = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, EVENT_KEY.as_bytes()),
            if valid { &body } else { b"different" },
        );
        let signature = format!(
            "sha256={}",
            signature
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(temp.path())
            .args([
                "github",
                "publish-event",
                "--config-root",
                config.path().to_str().unwrap(),
                "--installation-id",
                "789",
                "--workspace-id",
                "fixture",
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env(
                "WCODE_GITHUB_APP_KEY_FILE",
                config.path().join("never-read.pem"),
            )
            .env("WCODE_GITHUB_INSTALLATION_ID", "790")
            .env("WCODE_GITHUB_WEBHOOK_SECRET", EVENT_KEY)
            .env("WCODE_GITHUB_SIGNATURE_256", signature)
            .env("WCODE_GITHUB_EVENT", "pull_request")
            .env(
                "WCODE_GITHUB_DELIVERY",
                "72d3162e-cc78-11e3-81ab-4c9367dc0958",
            )
            .env("WCODE_GITHUB_CONTENT_TYPE", "application/json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&body).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if valid {
                "installation differs"
            } else {
                "signature verification failed"
            }),
            "{error}"
        );
        assert!(!error.contains(EVENT_KEY));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn github_deployment_template_invokes_the_installed_cli_with_guarded_exact_candidate() {
    let template = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/git-provider/github-gate.yml"
    ))
    .unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&template).unwrap();
    let script = value["jobs"]["publish"]["steps"][0]["run"]
        .as_str()
        .unwrap();
    assert!(
        script.contains("/opt/wcode/bin/wcode"),
        "template uses an obsolete publisher entrypoint"
    );
    assert!(
        !script.contains("${{"),
        "untrusted expressions must be passed through environment, not shell source"
    );
    assert!(!script.contains("--owner"));
    let script = script.replace(
        "/opt/wcode/bin/wcode",
        "\"${WCODE_TEST_BINARY}\" --read-only",
    );
    let root = tempfile::tempdir().unwrap();
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(root.path())
        .env("WCODE_TEST_BINARY", env!("CARGO_BIN_EXE_wcode"))
        .env("NATIVE_WORKSPACE", root.path().join("candidate"))
        .env("NATIVE_WORKSPACE_ID", "candidate")
        .env("PUBLISHER_CONFIG_ROOT", root.path().join("config"))
        .env("WCODE_STATE_DIR", root.path().join("state"))
        .env("GITHUB_REPOSITORY", "francis-du/wcode")
        .env("PULL_NUMBER", "7")
        .env("CANDIDATE_BASE", "a".repeat(40))
        .env("CANDIDATE_HEAD", "b".repeat(40))
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .env_remove("WCODE_GITHUB_WEBHOOK_SECRET")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("publication is disabled"),
        "template failed to reach the real safety guard: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

fn event_body() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "action": "synchronize", "number": 7,
        "installation": {"id": 789},
        "repository": {"id": 123, "full_name": "owner/repo"},
        "pull_request": {
            "number": 7, "state": "open", "draft": false, "merged": false,
            "base": {"sha": "a".repeat(40), "repo": {"id":123,"full_name":"owner/repo"}},
            "head": {"sha": "b".repeat(40), "repo": {"id":987,"full_name":"fork/repo"}},
            "body": "私人源码不应输出"
        },
        "native_verification": true, "human_approval": true, "ready": true
    }))
    .unwrap()
}

fn event_config(root: &Path) {
    std::fs::create_dir(root.join(".wcode")).unwrap();
    std::fs::write(root.join(".wcode/github-publisher.yaml"),
        "schema_version: 1\nrepository: owner/repo\nrepository_id: 123\napp_id: 456\ncheck_name: gate\n").unwrap();
}

fn invoke_event(
    root: &Path,
    config: &Path,
    command: &str,
    body: &[u8],
    valid: bool,
    installation: &str,
) -> Output {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, EVENT_KEY.as_bytes());
    let signed = ring::hmac::sign(&key, if valid { body } else { b"different content" });
    let signature = format!(
        "sha256={}",
        signed
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let mut process = Command::new(env!("CARGO_BIN_EXE_wcode"));
    process
        .current_dir(root)
        .args([
            "github",
            command,
            "--config-root",
            config.to_str().unwrap(),
            "--installation-id",
            installation,
            "--json",
        ])
        .env("WCODE_GITHUB_WEBHOOK_SECRET", EVENT_KEY)
        .env("WCODE_GITHUB_SIGNATURE_256", signature)
        .env("WCODE_GITHUB_EVENT", "pull_request")
        .env(
            "WCODE_GITHUB_DELIVERY",
            "72d3162e-cc78-11e3-81ab-4c9367dc0958",
        )
        .env("WCODE_GITHUB_CONTENT_TYPE", "application/json")
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if command == "publish-event" {
        process.args(["--workspace-id", "project"]);
    }
    let mut child = process.spawn().unwrap();
    child.stdin.take().unwrap().write_all(body).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn github_event_signed_candidate_is_readonly_and_never_imports_proof() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    let before = std::fs::read(config.path().join(".wcode/github-publisher.yaml")).unwrap();
    let body = event_body();
    let output = invoke_event(
        root.path(),
        config.path(),
        "verify-event",
        &body,
        true,
        "789",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["target"]["change"], 7);
    assert_eq!(result["target"]["head_sha"], "b".repeat(40));
    for field in [
        "native_verification",
        "human_approval",
        "freshness_verified",
        "replay_protection_verified",
        "remote_mutations",
    ] {
        assert_eq!(result[field], false);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("私人源码"));
    assert!(!text.contains(EVENT_KEY));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert_eq!(
        std::fs::read(config.path().join(".wcode/github-publisher.yaml")).unwrap(),
        before
    );
}

#[test]
fn github_event_invalid_input_is_rejected_before_publisher_credential_access() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    for command in ["verify-event", "publish-event"] {
        let rejected = invoke_event(
            root.path(),
            config.path(),
            command,
            b"sensitive invalid JSON",
            false,
            "789",
        );
        assert!(!rejected.status.success());
        let error = String::from_utf8_lossy(&rejected.stderr);
        assert!(error.contains("signature verification failed"), "{error}");
        assert!(!error.contains("sensitive invalid JSON"));
        assert!(!error.contains(EVENT_KEY));
        let wrong_installation = invoke_event(
            root.path(),
            config.path(),
            command,
            &event_body(),
            true,
            "999",
        );
        assert!(!wrong_installation.status.success());
        assert!(String::from_utf8_lossy(&wrong_installation.stderr)
            .contains("does not match enrollment"));
    }
    // The valid event reaches the existing protected publisher, never a shell or PR script.
    let valid = invoke_event(
        root.path(),
        config.path(),
        "publish-event",
        &event_body(),
        true,
        "789",
    );
    assert!(!valid.status.success());
    assert!(String::from_utf8_lossy(&valid.stderr).contains("publisher credential is missing"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_publish_event_safety_rejects_before_stdin_configuration_or_keys() {
    let root = tempfile::tempdir().unwrap();
    for flag in ["--read-only", "--no-exec"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args([
                flag,
                "github",
                "publish-event",
                "--config-root",
                "absent",
                "--installation-id",
                "789",
                "--workspace-id",
                "project",
            ])
            .env_remove("WCODE_GITHUB_WEBHOOK_SECRET")
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("disabled"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_event_commands_are_available_without_credentials_or_runtime() {
    let root = tempfile::tempdir().unwrap();
    for command in ["verify-event", "publish-event"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["github", command, "--help"])
            .env_remove("WCODE_GITHUB_WEBHOOK_SECRET")
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--config-root"));
        assert!(help.contains("--installation-id"));
        assert!(!help.contains("--secret"));
        assert!(!help.contains("--token"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_watch_help_and_safety_are_available_without_credentials_or_runtime() {
    let root = tempfile::tempdir().unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(["github", "watch-candidate", "--help"])
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .output()
        .unwrap();
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    let text = String::from_utf8_lossy(&help.stdout);
    for argument in [
        "--base",
        "--head",
        "--pull",
        "--workspace-id",
        "--config-root",
    ] {
        assert!(text.contains(argument), "missing {argument}");
    }
    assert!(!text.contains("--token"));
    for flag in ["--read-only", "--no-exec"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args([
                flag,
                "github",
                "watch-candidate",
                "--config-root",
                "missing",
                "--pull",
                "7",
                "--workspace-id",
                "project",
                "--base",
                &"a".repeat(40),
                "--head",
                &"b".repeat(40),
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("publication is disabled"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_watch_disjoint_roots_and_missing_credentials_fail_before_startup() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    event_config(config.path());
    let before = std::fs::read(config.path().join(".wcode/github-publisher.yaml")).unwrap();
    for (path, expected) in [
        (root.path(), "must not overlap"),
        (config.path(), "credential is missing"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args([
                "github",
                "watch-candidate",
                "--config-root",
                path.to_str().unwrap(),
                "--pull",
                "7",
                "--workspace-id",
                "project",
                "--base",
                &"a".repeat(40),
                "--head",
                &"b".repeat(40),
                "--json",
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        std::fs::read(config.path().join(".wcode/github-publisher.yaml")).unwrap(),
        before
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_publisher_help_is_available_in_installed_binary_without_runtime() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["github", "--help"],
        vec!["github", "preflight", "--help"],
        vec!["github", "publish", "--help"],
        vec!["help-all", "github", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(args)
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("preflight")
                || String::from_utf8_lossy(&output.stdout).contains("--config-root")
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn github_publisher_safety_and_missing_credentials_preserve_files() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::create_dir(config.path().join(".wcode")).unwrap();
    let content="schema_version: 1\nrepository: owner/repo\nrepository_id: 1\napp_id: 2\ncheck_name: gate\n";
    std::fs::write(config.path().join(".wcode/github-publisher.yaml"), content).unwrap();
    for flag in ["--read-only", "--no-exec"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args([
                flag,
                "github",
                "publish",
                "--config-root",
                config.path().to_str().unwrap(),
                "--pull",
                "7",
                "--workspace-id",
                "project",
                "--base",
                &"a".repeat(40),
                "--head",
                &"b".repeat(40),
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("disabled"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args([
            "github",
            "preflight",
            "--config-root",
            config.path().to_str().unwrap(),
            "--pull",
            "7",
            "--check",
            "--json",
        ])
        .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("credential is missing"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_to_string(config.path().join(".wcode/github-publisher.yaml")).unwrap(),
        content
    );
}
