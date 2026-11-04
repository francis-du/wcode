use super::*;

async fn sandbox_marker(state: &AppState) -> bool {
    let response = call_tool(
        state,
        json!({
            "name": "run_command",
            "arguments": {
                "program": "sh",
                "args": ["-c", "printf %s \"$WCODE_SANDBOX\""],
                "sandbox": true
            }
        }),
    )
    .await
    .expect("sandbox probe must be accepted");
    assert_eq!(response["isError"], false, "{response}");
    assert_eq!(response["structuredContent"]["success"], true, "{response}");
    response["structuredContent"]["stdout"].as_str() == Some("1")
}

#[cfg(unix)]
#[tokio::test]
async fn run_command_sandbox_argument_selects_the_opt_in_lane() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    // The backend binary can exist while the platform still refuses to apply
    // it (for example an uninitialised authority root). Calibrate on the real
    // behaviour instead of assuming availability.
    let sandbox_applied = sandbox_marker(&state).await;
    let opt_in = if sandbox_applied {
        "test \"$WCODE_SANDBOX\" = 1 || exit 41"
    } else {
        "if test \"$WCODE_SANDBOX\" = 1; then exit 41; fi"
    };
    let response = call_tool(
        &state,
        json!({
            "name": "run_command",
            "arguments": {"program": "sh", "args": ["-c", opt_in], "sandbox": true}
        }),
    )
    .await
    .expect("sandbox=true must be accepted");
    assert_eq!(response["isError"], false, "{response}");
    assert_eq!(response["structuredContent"]["success"], true, "{response}");

    // The default lane never selects the sandbox, even when one is available.
    let escape = root
        .path()
        .parent()
        .unwrap()
        .join("wcode-mcp-sandbox-escape.txt");
    let _ = std::fs::remove_file(&escape);
    let response = call_tool(
        &state,
        json!({
            "name": "run_command",
            "arguments": {
                "program": "sh",
                "args": ["-c", "printf escape > ../wcode-mcp-sandbox-escape.txt"]
            }
        }),
    )
    .await
    .expect("default run_command must be accepted");
    assert_eq!(response["isError"], false, "{response}");
    assert_eq!(response["structuredContent"]["success"], true, "{response}");
    assert!(
        escape.exists(),
        "default run_command must stay on the direct lane"
    );
}
