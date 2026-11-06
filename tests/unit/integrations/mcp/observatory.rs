use super::*;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;

fn fixture(root: &std::path::Path) -> Arc<AppState> {
    let workspaces = Workspaces::new([root], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let monitor = TaskMonitor::new([workspace_id]);
    // The runtime serves HTTP, so the Observatory address must be offered.
    let auth = Arc::new(
        AuthState::new_with_monitor(
            "http://127.0.0.1:8765".into(),
            monitor.clone(),
            &[root.to_path_buf()],
            None,
        )
        .unwrap(),
    );
    Arc::new(AppState {
        auth,
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor,
        tasks: TaskRuntime::default(),
    })
}

fn request_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": crate::mcp::MODERN_PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

async fn call(state: Arc<AppState>, message: Value, owner: &str) -> Value {
    crate::mcp::handle_message_isolated(state, message, crate::mcp::MODERN_PROTOCOL_VERSION, owner)
        .await
        .unwrap()
}

#[tokio::test]
async fn observatory_ui_resource_and_tool_are_discoverable_and_authenticated() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);

    let listed = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":1,"method":"resources/list","params":{"_meta":request_meta()}}),
        &owner,
    )
    .await;
    let resources = listed["result"]["resources"].as_array().unwrap();
    let observatory = resources
        .iter()
        .find(|resource| resource["uri"] == "ui://wcode/observatory")
        .expect("the Observatory UI resource must be listed");
    assert_eq!(observatory["mimeType"], "text/html");

    let read = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"ui://wcode/observatory","_meta":request_meta()}}),
        &owner,
    )
    .await;
    let html = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains("/intelligence#token="),
        "the UI resource must frame the authenticated Observatory: {html}"
    );
    assert!(
        html.contains("view=changes"),
        "the workbench pane must open on the current changes: {html}"
    );

    let tools = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":request_meta()}}),
        &owner,
    )
    .await;
    let tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "open_observatory")
        .expect("open_observatory must be in the catalog");
    assert_eq!(
        tool["_meta"]["ui"]["resourceUri"], "ui://wcode/observatory",
        "clients render this registered UI resource"
    );
    let entrypoints = tool["_meta"]["openai/ui"]["entrypoints"]
        .as_array()
        .expect("OpenAI UI entrypoints must be declared");
    assert!(
        entrypoints.iter().any(|entry| entry["type"] == "thread"),
        "the conversation side panel is the third pane: {entrypoints:?}"
    );

    let opened = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"open_observatory","arguments":{},"_meta":request_meta()}}),
        &owner,
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    let url = opened["result"]["structuredContent"]["url"]
        .as_str()
        .unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:8765/intelligence#token="),
        "the returned address must be the authenticated local Observatory: {url}"
    );
    assert!(
        url.contains("view=changes"),
        "the address must open on the current changes: {url}"
    );
}

#[tokio::test]
async fn plain_stdio_does_not_invent_an_observatory_address() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    // Plain stdio has no listener, so it must say so instead of handing out an
    // address that cannot load.
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1".to_owned())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let owner = "a".repeat(64);
    let opened = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"open_observatory","arguments":{},"_meta":request_meta()}}),
        &owner,
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    assert_eq!(
        opened["result"]["structuredContent"]["served_by_this_process"],
        false
    );
    assert!(opened["result"]["structuredContent"]["url"].is_null());

    let read = call(
        state.clone(),
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"ui://wcode/observatory","_meta":request_meta()}}),
        &owner,
    )
    .await;
    let html = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains("unavailable"),
        "an unserved Observatory must not frame an address: {html}"
    );
}
