use super::*;
use crate::scopes;
use std::sync::OnceLock;

#[path = "tool_catalog.rs"]
mod tool_catalog;

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
                "description": "Only pass when switching away from the default Workspace."
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

fn strip_model_tuning_args(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
                for key in MODEL_HIDDEN_TUNING_ARGS {
                    properties.remove(*key);
                }
            }
            for child in object.values_mut() {
                strip_model_tuning_args(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_model_tuning_args(child);
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
    strip_model_tuning_args(&mut input_schema);
    let product_scopes = scopes::tool_scopes(name)
        .into_iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>();
    let mut meta = json!({"dev.wcode/productScopes": product_scopes});
    if crate::harness::model_tool_preload_recommended(name) {
        meta["dev.wcode/preloadRecommended"] = Value::Bool(true);
    }
    json!({
        "name": name,
        "title": name.replace('_', " "),
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

fn take_agent_context_telemetry(value: &mut Value) -> serde_json::Map<String, Value> {
    let mut telemetry = serde_json::Map::new();
    let Some(pack) = value.as_object_mut() else {
        return telemetry;
    };
    for key in [
        "serialized_bytes",
        "estimated_tokens",
        "baseline_context_bytes",
        "context_bytes_avoided",
        "context_reduction_percent",
        "cache_hit",
        "timing",
    ] {
        if let Some(value) = pack.remove(key) {
            telemetry.insert(key.to_owned(), value);
        }
    }
    if let Some(repo_map) = pack.get_mut("repo_map").and_then(Value::as_object_mut) {
        let mut repo_telemetry = serde_json::Map::new();
        for key in [
            "cache_hit",
            "build_ms",
            "files_indexed",
            "graph_edges",
            "provider_nodes_mapped",
            "provider_edges_mapped",
            "candidates",
        ] {
            if let Some(value) = repo_map.remove(key) {
                repo_telemetry.insert(key.to_owned(), value);
            }
        }
        if let Some(items) = repo_map.get_mut("items").and_then(Value::as_array_mut) {
            let mut ranking = Vec::new();
            for item in items {
                let Some(item) = item.as_object_mut() else {
                    continue;
                };
                let score = item.remove("score");
                let degree = item.remove("degree");
                if score.is_some() || degree.is_some() {
                    ranking.push(json!({
                        "id": item.get("id").cloned().unwrap_or(Value::Null),
                        "score": score,
                        "degree": degree,
                    }));
                }
            }
            if !ranking.is_empty() {
                repo_telemetry.insert("ranking".to_owned(), Value::Array(ranking));
            }
        }
        if !repo_telemetry.is_empty() {
            telemetry.insert("repo_map".to_owned(), Value::Object(repo_telemetry));
        }
    }
    telemetry
}

pub(super) fn batch_validation_error(item_count: usize) -> Option<Value> {
    if item_count == 0 {
        Some(jsonrpc_error(Value::Null, -32600, "empty batch is invalid"))
    } else if item_count > MAX_BATCH_ITEMS {
        Some(jsonrpc_error(
            Value::Null,
            -32600,
            format!("batch exceeds the {MAX_BATCH_ITEMS}-item limit"),
        ))
    } else {
        None
    }
}

pub(crate) fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

pub(super) fn task_detail(name: &str, args: &Value) -> String {
    let path = || string_arg(args, "path").unwrap_or(".");
    match name {
        "workspace_info" => {
            "inspect configured roots, discovered subspaces, and capabilities".to_owned()
        }
        "scope_status" => "audit Product Scope source coverage and unmapped files".to_owned(),
        "design_status" => "validate structured desired software state".to_owned(),
        "design_init" => "initialize minimal structured desired software state".to_owned(),
        "software_graph" => format!(
            "{} · software graph · {} files · {} symbols",
            path(),
            usize_arg(args, "max_files").unwrap_or(500),
            usize_arg(args, "max_symbols").unwrap_or(1_000)
        ),
        "graph_provider_import" => {
            "persist external semantic/runtime graph provider revision".to_owned()
        }
        "graph_provider_status" => "inspect active external graph provider revisions".to_owned(),
        "semantic_provider_status" => "inspect LSP server status".to_owned(),
        "language_quality_status" => {
            "inspect per-language quality capability coverage and gaps".to_owned()
        }
        "language_quality_run" => format!(
            "run check-only language quality provider · {} · {}",
            string_arg(args, "language").unwrap_or("unknown"),
            string_arg(args, "provider_id").unwrap_or("unknown")
        ),
        "semantic_provider_refresh" => format!(
            "refresh LSP semantics · {} · {} files / {} symbols",
            path(),
            usize_arg(args, "max_files").unwrap_or(128),
            usize_arg(args, "max_symbols").unwrap_or(1_000)
        ),
        "semantic_navigation" => format!(
            "{} · semantic {} · {}",
            path(),
            string_arg(args, "intent").unwrap_or("inspect"),
            string_arg(args, "symbol")
                .map(|_| "symbol-first")
                .unwrap_or("position")
        ),
        "graph_history" => format!(
            "list persisted graph revisions · limit {}",
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "graph_query" => "query one persisted software graph revision".to_owned(),
        "graph_diff" => "compare persisted software graph revisions".to_owned(),
        "traceability_status" => {
            "resolve requirement, implementation, and verification chains".to_owned()
        }
        "software_context" => format!(
            "task context · query {} chars · budget {}",
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "budget").unwrap_or(12_000)
        ),
        "agent_context" => match usize_arg(args, "budget") {
            Some(budget) => format!(
                "edit-ready context · query {} chars · budget {budget} tokens",
                string_arg(args, "query").map(str::len).unwrap_or(0)
            ),
            None => format!(
                "edit-ready context · query {} chars · adaptive budget",
                string_arg(args, "query").map(str::len).unwrap_or(0)
            ),
        },
        "execution_status" => "inspect durable coding Execution checkpoint".to_owned(),
        "execution_propose" => "record advisory terminal Execution proposal".to_owned(),
        "execution_steer" => "record structured revision-guarded Execution steering".to_owned(),
        "execution_handoff" => "create clean durable Execution handoff lineage".to_owned(),
        "worklist_status" => "inspect persistent model worklist and runnable lanes".to_owned(),
        "worklist_update" => "update persistent model worklist with revision guard".to_owned(),
        "semantic_status" => format!(
            "semantic registry · limit {}",
            usize_arg(args, "limit").unwrap_or(50)
        ),
        "semantic_query" => format!(
            "semantic query · {} chars · limit {}",
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "semantic_record" => "record non-authoritative semantic candidate".to_owned(),
        "semantic_confirm" => "confirm semantic fact after explicit human attestation".to_owned(),
        "semantic_retire" => "retire semantic fact after explicit human attestation".to_owned(),
        "evidence_status" => format!(
            "evidence{} · limit {}",
            string_arg(args, "subject")
                .map(|_| " for one subject")
                .unwrap_or(""),
            usize_arg(args, "limit").unwrap_or(50)
        ),
        "verification_claim" => format!(
            "claim blind review · {} capabilities",
            array_len(args, "capabilities")
        ),
        "verification_submit" => "submit structured blind review evidence".to_owned(),
        "verification_executor_status" => {
            "inspect cross-language verification executors".to_owned()
        }
        "verification_execute_stages" => {
            "execute required property/mutation/fuzz/runtime stages".to_owned()
        }
        "verification_stage_submit" => {
            "submit property/mutation/fuzz/runtime stage evidence".to_owned()
        }
        "verification_approve" => "record explicit human approval evidence".to_owned(),
        "verification_status" => "inspect verification readiness and reviewer states".to_owned(),
        "verification_history" => format!(
            "list persisted verification plans · limit {}",
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "reconciliation_status" => "load one persisted reconciliation plan".to_owned(),
        "reconciliation_approve" => {
            "freeze one explicit human-approved reconciliation plan snapshot".to_owned()
        }
        "reconciliation_history" => format!(
            "list persisted reconciliation plans · limit {}",
            usize_arg(args, "limit").unwrap_or(10)
        ),
        "reconciliation_execution_status" => {
            "inspect durable reconciliation execution state".to_owned()
        }
        "reconciliation_claim" => "claim one dependency-ready reconciliation task".to_owned(),
        "reconciliation_submit" => "submit claimed reconciliation task result".to_owned(),
        "reconciliation_retry" => "requeue one failed reconciliation task".to_owned(),
        "project_context" => "collect repository guidance and inferred quality checks".to_owned(),
        "review_changes" => "review the current Git working tree for bounded risks".to_owned(),
        "drift_status" => {
            "compare Design State with working-tree and runtime Actual State".to_owned()
        }
        "risk_status" => "assess change, traceability, and drift risks".to_owned(),
        "impact_analysis" => "map current changes to impacted software context".to_owned(),
        "verification_plan" => "create a risk-adaptive verification plan".to_owned(),
        "reconciliation_plan" => {
            "create a bounded Design/Implementation reconciliation plan".to_owned()
        }
        "verify_project" => format!(
            "{} quality gate · timeout {}s",
            string_arg(args, "level").unwrap_or("quick"),
            args.get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120)
        ),
        "parallel_tools" => format!("{} independent tool requests", array_len(args, "tasks")),
        "list_files" => format!(
            "{} · limit {}",
            path(),
            usize_arg(args, "max_entries").unwrap_or(2_000)
        ),
        "search_code" => format!(
            "{} · {} · query {} chars · context {} · limit {}",
            path(),
            string_arg(args, "mode").unwrap_or("auto"),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "context_lines").unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(100)
        ),
        "search_many" => format!(
            "{} · {} · {} queries · context {} · limit {}",
            path(),
            string_arg(args, "mode").unwrap_or("exact"),
            array_len(args, "queries"),
            usize_arg(args, "context_lines").unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(200)
        ),
        "scan_patterns" => format!(
            "{} · {} · {} patterns · context {} · limit {}",
            path(),
            string_arg(args, "mode").unwrap_or("regex"),
            array_len(args, "patterns"),
            usize_arg(args, "context_lines").unwrap_or(2),
            usize_arg(args, "max_results").unwrap_or(500)
        ),
        "search_syntax" => format!(
            "{} · {} AST kinds · regex {} chars · files {} · limit {}",
            path(),
            array_len(args, "node_kinds"),
            string_arg(args, "text_regex").map(str::len).unwrap_or(0),
            usize_arg(args, "max_files").unwrap_or(1_000),
            usize_arg(args, "max_results").unwrap_or(200)
        ),
        "file_outline" => format!(
            "{} · syntax outline · limit {}",
            path(),
            usize_arg(args, "max_symbols").unwrap_or(500)
        ),
        "find_symbol" => format!(
            "{} · symbol query {} chars · limit {}",
            path(),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(50)
        ),
        "symbol_context" => format!(
            "symbol id {} chars · body limit {} lines",
            string_arg(args, "symbol_id").map(str::len).unwrap_or(0),
            usize_arg(args, "max_body_lines").unwrap_or(1_000)
        ),
        "read_file" => format!(
            "{} · lines {}-{}",
            path(),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned())
        ),
        "read_files" => format!(
            "{} files · lines {}-{}{}",
            array_len(args, "paths"),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned()),
            first_array_item(args, "paths")
                .map(|path| format!(" · first {path}"))
                .unwrap_or_default()
        ),
        "read_media" => format!(
            "{} · media metadata{}",
            path(),
            if args
                .get("include_content")
                .and_then(Value::as_bool)
                .is_some_and(|enabled| enabled)
            {
                " + opt-in content"
            } else {
                ""
            }
        ),
        "path_info" => format!("{} · metadata + digest", path()),
        "replace_text" => format!(
            "{} · replace {}B with {}B",
            path(),
            string_arg(args, "old_text").map(str::len).unwrap_or(0),
            string_arg(args, "new_text").map(str::len).unwrap_or(0)
        ),
        "apply_edits" => format!("{} · {} edits", path(), array_len(args, "edits")),
        "write_file" => format!(
            "{} · write {}B{}",
            path(),
            string_arg(args, "content").map(str::len).unwrap_or(0),
            string_arg(args, "expected_sha256")
                .map(|_| " · guarded overwrite")
                .unwrap_or(" · create")
        ),
        "create_directory" => format!("{} · recursive mkdir", path()),
        "create_file" => format!(
            "{} · create {}B",
            path(),
            string_arg(args, "content").map(str::len).unwrap_or(0)
        ),
        "create_files" => format!("{} files · parallel create", array_len(args, "files")),
        "apply_file_edits" => {
            format!(
                "{} files · parallel guarded edits",
                array_len(args, "files")
            )
        }
        "move_path" => format!(
            "{} → {}",
            string_arg(args, "source").unwrap_or("?"),
            string_arg(args, "destination").unwrap_or("?")
        ),
        "move_paths" => format!("{} independent moves", array_len(args, "moves")),
        "delete_path" => format!("{} · one-shot authorized delete", path()),
        "run_command" => command_preview(args),
        _ => "unknown tool request".to_owned(),
    }
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
