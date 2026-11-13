use super::*;
use crate::scopes;
use std::sync::OnceLock;

#[path = "tool_catalog.rs"]
mod tool_catalog;
#[path = "tools_telemetry.rs"]
mod tools_telemetry;
use tools_telemetry::take_agent_context_telemetry;
pub(super) use tools_telemetry::{batch_validation_error, task_detail};

pub(super) fn tools() -> &'static [Value] {
    static CATALOG: OnceLock<Vec<Value>> = OnceLock::new();
    CATALOG.get_or_init(tool_catalog::build_tools).as_slice()
}

pub(super) fn catalog_metrics() -> &'static Value {
    static METRICS: OnceLock<Value> = OnceLock::new();
    METRICS.get_or_init(|| {
        let catalog = tools();
        let core_tool_count = catalog
            .iter()
            .filter(|tool| tool["_meta"]["dev.wcode/preloadRecommended"] == true)
            .count();
        let input_schema_bytes = catalog
            .iter()
            .map(|tool| serde_json::to_vec(&tool["inputSchema"]).map_or(0, |bytes| bytes.len()))
            .sum::<usize>();
        let core = catalog
            .iter()
            .filter(|tool| tool["_meta"]["dev.wcode/preloadRecommended"] == true)
            .collect::<Vec<_>>();
        let catalog_bytes = serde_json::to_vec(catalog).map_or(0, |bytes| bytes.len());
        let core_catalog_bytes = serde_json::to_vec(&core).map_or(0, |bytes| bytes.len());
        let core_schema_bytes = core
            .iter()
            .map(|tool| serde_json::to_vec(&tool["inputSchema"]).map_or(0, |bytes| bytes.len()))
            .sum::<usize>();
        json!({
            "tool_count": catalog.len(),
            "core_tool_count": core_tool_count,
            "on_demand_tool_count": catalog.len().saturating_sub(core_tool_count),
            "catalog_bytes": catalog_bytes,
            "input_schema_bytes": input_schema_bytes,
            "preload_catalog_bytes": core_catalog_bytes,
            "preload_input_schema_bytes": core_schema_bytes,
            "preload_catalog_reduction_percent": core_catalog_bytes
                .saturating_mul(100)
                .checked_div(catalog_bytes)
                .map(|used| 100usize.saturating_sub(used))
                .unwrap_or(0),
            "task_manifest": "agent_context.capabilities",
            "action_groups": "task_manifest_only",
            "dynamic_tool_list": false,
            "dynamic_tool_list_policy": "task_independent_protocol_catalog",
        })
    })
}

fn schema(mut properties: Value, required: &[&str]) -> Value {
    if let Some(properties) = properties.as_object_mut() {
        properties.insert(
            "workspace".to_owned(),
            json!({
                "type": "string",
                "description": "Omit for default."
            }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(super) fn workspace_arg(args: &Value) -> Result<Option<&str>, String> {
    let args = args.as_object().ok_or("arguments must be an object")?;
    match args.get("workspace") {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        Some(_) => Err("workspace must be a non-empty string when provided".to_owned()),
    }
}

pub(crate) fn selected_workspace(
    state: &AppState,
    args: &Value,
) -> Result<(String, Workspace), String> {
    state
        .workspaces
        .select(workspace_arg(args)?)
        .map_err(|error| error.to_string())
}

tokio::task_local! {
    pub(super) static BLOCKING_PERMIT: std::sync::Arc<crate::harness::ToolPermit>;
    pub(super) static BLOCKING_TASK: crate::monitor::TaskTicket;
}

pub(super) async fn acquire_status_permit(
    state: &AppState,
) -> Result<std::sync::Arc<crate::harness::ToolPermit>, Value> {
    state
        .harness
        .acquire_tool_with_wait_timeout(false, std::time::Duration::from_millis(250))
        .await
        .map(std::sync::Arc::new)
        .map_err(|_| state.harness.admission_diagnostic())
}

pub(super) async fn acquire_tool_permit(
    state: &AppState,
    executes_process: bool,
) -> Result<std::sync::Arc<crate::harness::ToolPermit>, String> {
    state
        .harness
        .acquire_tool_with_wait_timeout(executes_process, crate::harness::TOOL_SLOT_WAIT_CAP)
        .await
        .map(std::sync::Arc::new)
}

pub(super) async fn run_blocking<F, T>(work: F) -> AnyResult<T>
where
    F: FnOnce() -> AnyResult<T> + Send + 'static,
    T: Send + 'static,
{
    // A started blocking worker outlives cancellation of its async caller.
    // Keep its real slot until it finishes; JoinSet aborts work still queued.
    let permit = BLOCKING_PERMIT.try_with(std::sync::Arc::clone).ok();
    let task = BLOCKING_TASK.try_with(Clone::clone).ok();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn_blocking(move || {
        // A cancelled caller may disappear before synchronous work can stop.
        // Keep both real capacity and its visible monitor lifetime attached to
        // the worker until the blocking function actually returns or panics.
        let _permit = permit;
        let _task = task;
        work()
    });
    tasks
        .join_next()
        .await
        .expect("one blocking worker was scheduled")
        .map_err(|error| anyhow!("blocking task failed: {error}"))?
}

const MAX_TOOL_DESCRIPTION_CHARS: usize = 200;
const MAX_ON_DEMAND_TOOL_DESCRIPTION_CHARS: usize = 140;

pub(super) fn compact_tool_description(name: &str, description: &str) -> String {
    let limit = if crate::harness::model_tool_preload_recommended(name) {
        MAX_TOOL_DESCRIPTION_CHARS
    } else {
        MAX_ON_DEMAND_TOOL_DESCRIPTION_CHARS
    };
    if description.chars().count() <= limit {
        return description.to_owned();
    }
    let mut compact = description
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    compact = compact.trim_end().to_owned();
    compact.push('…');
    compact
}

fn strip_schema_defaults(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("default");
            for child in object.values_mut() {
                strip_schema_defaults(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_schema_defaults(child);
            }
        }
        _ => {}
    }
}

const MODEL_HIDDEN_TUNING_ARGS: &[&str] = &[
    "timeout_seconds",
    "budget",
    "limit",
    "max_results",
    "max_files",
    "max_symbols",
    "max_entries",
    "max_body_lines",
];

fn strip_model_tuning_args(value: &mut Value, expose_timeout: bool) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
                for key in MODEL_HIDDEN_TUNING_ARGS {
                    if *key != "timeout_seconds" || !expose_timeout {
                        properties.remove(*key);
                    }
                }
            }
            for child in object.values_mut() {
                strip_model_tuning_args(child, expose_timeout);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_model_tuning_args(child, expose_timeout);
            }
        }
        _ => {}
    }
}

fn bug_scan_schema() -> Value {
    let mut value = search_schema("patterns");
    value["required"] = json!([]);
    value["properties"]["include_comments"] = json!({"type":"boolean","default":false});
    value["properties"]["preset"] = json!({"type":"string","enum":["go_common_bugs"]});
    value["properties"]["pattern"] = json!({
        "type":"string",
        "enum":["nil_deref","err_swallowed","index_mismatch","empty_test","unguarded_subscript"]
    });
    value
}

fn search_schema(key: &str) -> Value {
    let mut properties = json!({
        "path":{"type":"string"},
        "mode":{"type":"string","enum":["exact","regex","tokens_all","tokens_any"]},
        "context_lines":{"type":"integer","minimum":0,"maximum":20},
        "max_results":{"type":"integer","minimum":1,"maximum":2000},
        "auto_page":{"type":"boolean","default":false},
        "offset":{"type":"integer","minimum":0,"maximum":10000},
        "output_mode":{"type":"string","enum":["content","files_with_matches","count_matches"]}
    });
    properties[key] = json!({"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}});
    if key == "query" {
        properties[key]["type"] = json!(["string", "array"]);
        properties["mode"]["enum"] = json!(["auto", "exact", "regex", "tokens_all", "tokens_any"]);
    }
    schema(properties, &[key])
}

fn tool(
    name: &str,
    description: &str,
    mut input_schema: Value,
    read_only: bool,
    destructive: bool,
) -> Value {
    strip_schema_defaults(&mut input_schema);
    // Runtime allowance is an operational control, not retrieval tuning.
    // Keep it discoverable on long-command entry points; parser bounds apply.
    let expose_timeout = matches!(name, "run_command" | "command_task" | "verify_project");
    strip_model_tuning_args(&mut input_schema, expose_timeout);
    let product_scopes = scopes::tool_scopes(name)
        .into_iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>();
    let mut meta = json!({"dev.wcode/productScopes": product_scopes});
    if crate::harness::model_tool_preload_recommended(name) {
        meta["dev.wcode/preloadRecommended"] = Value::Bool(true);
    }
    // OpenAI clients render a registered ui:// resource beside the conversation
    // (thread entrypoint) or from the sidebar (global entrypoint). Other clients
    // still receive the address as text.
    if name == "open_observatory" {
        meta["ui"] = json!({"resourceUri": "ui://wcode/observatory"});
        meta["openai/ui"] = json!({"entrypoints": [{"type": "thread"}, {"type": "global"}]});
    }
    json!({
        "name": name,
        "description": compact_tool_description(name, description),
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
        "_meta": meta
    })
}

pub(super) fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    tool_result_with_text(value, is_error, text)
}

pub(super) fn tool_result_with_text(value: Value, is_error: bool, text: String) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": value,
        "isError": is_error,
    })
}

pub(super) fn structured_tool_result(value: Value, is_error: bool) -> Value {
    // Internal fan-out children never cross the MCP protocol boundary. Avoid
    // serializing the same potentially large structured payload into a second
    // text copy that the parent immediately discards.
    json!({
        "structuredContent": value,
        "isError": is_error,
    })
}

fn serialized_json_bytes(value: &Value) -> u64 {
    struct ByteCounter(u64);

    impl std::io::Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len() as u64);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter(0);
    if serde_json::to_writer(&mut counter, value).is_ok() {
        counter.0
    } else {
        0
    }
}

pub(super) fn agent_context_tool_result(value: Value, is_error: bool) -> Value {
    agent_context_result(value, is_error, true)
}

pub(super) fn agent_context_structured_result(value: Value, is_error: bool) -> Value {
    agent_context_result(value, is_error, false)
}

fn agent_context_result(mut value: Value, is_error: bool, include_text: bool) -> Value {
    let mut telemetry = take_agent_context_telemetry(&mut value);
    if let Some(selection) = capability_selection_telemetry(&value) {
        telemetry.insert("capability_selection".to_owned(), selection);
    }
    let serialized_text =
        include_text.then(|| serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned()));
    let model_bytes = serialized_text
        .as_ref()
        .map_or_else(|| serialized_json_bytes(&value), |text| text.len() as u64);
    let model_tokens = model_bytes.div_ceil(4);
    telemetry.insert("model_serialized_bytes".to_owned(), json!(model_bytes));
    telemetry.insert("model_estimated_tokens".to_owned(), json!(model_tokens));
    if let Some(budget_tokens) = value.get("budget").and_then(Value::as_u64) {
        let utilization_percent = if budget_tokens == 0 {
            0.0
        } else {
            ((model_tokens as f64 / budget_tokens as f64) * 10_000.0).round() / 100.0
        };
        telemetry.insert("budget_tokens".to_owned(), json!(budget_tokens));
        telemetry.insert(
            "budget_utilization_percent".to_owned(),
            json!(utilization_percent.min(100.0)),
        );
    }
    if let Some(baseline) = telemetry
        .get("baseline_context_bytes")
        .and_then(Value::as_u64)
    {
        let avoided = baseline.saturating_sub(model_bytes);
        let reduction_percent = if baseline == 0 {
            0.0
        } else {
            ((avoided as f64 / baseline as f64) * 10_000.0).round() / 100.0
        };
        telemetry.insert("context_bytes_avoided".to_owned(), json!(avoided));
        telemetry.insert(
            "context_reduction_percent".to_owned(),
            json!(reduction_percent),
        );
    }

    let mut result = if let Some(text) = serialized_text {
        tool_result_with_text(value, is_error, text)
    } else {
        structured_tool_result(value, is_error)
    };
    if let Some(result) = result.as_object_mut() {
        result.insert(
            "_meta".to_owned(),
            json!({"dev.wcode/agentContextTelemetry": telemetry}),
        );
    }
    result
}

fn capability_selection_telemetry(value: &Value) -> Option<Value> {
    let requested = value
        .pointer("/capabilities/recommended_tools")
        .and_then(Value::as_array)?;
    let catalog = tools();
    let selected = requested
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|name| catalog.iter().find(|tool| tool["name"] == name))
        .collect::<Vec<_>>();
    let selected_catalog_bytes =
        serde_json::to_vec(&selected).map_or(0, |bytes| bytes.len()) as u64;
    let selected_schema_bytes = selected
        .iter()
        .map(|tool| serialized_json_bytes(&tool["inputSchema"]))
        .sum::<u64>();
    let metrics = catalog_metrics();
    let catalog_bytes = metrics["catalog_bytes"].as_u64().unwrap_or(0);
    let schema_bytes = metrics["input_schema_bytes"].as_u64().unwrap_or(0);
    let reduction = |selected: u64, full: u64| {
        selected
            .saturating_mul(100)
            .checked_div(full)
            .map(|used| 100u64.saturating_sub(used))
            .unwrap_or(0)
    };
    Some(json!({
        "requested_tool_count": requested.len(),
        "resolved_tool_count": selected.len(),
        "catalog_tool_count": catalog.len(),
        "selected_catalog_bytes": selected_catalog_bytes,
        "selected_input_schema_bytes": selected_schema_bytes,
        "catalog_reduction_percent": reduction(selected_catalog_bytes, catalog_bytes),
        "schema_reduction_percent": reduction(selected_schema_bytes, schema_bytes),
        "compacted": value.pointer("/capabilities/compacted").and_then(Value::as_bool).unwrap_or(false),
    }))
}

fn array_len(args: &Value, key: &str) -> usize {
    args.get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn first_array_item<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
}

fn command_preview(args: &Value) -> String {
    let program = string_arg(args, "program").unwrap_or("command");
    let command_args = args
        .get("args")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut parts = vec![program.to_owned()];
    let mut redact_next = false;

    for value in command_args.iter().take(6) {
        let Some(argument) = value.as_str() else {
            continue;
        };
        if redact_next {
            parts.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        let lower = argument.to_ascii_lowercase();
        let sensitive = ["token", "secret", "password", "passwd", "api-key", "apikey"]
            .iter()
            .any(|needle| lower.contains(needle));
        if sensitive {
            if let Some((key, _)) = argument.split_once('=') {
                parts.push(format!("{key}=[REDACTED]"));
            } else {
                parts.push(argument.to_owned());
                redact_next = argument.starts_with('-');
            }
        } else {
            parts.push(argument.to_owned());
        }
    }
    if command_args.len() > 6 {
        parts.push(format!("…+{}", command_args.len() - 6));
    }
    format!(
        "{} · cwd {}",
        parts.join(" "),
        string_arg(args, "cwd").unwrap_or(".")
    )
}

pub(super) fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(super) fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    string_arg(args, key).ok_or_else(|| format!("missing string argument: {key}"))
}

pub(super) fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

/// Validate an optional integer argument against its declared schema minimum.
/// Values above `max` keep their existing downstream bound or clamp behavior.
pub(super) fn bounded_usize_arg(
    args: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value >= min)
        .ok_or_else(|| format!("{key} must be an integer between {min} and {max} when provided"))
}

/// Validate an optional integer argument against its declared schema minimum and
/// maximum. Values outside the declared range are rejected instead of clamped.
pub(super) fn bounded_usize_arg_range(
    args: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| format!("{key} must be an integer between {min} and {max} when provided"))
}

/// Validate an optional integer argument against its declared schema minimum,
/// preserving absence. Schemas without a declared maximum keep downstream bounds.
pub(super) fn optional_bounded_usize_arg(
    args: &Value,
    key: &str,
    min: usize,
) -> Result<Option<usize>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value >= min)
        .map(Some)
        .ok_or_else(|| format!("{key} must be an integer >= {min} when provided"))
}

pub(super) fn optional_string_array_arg(
    args: &Value,
    key: &str,
    max_items: usize,
) -> Result<Vec<String>, String> {
    let Some(values) = args.get(key) else {
        return Ok(Vec::new());
    };
    let values = values
        .as_array()
        .ok_or_else(|| format!("{key} must be an array"))?;
    if values.len() > max_items {
        return Err(format!("{key} must contain at most {max_items} items"));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain non-empty strings"))
        })
        .collect()
}

pub(super) fn string_array_arg(
    args: &Value,
    key: &str,
    max_items: usize,
) -> Result<Vec<String>, String> {
    let values = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array argument: {key}"))?;
    if values.is_empty() || values.len() > max_items {
        return Err(format!(
            "{key} must contain between 1 and {max_items} items"
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain non-empty strings"))
        })
        .collect()
}

pub(super) fn reviewer_role_arg(args: &Value) -> Result<Option<ReviewerRole>, String> {
    args.get("role")
        .cloned()
        .map(|value| {
            serde_json::from_value(value).map_err(|error| format!("invalid reviewer role: {error}"))
        })
        .transpose()
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/definitions.rs"]
mod tests;
