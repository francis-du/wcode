use super::*;

#[cfg(unix)]
#[tokio::test]
async fn run_command_sandbox_argument_selects_the_opt_in_lane() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    let sandboxed = crate::workspace::execution_sandbox_status().available;
    let opt_in = if sandboxed {
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

    let default = if sandboxed {
        "if test \"$WCODE_SANDBOX\" = 1; then exit 41; fi"
    } else {
        "exit 0"
    };
    let response = call_tool(
        &state,
        json!({
            "name": "run_command",
            "arguments": {"program": "sh", "args": ["-c", default]}
        }),
    )
    .await
    .expect("default run_command must be accepted");
    assert_eq!(response["isError"], false, "{response}");
    assert_eq!(response["structuredContent"]["success"], true, "{response}");
}
