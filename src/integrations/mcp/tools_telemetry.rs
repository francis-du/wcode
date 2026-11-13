use super::*;

pub(super) fn take_agent_context_telemetry(value: &mut Value) -> serde_json::Map<String, Value> {
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

pub(in crate::mcp) fn batch_validation_error(item_count: usize) -> Option<Value> {
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

pub(in crate::mcp) fn task_detail(name: &str, args: &Value) -> String {
    let path = || string_arg(args, "path").unwrap_or(".");
    match name {
        "workspace_info" => {
            "inspect configured roots, discovered subspaces, and capabilities".to_owned()
        }
        "scope_status" => "audit Product Scope source coverage and unmapped files".to_owned(),
        "design_status" => "validate structured desired software state".to_owned(),
        "acceptance_policy" => format!(
            "{} native Acceptance Policy",
            string_arg(args, "action").unwrap_or("status")
        ),
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
        "worklist_claim" => "claim or renew one scoped model work lane".to_owned(),
        "worklist_submit" => {
            "record bounded claimed worker result and evidence references".to_owned()
        }
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
        "evidence_journal_export" => format!(
            "export engineering journal · limit {}",
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
            "retain advisory property/mutation/fuzz/runtime report".to_owned()
        }
        "verification_approve" => "request exact local operator approval".to_owned(),
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
                .unwrap_or(600)
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
