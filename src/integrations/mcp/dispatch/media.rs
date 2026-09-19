use super::super::mcp_tools::acquire_tool_permit;
use super::*;

pub(super) async fn read_media_tool(state: &AppState, params: &Value) -> Result<Value, String> {
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let workspace_label = string_arg(&args, "workspace")
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let request_bytes = serialized_size(&args) as u64;
    let task = state.monitor.queue(
        workspace_label,
        "read_media",
        task_detail("read_media", &args),
        request_bytes,
    );
    let permit = acquire_tool_permit(state, false).await?;
    task.start();

    let (workspace_id, workspace) = match selected_workspace(state, &args) {
        Ok(selected) => selected,
        Err(error) => {
            task.finish(false, error.len() as u64);
            return Ok(tool_result(json!({"error": error}), true));
        }
    };
    let path = match required_string(&args, "path") {
        Ok(path) => path.to_owned(),
        Err(error) => {
            task.finish(false, error.len() as u64);
            return Ok(tool_result(json!({"error": error}), true));
        }
    };
    let include_content = args
        .get("include_content")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read = super::mcp_tools::BLOCKING_TASK
        .scope(
            task.clone(),
            super::mcp_tools::BLOCKING_PERMIT
                .scope(permit, run_blocking(move || workspace.read_media(&path))),
        )
        .await;
    let view = match read {
        Ok(view) => view,
        Err(error) => {
            let response = tool_result(json!({"error": error.to_string()}), true);
            task.finish(false, serialized_size(&response) as u64);
            return Ok(response);
        }
    };

    let mut metadata = view.metadata();
    metadata["workspace"] = json!(workspace_id);
    metadata["content_available"] = json!(matches!(view.kind, "image" | "audio"));
    metadata["content_requested"] = json!(include_content);
    metadata["content_returned"] = json!(false);

    if !include_content {
        let response = tool_result(metadata, false);
        task.finish(true, serialized_size(&response) as u64);
        return Ok(response);
    }
    if !matches!(view.kind, "image" | "audio") {
        metadata["error_code"] = json!("media_content_type_not_supported");
        metadata["error"] = json!(
            "MCP tool results do not expose a standard video content block; video is metadata-only"
        );
        let response = tool_result(metadata, true);
        task.finish(false, serialized_size(&response) as u64);
        return Ok(response);
    }
    metadata["content_returned"] = json!(true);
    let encoded = STANDARD.encode(&view.data);
    let text = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_owned());
    let response = json!({
        "content": [
            {"type": "text", "text": text},
            {"type": view.kind, "data": encoded, "mimeType": view.mime_type}
        ],
        "structuredContent": metadata,
        "isError": false,
    });
    task.finish(true, serialized_size(&response) as u64);
    Ok(response)
}
