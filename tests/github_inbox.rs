use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const KEY: &str = "inbox-cli-test-key-not-a-live-secret";

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    config: PathBuf,
    inbox: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let config = root.join("config");
        fs::create_dir_all(config.join(".wcode")).unwrap();
        fs::write(config.join(".wcode/github-publisher.yaml"),
            "schema_version: 1\nrepository: owner/repo\nrepository_id: 12\napp_id: 34\ncheck_name: gate\n").unwrap();
        let fixture = Self {
            inbox: root.join("inbox"),
            _temp: temp,
            root,
            config,
        };
        let output = fixture.command("inbox-init").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fixture
    }
    fn command(&self, action: &str) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wcode"));
        cmd.current_dir(&self.root)
            .args([
                "github",
                action,
                "--config-root",
                self.config.to_str().unwrap(),
                "--inbox",
                self.inbox.to_str().unwrap(),
                "--installation-id",
                "56",
                "--json",
            ])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env_remove("WCODE_GITHUB_WEBHOOK_SECRET");
        cmd
    }
    fn status(&self) -> Value {
        let output = self.command("inbox-status").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!text.contains("PRIVATE-PAYLOAD"));
        assert!(!text.contains(KEY));
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn enqueue(&self, delivery: &str, valid: bool) -> Output {
        self.enqueue_key(delivery, valid, KEY)
    }
    fn enqueue_key(&self, delivery: &str, valid: bool, key: &str) -> Output {
        let body = serde_json::to_vec(&json!({"action":"synchronize", "number":7,
            "installation":{"id":56}, "repository":{"id":12,"full_name":"owner/repo"},
            "pull_request":{"number":7,"state":"open","draft":false,"merged":false,
                "base":{"sha":"a".repeat(40),"repo":{"id":12,"full_name":"owner/repo"}},
                "head":{"sha":"b".repeat(40),"repo":{"id":13,"full_name":"fork/repo"}},
                "body":"PRIVATE-PAYLOAD"}}))
        .unwrap();
        let tag = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes()),
            if valid { &body } else { b"modified" },
        );
        let signature = format!(
            "sha256={}",
            tag.as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut child = self
            .command("enqueue-event")
            .env("WCODE_GITHUB_WEBHOOK_SECRET", key)
            .env("WCODE_GITHUB_SIGNATURE_256", signature)
            .env("WCODE_GITHUB_EVENT", "pull_request")
            .env("WCODE_GITHUB_DELIVERY", delivery)
            .env("WCODE_GITHUB_CONTENT_TYPE", "application/json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&body).unwrap();
        child.wait_with_output().unwrap()
    }
}

#[test]
fn github_inbox_archive_cli_is_available_and_empty_or_readonly_rejects_without_writes() {
    let help = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["github", "archive-inbox", "--help"])
        .output()
        .unwrap();
    assert!(
        help.status.success(),
        "archive command is missing: {}",
        String::from_utf8_lossy(&help.stderr)
    );
    let fixture = Fixture::new();
    let destination = fixture.root.join("archive.json");
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let rejected = fixture
        .command("archive-inbox")
        .args([
            "--expected-generation",
            "1",
            "--output",
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("no terminal deliveries"));
    assert!(!destination.exists());
    let readonly = fixture
        .command("archive-inbox")
        .args([
            "--expected-generation",
            "1",
            "--output",
            destination.to_str().unwrap(),
            "--read-only",
        ])
        .output()
        .unwrap();
    assert!(!readonly.status.success());
    assert!(String::from_utf8_lossy(&readonly.stderr).contains("read-only"));
    assert!(!destination.exists());
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
}

#[test]
fn github_inbox_archive_cli_migrates_legacy_terminal_history_and_preserves_replay() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let delivery = "72d3162e-cc78-11e3-81ab-4c9367dc0958";
    assert!(fixture.enqueue(delivery, true).status.success());
    let metadata = fixture.status();
    // A schema-v1 terminal queue fixture, not a native execution/remote Check
    // attestation. Preserve legacy struct field ordering in its checksum.
    let stored = String::from_utf8(fs::read(fixture.inbox.join("inbox.json")).unwrap()).unwrap();
    let (state, _) = stored
        .strip_prefix("{\"state\":")
        .unwrap()
        .rsplit_once(",\"checksum\":")
        .unwrap();
    let previous = "\"state\":\"queued\",\"attempts\":0,\"retry_at_ms\":0";
    assert_eq!(state.matches(previous).count(), 1);
    let terminal = state.replacen(
        previous,
        &format!(
            "\"state\":\"completed\",\"attempts\":1,\"retry_at_ms\":{}",
            metadata["entries"][0]["updated_at_ms"].as_u64().unwrap(),
        ),
        1,
    );
    let legacy = format!(
        "{{\"state\":{terminal},\"checksum\":\"sha256:{:x}\"}}",
        Sha256::digest(terminal.as_bytes())
    );
    fs::write(fixture.inbox.join("inbox.json"), legacy).unwrap();
    assert_eq!(fixture.status()["entries"][0]["state"], "completed");
    let destination = fixture.root.join("archive.json");
    let generation = metadata["generation"].as_u64().unwrap().to_string();
    let output = fixture
        .command("archive-inbox")
        .args([
            "--expected-generation",
            &generation,
            "--output",
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pin: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(pin["archived_entries"], 1);
    assert_eq!(pin["current_acceptance"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-PAYLOAD"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(KEY));
    let compacted = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let exported = fs::read(&destination).unwrap();
    let info = fixture
        .command("inspect-inbox-archive")
        .args([
            "--archive",
            destination.to_str().unwrap(),
            "--digest",
            pin["archive_digest"].as_str().unwrap(),
            "--read-only",
            "--no-exec",
        ])
        .output()
        .unwrap();
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&info.stdout).unwrap()["restored"],
        false
    );
    let repeated = fixture.enqueue(delivery, true);
    assert!(repeated.status.success());
    let ack: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(ack["archived"], true);
    assert_eq!(ack["duplicate"], true);
    assert_eq!(fixture.status()["retained"], 0);
    assert_eq!(fixture.status()["archived_replay_tombstones"], 1);
    assert_eq!(
        compacted,
        fs::read(fixture.inbox.join("inbox.json")).unwrap()
    );
    assert_eq!(exported, fs::read(&destination).unwrap());
    let rejected = fixture
        .command("inspect-inbox-archive")
        .args([
            "--archive",
            destination.to_str().unwrap(),
            "--digest",
            "sha256:bad",
            "--read-only",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(
        compacted,
        fs::read(fixture.inbox.join("inbox.json")).unwrap()
    );
}

#[test]
fn github_inbox_cli_persists_and_deduplicates_across_processes() {
    let fixture = Fixture::new();
    let one = fixture.enqueue("72d3162e-cc78-11e3-81ab-4c9367dc0958", true);
    assert!(
        one.status.success(),
        "{}",
        String::from_utf8_lossy(&one.stderr)
    );
    let first: Value = serde_json::from_slice(&one.stdout).unwrap();
    assert_eq!(first["duplicate"], false);
    let two = fixture.enqueue("82d3162e-cc78-11e3-81ab-4c9367dc0958", true);
    assert!(two.status.success());
    let second: Value = serde_json::from_slice(&two.stdout).unwrap();
    assert_eq!(second["duplicate"], true);
    assert_eq!(first["generation"], second["generation"]);
    assert_eq!(fixture.status()["retained"], 1);
    assert_eq!(fixture.status()["current_acceptance"], false);
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    assert!(!fixture
        .enqueue("72d3162e-cc78-11e3-81ab-4c9367dc0958", false)
        .status
        .success());
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
    assert!(!fixture
        .command("inbox-init")
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn github_inbox_cli_rotated_redelivery_is_durable_without_duplicate_work() {
    let fixture = Fixture::new();
    let delivery = "72d3162e-cc78-11e3-81ab-4c9367dc0958";
    let initial = fixture.enqueue(delivery, true);
    assert!(initial.status.success());
    let before = fixture.status();
    let snapshot = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let rotated = "rotated-cli-test-key-not-live";
    let denied = fixture.enqueue_key(delivery, false, rotated);
    assert!(!denied.status.success());
    assert_eq!(
        snapshot,
        fs::read(fixture.inbox.join("inbox.json")).unwrap()
    );
    let updated = fixture.enqueue_key(delivery, true, rotated);
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let ack: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(ack["duplicate"], true);
    assert_eq!(ack["reauthenticated"], true);
    assert!(ack["generation"].as_u64().unwrap() > before["generation"].as_u64().unwrap());
    let after = fixture.status();
    assert_eq!(after["retained"], 1);
    assert_eq!(after["entries"][0]["state"], "queued");
    assert_eq!(after["entries"][0]["attempts"], 0);
    let stable = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let repeated = fixture.enqueue_key(delivery, true, rotated);
    assert!(repeated.status.success());
    let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated["reauthenticated"], false);
    assert_eq!(repeated["generation"], ack["generation"]);
    assert_eq!(stable, fs::read(fixture.inbox.join("inbox.json")).unwrap());
    assert!(!String::from_utf8_lossy(&updated.stdout).contains(rotated));
    assert!(!String::from_utf8_lossy(&updated.stdout).contains("PRIVATE-PAYLOAD"));
}

#[test]
fn github_inbox_cross_process_lock_rejects_writes_but_not_status_and_recovers() {
    let fixture = Fixture::new();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.inbox.join("inbox.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let output = fixture.enqueue("72d3162e-cc78-11e3-81ab-4c9367dc0958", true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("inbox busy"));
    assert_eq!(fixture.status()["retained"], 0);
    drop(lock);
    assert!(fixture
        .enqueue("72d3162e-cc78-11e3-81ab-4c9367dc0958", true)
        .status
        .success());
    assert_eq!(fixture.status()["retained"], 1);
}

#[test]
fn github_inbox_cli_restrictions_and_missing_state_do_not_reset_history() {
    let fixture = Fixture::new();
    for action in ["inbox-init", "enqueue-event", "publish-next"] {
        let mut command = fixture.command(action);
        command.arg("--read-only");
        if action == "publish-next" {
            command.args(["--workspace-id", "project"]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("read-only"));
    }
    fs::remove_file(fixture.inbox.join("inbox.json")).unwrap();
    assert!(!fixture
        .command("inbox-status")
        .output()
        .unwrap()
        .status
        .success());
    assert!(!fixture
        .command("inbox-init")
        .output()
        .unwrap()
        .status
        .success());
    assert!(!fixture.inbox.join("inbox.json").exists());
}

#[test]
fn github_inbox_receiver_denies_public_bind_and_readonly_before_starting() {
    let fixture = Fixture::new();
    let output = fixture
        .command("serve-inbox")
        .args(["--listen", "0.0.0.0:0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
    let output = fixture
        .command("serve-inbox")
        .args(["--listen", "127.0.0.1:0", "--read-only"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("read-only"));
    assert_eq!(fixture.status()["retained"], 0);
}

#[test]
fn github_inbox_worker_has_help_and_rejects_unsafe_start_before_secrets() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["github", "work-inbox", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("--workspace-id"));
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    for flag in ["--read-only", "--no-exec"] {
        let output = fixture
            .command("work-inbox")
            .args(["--workspace-id", "project", flag])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("disabled"));
    }
    let output = fixture
        .command("work-inbox")
        .args([
            "--workspace-id",
            "project",
            "-w",
            fixture.config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlap"));
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
}

#[cfg(unix)]
#[test]
fn github_inbox_worker_real_process_stops_on_signals_without_mutating_idle_history() {
    use std::io::{BufRead, BufReader, Read};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let fixture = Fixture::new();
        let candidate = fixture.root.join("candidate");
        fs::create_dir(&candidate).unwrap();
        let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
        let mut child = ChildGuard(
            fixture
                .command("work-inbox")
                .args([
                    "--workspace-id",
                    "project",
                    "-w",
                    candidate.to_str().unwrap(),
                ])
                .env("WCODE_GITHUB_WEBHOOK_SECRET", KEY)
                .env(
                    "WCODE_GITHUB_PUBLISHER_TOKEN",
                    "NOT-LIVE-WORKER-TEST-CREDENTIAL",
                )
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let output = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut startup = String::new();
            let _ = reader.read_line(&mut startup);
            let _ = send.send(startup);
            let mut tail = String::new();
            reader.read_to_string(&mut tail).unwrap();
            tail
        });
        let startup = receive.recv_timeout(Duration::from_secs(10)).unwrap();
        let startup: Value = serde_json::from_str(&startup).unwrap();
        assert_eq!(startup["event"], "worker_started");
        assert_eq!(startup["current_acceptance"], false);
        // Only this isolated test child receives the signal.
        assert_eq!(
            unsafe { libc::kill(child.0.id() as libc::pid_t, signal) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "worker did not drain on signal {signal}: {status}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "worker shutdown exceeded deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let tail = output.join().unwrap();
        let events = tail
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 1, "idle worker produced extra output: {tail}");
        assert_eq!(events[0]["event"], "worker_stopped");
        assert_eq!(events[0]["summary"]["publications"], 0);
        assert_eq!(events[0]["summary"]["unavailable_polls"], 0);
        assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
        assert_eq!(fixture.status()["retained"], 0);
        assert!(!tail.contains(KEY));
        assert!(!tail.contains("NOT-LIVE"));
    }
}

#[test]
fn github_inbox_worker_rejects_bad_key_before_starting_or_mutating_history() {
    let fixture = Fixture::new();
    let candidate = fixture.root.join("candidate");
    fs::create_dir(&candidate).unwrap();
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let output = fixture
        .command("work-inbox")
        .args([
            "--workspace-id",
            "project",
            "-w",
            candidate.to_str().unwrap(),
        ])
        .env("WCODE_GITHUB_WEBHOOK_SECRET", "short-key")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("verification key is invalid"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("short-key"));
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
}

#[test]
fn github_inbox_app_credentials_do_not_bypass_safety_or_rewrite_routing() {
    let fixture = Fixture::new();
    let candidate = fixture.root.join("candidate");
    fs::create_dir(&candidate).unwrap();
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    for action in ["publish-next", "work-inbox"] {
        for restricted in [false, true] {
            let mut command = fixture.command(action);
            command
                .args([
                    "--workspace-id",
                    "fixture",
                    "-w",
                    candidate.to_str().unwrap(),
                ])
                .env("WCODE_GITHUB_WEBHOOK_SECRET", KEY)
                .env("WCODE_GITHUB_INSTALLATION_ID", "57")
                .env(
                    "WCODE_GITHUB_APP_KEY_FILE",
                    fixture.config.join("never-read.pem"),
                );
            if restricted {
                command.arg("--read-only");
            }
            let output = command.output().unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(if restricted {
                    "disabled"
                } else {
                    "installation differs"
                }),
                "{error}"
            );
            assert!(!error.contains(KEY));
        }
    }
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
}

#[test]
fn github_inbox_status_ignores_unusable_app_credentials_without_exposing_them() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.inbox.join("inbox.json")).unwrap();
    let output = fixture
        .command("inbox-status")
        .env(
            "WCODE_GITHUB_APP_KEY_FILE",
            fixture.inbox.join("never-read.pem"),
        )
        .env("WCODE_GITHUB_INSTALLATION_ID", "invalid")
        .env("WCODE_GITHUB_PUBLISHER_TOKEN", "PRIVATE-INVALID-TOKEN\n")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["retained"], 0);
    assert_eq!(status["current_acceptance"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-INVALID"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-INVALID"));
    assert_eq!(before, fs::read(fixture.inbox.join("inbox.json")).unwrap());
    assert!(!fixture.inbox.join("never-read.pem").exists());
}

#[test]
fn github_inbox_commands_are_available_without_starting_a_service() {
    let root = tempfile::tempdir().unwrap();
    for command in [
        "inbox-init",
        "inbox-status",
        "serve-inbox",
        "enqueue-event",
        "publish-next",
        "work-inbox",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["github", command, "--help"])
            .env_remove("WCODE_GITHUB_PUBLISHER_TOKEN")
            .env_remove("WCODE_GITHUB_WEBHOOK_SECRET")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--inbox"));
        assert!(help.contains("--installation-id"));
        assert!(!help.contains("--secret"));
        assert!(!help.contains("--token"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
