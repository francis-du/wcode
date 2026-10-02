use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn menu_bar_json(state_root: &std::path::Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["menu-bar", "--json"])
        .env("WCODE_STATE_DIR", state_root)
        .output()
        .expect("menu-bar status command must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("menu-bar status must be JSON")
}

#[test]
fn real_stdio_runtime_is_visible_to_menu_bar_and_cleans_up_on_eof() {
    let project = tempfile::tempdir().unwrap();
    let state_root = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("README.md"),
        "# stdio presence fixture\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["--no-menu-bar", "mcp-stdio"])
        .current_dir(project.path())
        .env("WCODE_STATE_DIR", state_root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real stdio runtime must start");

    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{}}
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("stdio runtime did not answer initialize")
        .expect("cannot read stdio initialize response");
    let response: Value = serde_json::from_str(&line).expect("stdio response must be JSON");
    assert_eq!(response["id"], 1);
    assert!(response.get("result").is_some(), "{response}");

    let status = menu_bar_json(state_root.path());
    assert_eq!(status["kind"], "menu_bar_status");
    assert_eq!(status["summary"]["stdio_runtimes"], 1);
    assert_eq!(status["summary"]["http_runtimes"], 0);
    assert_eq!(status["summary"]["mcp_connected"], 1);
    assert_eq!(status["summary"]["partial"], false);
    let records = status["presence"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["transport"], "stdio");
    assert_eq!(records[0]["mcp_connected"], true);
    assert!(records[0].get("local_url").is_none());
    let serialized = status.to_string();
    for forbidden in [
        "ui_token",
        "oauth_token",
        "owner",
        "arguments",
        "source_path",
    ] {
        assert!(!serialized.contains(forbidden), "{forbidden} leaked");
    }

    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(exit) = child.try_wait().unwrap() {
            assert!(
                exit.success(),
                "stdio runtime exited unsuccessfully: {exit}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stdio runtime did not exit after EOF"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let after = menu_bar_json(state_root.path());
    assert_eq!(after["summary"]["runtime_count"], 0);
    assert_eq!(after["summary"]["state"], "offline");
}

#[test]
fn real_http_runtime_is_visible_to_menu_bar_without_tunnel_or_secret_state() {
    let project = tempfile::tempdir().unwrap();
    let state_root = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("README.md"),
        "# HTTP presence fixture\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args([
            "--no-tunnel",
            "--no-monitor",
            "--no-menu-bar",
            "--no-install",
            "--allow-sleep",
            "--port",
            "0",
        ])
        .current_dir(project.path())
        .env("WCODE_STATE_DIR", state_root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real HTTP runtime must start");

    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(exit) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            if let Some(pipe) = child.stderr.take() {
                let _ = BufReader::new(pipe).read_to_string(&mut stderr);
            }
            panic!("HTTP runtime exited before publishing presence: {exit}: {stderr}");
        }
        let status = menu_bar_json(state_root.path());
        if status["summary"]["http_runtimes"] == 1 {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "HTTP runtime did not publish menu bar presence"
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    assert_eq!(status["summary"]["stdio_runtimes"], 0);
    assert_eq!(status["summary"]["mcp_connected"], 0);
    assert_eq!(status["summary"]["partial"], false);
    let records = status["presence"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["transport"], "http");
    assert_eq!(records[0]["mcp_connected"], false);
    let local_url = records[0]["local_url"].as_str().unwrap();
    assert!(local_url.starts_with("http://127.0.0.1:"));
    assert!(!local_url.contains('#'));
    assert!(!local_url.contains('?'));

    let serialized = status.to_string();
    for forbidden in [
        "ui_token",
        "oauth_token",
        "owner",
        "arguments",
        "source_path",
        "password",
    ] {
        assert!(!serialized.contains(forbidden), "{forbidden} leaked");
    }

    child
        .kill()
        .expect("isolated HTTP fixture must be stoppable");
    let _ = child.wait().expect("isolated HTTP fixture must terminate");
}

#[cfg(target_os = "macos")]
#[test]
fn native_menu_bar_process_starts_as_a_live_accessory_and_is_cleanup_safe() {
    let state_root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .arg("menu-bar")
        .env("WCODE_STATE_DIR", state_root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native menu bar process must start");

    std::thread::sleep(Duration::from_secs(2));
    if let Some(exit) = child.try_wait().unwrap() {
        let mut stderr = String::new();
        if let Some(pipe) = child.stderr.take() {
            let _ = BufReader::new(pipe).read_to_string(&mut stderr);
        }
        panic!("native menu bar exited during startup: {exit}: {stderr}");
    }

    child
        .kill()
        .expect("native menu bar smoke process must be cleanup safe");
    let _ = child
        .wait()
        .expect("native menu bar smoke process must terminate");
}
