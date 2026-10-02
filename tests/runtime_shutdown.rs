#![cfg(unix)]

use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct RuntimeChild(Child);

impl Drop for RuntimeChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn shutdown_after_presence(signal: i32, blocked_endpoint: bool) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let stderr = state.path().join("stderr.log");
    let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
    command
        .current_dir(root.path())
        .env("WCODE_STATE_DIR", state.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--no-monitor",
            "--no-semantic",
            "--no-exec",
            "--read-only",
            "--allow-sleep",
            "--no-menu-bar",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    // Hold a local listener without accepting TLS. Readiness cannot complete,
    // and this fixture never contacts a remote service or starts a tunnel.
    let endpoint = blocked_endpoint.then(|| std::net::TcpListener::bind("127.0.0.1:0").unwrap());
    if let Some(endpoint) = endpoint.as_ref() {
        command.args([
            "--public-url",
            &format!("https://{}", endpoint.local_addr().unwrap()),
        ]);
    } else {
        command.arg("--no-tunnel");
    }
    let mut child = RuntimeChild(command.spawn().expect("HTTP runtime must start"));
    let presence = state.path().join("runtime-presence/v1");
    let diagnostics = || std::fs::read_to_string(&stderr).unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let published = std::fs::read_dir(&presence).ok().is_some_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                std::fs::read(entry.path())
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .is_some_and(|value| value["transport"] == "http")
            })
        });
        if published {
            break;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "runtime exited before readiness: {}",
            diagnostics()
        );
        assert!(
            Instant::now() < deadline,
            "runtime did not publish readiness: {}",
            diagnostics()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, signal) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "signal {signal} did not stop runtime (blocked_endpoint={blocked_endpoint}): {}",
            diagnostics()
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        status.success(),
        "signal {signal} bypassed clean shutdown: {status}; {}",
        diagnostics()
    );
    assert!(
        !std::fs::read_dir(&presence)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")),
        "shutdown retained a live runtime record"
    );
}

#[test]
fn http_shutdown_immediately_after_presence_cleans_up() {
    for _ in 0..3 {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            shutdown_after_presence(signal, false);
        }
    }
}

#[test]
fn http_shutdown_during_public_endpoint_startup_cleans_up() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        shutdown_after_presence(signal, true);
    }
}
