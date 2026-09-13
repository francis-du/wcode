use super::*;

#[path = "agent_readiness.rs"]
mod agent_readiness;
#[path = "context_anchors.rs"]
mod context_anchors;
#[path = "context_budget.rs"]
mod context_budget;
#[path = "context_guidance.rs"]
mod context_guidance;
#[path = "context_operations.rs"]
mod context_operations;
#[path = "context_project.rs"]
mod context_project;
#[cfg(test)]
use agent_readiness::covered_repo_map_precision;
use agent_readiness::{
    query_needs_semantic_relationships, query_requests_architecture_change, update_agent_readiness,
};
#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/context_budget.rs"]
mod tests;
use context_budget::{estimated_json_tokens, serialized_json_bytes, trim_agent_context};

const MIN_AGENT_CONTEXT_BUDGET: usize = 1_000;
const MAX_AGENT_CONTEXT_BUDGET: usize = 12_000;
const MAX_AGENT_GUIDANCE: usize = 2;
const MAX_AGENT_DESIGN_ITEMS: usize = 6;
const MAX_AGENT_TARGETS: usize = 8;
const MAX_AGENT_REPO_MAP: usize = 12;
const MAX_AGENT_HOT_SOURCE_LINES: usize = 80;
const MAX_AGENT_FILES: usize = 10;
const MAX_AGENT_TESTS: usize = 8;
const MAX_AGENT_RISKS: usize = 4;
const MAX_AGENT_CHECKS: usize = 6;

impl ToolHarness {
    pub fn agent_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        budget: usize,
        requested_scopes: &[String],
    ) -> Result<Value> {
        let total_started = Instant::now();
        let workspace_id = workspace_id.into();
        let query = query.trim();
        if query.is_empty() {
            bail!("agent context query must not be empty");
        }
        let requested_budget =
            (budget != 0).then(|| budget.clamp(MIN_AGENT_CONTEXT_BUDGET, MAX_AGENT_CONTEXT_BUDGET));
        let profile_started = Instant::now();
        let (profile, cache_hit) = self.load_project_profile(workspace)?;
        if requested_scopes.is_empty() {
            if let Some(pack) = context_operations::build(
                &profile,
                &workspace_id,
                query,
                requested_budget,
                self.max_parallel,
                cache_hit,
            )? {
                return Ok(pack);
            }
        }
        let profile_ms = profile_started.elapsed().as_millis();
        let internal_budget = requested_budget
            .map(|budget| budget.saturating_mul(2).clamp(2_000, 12_000))
            .unwrap_or(4_000);
        let software_context_started = Instant::now();
        let (context, anchors) = context_anchors::build_context(
            self,
            &workspace_id,
            workspace,
            &SoftwareContextRequest {
                query: query.to_owned(),
                intent: "implement".to_owned(),
                budget: internal_budget,
                scopes: requested_scopes.to_vec(),
            },
        )?;
        let software_context_ms = software_context_started.elapsed().as_millis();
        let budget = requested_budget
            .unwrap_or_else(|| adaptive_agent_budget(&context, query, requested_scopes));
        let budget_mode = if requested_budget.is_some() {
            "explicit"
        } else {
            "adaptive"
        };
        let full_context_bytes = serialized_json_bytes(&context)? as u64;
        // This pack replaces the old coding startup path of loading repository/project
        // guidance and then a separate Software Context payload. Use that conservative
        // two-payload lower bound for savings telemetry rather than claiming savings
        // against Software Context alone. Count into a sink instead of allocating two
        // full serialized buffers solely for telemetry.
        let baseline_context_bytes =
            full_context_bytes.saturating_add(serialized_json_bytes(profile.as_ref())? as u64);

        let guidance = context_guidance::select_guidance(
            &profile.guidance,
            query,
            requested_scopes,
            &context,
            MAX_AGENT_GUIDANCE,
        );
        let core_constraints = compact_core_constraints();
        let convention_report = self.convention_status_cached(workspace)?;
        let conventions = compact_convention_report(convention_report.as_ref());
        let design = context
            .design_items
            .iter()
            .take(MAX_AGENT_DESIGN_ITEMS)
            .map(compact_design_item)
            .collect::<Vec<_>>();
        let targets = context
            .symbols
            .iter()
            .take(MAX_AGENT_TARGETS)
            .map(compact_symbol)
            .collect::<Vec<_>>();
        let repo_map = context_anchors::repo_map(
            self,
            &workspace_id,
            workspace,
            query,
            &context,
            anchors.is_empty(),
        )?;
        let hot_source_items = if budget >= 3_000 { 2 } else { 1 };
        let hot_source_chars = budget.saturating_mul(4).saturating_div(3).clamp(900, 3_200);
        let hot_source = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("id").and_then(Value::as_str))
            .take(if anchors.is_empty() {
                hot_source_items
            } else {
                0
            })
            .filter_map(|symbol_id| {
                self.symbol_context(
                    workspace_id.clone(),
                    workspace,
                    symbol_id,
                    MAX_AGENT_HOT_SOURCE_LINES,
                )
                .ok()
            })
            .map(|source| compact_hot_source(&source, hot_source_chars))
            .collect::<Vec<_>>();

        let mut paths = BTreeMap::<String, BTreeSet<String>>::new();
        for target in &targets {
            if let Some(path) = target.get("path").and_then(Value::as_str) {
                paths
                    .entry(path.to_owned())
                    .or_default()
                    .insert("symbol-match".to_owned());
            }
        }
        for item in repo_map
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(path) = item.get("path").and_then(Value::as_str) {
                paths
                    .entry(path.to_owned())
                    .or_default()
                    .insert("repo-map".to_owned());
            }
        }
        for requirement in &context.coverage.requirements {
            for reference in &requirement.implementation {
                if let Some(path) = trace_target_path(&reference.target) {
                    paths
                        .entry(path)
                        .or_default()
                        .insert(requirement.id.clone());
                }
            }
        }
        // A lexical path limit can evict the actual edit target behind unrelated
        // Design mappings. Preserve direct-symbol order, then ranked repo-map order.
        let mut paths = paths.into_iter().collect::<Vec<_>>();
        paths.sort_by_key(|(path, _)| {
            let direct = targets.iter().position(|target| target["path"] == *path);
            let ranked = repo_map["items"]
                .as_array()
                .and_then(|items| items.iter().position(|item| item["path"] == *path));
            (direct.unwrap_or(usize::MAX), ranked.unwrap_or(usize::MAX))
        });
        let mut files = Vec::with_capacity(MAX_AGENT_FILES);
        for batch in paths.chunks(MAX_AGENT_FILES) {
            let loaded = crate::resource::parallel_io(batch, |(path, reasons)| {
                if let Ok(source) = workspace.load_source(path) {
                    let source_lines =
                        crate::conventions::maintained_source_lines(path, &source.content);
                    return Some(json!({
                        "path": source.path,
                        "sha256": source.sha256,
                        "size": source.content.len(),
                        "readonly": source.readonly,
                        "source_lines": source_lines,
                        "source_line_limit": crate::conventions::OVERSIZED_SOURCE_LINES,
                        "source_oversized": source_lines.is_some_and(|lines| lines > crate::conventions::OVERSIZED_SOURCE_LINES),
                        "reasons": reasons,
                    }));
                }

                // Preserve metadata-only coverage for non-text files while
                // avoiding a second full-file hash/read on normal source files.
                let info = workspace.path_info(path).ok()?;
                (info.kind == "file").then(|| {
                    json!({
                        "path": info.path,
                        "sha256": info.sha256,
                        "size": info.size,
                        "readonly": info.readonly,
                        "source_lines": Value::Null,
                        "source_line_limit": crate::conventions::OVERSIZED_SOURCE_LINES,
                        "source_oversized": false,
                        "reasons": reasons,
                    })
                })
            })?;
            for file in loaded.into_iter().flatten() {
                files.push(file);
                if files.len() == MAX_AGENT_FILES {
                    break;
                }
            }
            if files.len() == MAX_AGENT_FILES {
                break;
            }
        }
        let tests = context
            .coverage
            .requirements
            .iter()
            .flat_map(|requirement| {
                requirement.verification.iter().map(move |reference| {
                    json!({
                        "requirement": requirement.id,
                        "target": reference.target,
                        "resolved": reference.resolved,
                        "provider": reference.provider,
                        "precision": reference.precision,
                    })
                })
            })
            .take(MAX_AGENT_TESTS)
            .collect::<Vec<_>>();
        let risks = context
            .known_risks
            .iter()
            .take(MAX_AGENT_RISKS)
            .map(|risk| {
                json!({
                    "id": risk.id,
                    "subject": risk.subject,
                    "level": risk.level,
                    "category": risk.category,
                    "summary": short_text(&risk.summary, 260),
                })
            })
            .collect::<Vec<_>>();
        let checks = context_project::compact_checks(&profile, MAX_AGENT_CHECKS);
        let islands = context_project::compact_islands(&profile);
        let contracts = context_project::compact_contracts(&profile);
        let graph_nodes = context
            .graph_context
            .nodes
            .iter()
            .take(6)
            .map(|node| {
                json!({
                    "id": node.id,
                    "label": node.label,
                    "path": node.path,
                    "provider": node.provider,
                    "precision": node.precision,
                })
            })
            .collect::<Vec<_>>();
        let graph_edges = context
            .graph_context
            .edges
            .iter()
            .take(8)
            .map(|edge| {
                json!({
                    "from": edge.from,
                    "to": edge.to,
                    "kind": edge.kind,
                    "provider": edge.provider,
                    "precision": edge.precision,
                })
            })
            .collect::<Vec<_>>();
        let semantic_provider_hints = if query_needs_semantic_relationships(query) {
            self.semantic_provider_status_for_languages(
                workspace,
                &convention_report.detected_languages,
            )
            .unwrap_or_default()
            .into_iter()
            .filter(|status| status.detected && status.action.is_some())
            .take(4)
            .map(|status| {
                json!({
                    "language": status.language,
                    "provider": status.provider,
                    "executable": status.executable,
                    "discovery": status.discovery,
                    "action": status.action,
                    "reason": status.reason,
                })
            })
            .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Fail-soft like the old MCP-layer merge: a broken worklist store must not
        // take down the whole context pack.
        let worklist = crate::worklist::active_summary(workspace).unwrap_or(None);
        let migration_audit = crate::migration_audit::context_summary(workspace);

        let mut pack = json!({
            "workspace": workspace_id,
            "query": query,
            "budget": budget,
            "budget_mode": budget_mode,
            "requested_budget": requested_budget,
            "serialized_bytes": 0,
            "estimated_tokens": 0,
            "baseline_context_bytes": baseline_context_bytes,
            "context_bytes_avoided": 0,
            "context_reduction_percent": 0.0,
            "truncated": false,
            "cache_hit": cache_hit,
            "timing": {
                "build_ms": 0,
                "profile_ms": profile_ms,
                "software_context_ms": software_context_ms,
            },
            "readiness": {"parallelism": {"max_parallel": self.max_parallel}},
            "provenance_defaults": {
                "targets": {"provider": "tree-sitter", "precision": "syntax"},
                "repo_map_symbols": {"provider": "tree-sitter", "precision": "syntax"},
                "hot_source": {"provider": "tree-sitter", "precision": "syntax"}
            },
            "scopes": context.scopes,
            "project": {
                "project_types": profile.project_types,
                "manifests": profile.manifests,
                "islands": islands,
                "contracts": contracts,
                "write_enabled": profile.write_enabled,
                "exec_enabled": profile.exec_enabled,
                "source_line_limit": crate::conventions::OVERSIZED_SOURCE_LINES,
            },
            "guidance": guidance,
            "core_constraints": core_constraints,
            "conventions": conventions,
            "design": design,
            "targets": targets,
            "repo_map": repo_map,
            "hot_source": hot_source,
            "files": files,
            "tests": tests,
            "relations": {"nodes": graph_nodes, "edges": graph_edges},
            "risks": risks,
            "checks": checks,
            "semantic_provider_hints": semantic_provider_hints,
            "worklist": worklist,
            "workflow": [
                "Parallel-first: before the next tool call, split work into dependency lanes. If readiness.parallelism.strategy is top_level_concurrent_calls, launch independent discovery/read/review calls together now; do not serialize them merely for convenience.",
                "Start from hot_source; open additional bodies only when the edit requires them.",
                "Reuse existing components/helpers before adding branches, wrappers, or new modules.",
                "Treat core_constraints as mandatory wcode policy in every workspace. Maintained source cannot cross 1000 lines; an already oversized source file may not grow and verification stays blocked until it is decomposed below the limit. Split by cohesive responsibility instead of mechanically slicing text; when conventions.errors is non-zero, reconciliation_plan can turn those violations into implementation tasks.",
                "Edit independent target files concurrently; serialize only edits with real data or path dependencies. Keep parallel_tools for compact fan-out rather than large nested arguments.",
                "Edit with listed SHA preconditions; justify any file outside this pack.",
                "After edits run review_changes, then verify_project at the recommended level."
            ],
        });
        if let Some(migration_audit) = migration_audit {
            pack["migration_audit"] = migration_audit;
        }
        context_anchors::merge(&mut pack, anchors);
        update_agent_readiness(&mut pack);
        pack["timing"]["build_ms"] = json!(total_started.elapsed().as_millis());
        finalize_agent_context(&mut pack, baseline_context_bytes, budget)?;
        Ok(pack)
    }
}

fn compact_core_constraints() -> Vec<Value> {
    vec![
        json!({
            "id": "CONSTRAINT-SOURCE-DECOMPOSITION",
            "rule": "maintained source <=1000 lines; split before growth; generated/binary outputs exempt; filenames <=32 chars; Rust stems <=24",
        }),
        json!({
            "id": "CONSTRAINT-TEST-ROOT",
            "rule": "standalone automated tests live under tests/; avoid growing large inline test modules",
        }),
        json!({
            "id": "CONSTRAINT-DESIGN-SYNC",
            "rule": "responsibility/path/test/trust/transport moves update matching .wcode Design State in the same change",
        }),
    ]
}

fn compact_convention_report(report: &ConventionReport) -> Value {
    json!({
        "provider": report.provider,
        "errors": report.errors,
        "warnings": report.warnings,
        "truncated": report.truncated,
        "findings": report.findings.iter().take(12).map(|finding| json!({
            "code": finding.code,
            "severity": finding.severity,
            "path": finding.path,
            "language": finding.language,
            "message": short_text(&finding.message, 260),
        })).collect::<Vec<_>>(),
    })
}

fn compact_design_item(item: &crate::intelligence::DesignContextItem) -> Value {
    let relations = item
        .relations
        .iter()
        .take(4)
        .map(|(name, values)| {
            (
                name.clone(),
                Value::Array(
                    values
                        .iter()
                        .take(6)
                        .map(|value| Value::String(value.clone()))
                        .collect(),
                ),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "id": item.id,
        "kind": item.kind,
        "title": item.title,
        "summary": short_text(&item.summary, 360),
        "relations": relations,
    })
}

fn compact_symbol(symbol: &Value) -> Value {
    json!({
        "id": symbol.get("id").cloned().unwrap_or(Value::Null),
        "path": symbol.get("path").cloned().unwrap_or(Value::Null),
        "kind": symbol.get("kind").cloned().unwrap_or(Value::Null),
        "qualified_name": symbol.get("qualified_name").cloned().unwrap_or(Value::Null),
        "signature": symbol.get("signature").cloned().unwrap_or(Value::Null),
        "start_line": symbol.pointer("/range/start_line").cloned().unwrap_or(Value::Null),
        "end_line": symbol.pointer("/range/end_line").cloned().unwrap_or(Value::Null),
        "language": symbol.get("language").cloned().unwrap_or(Value::Null),
        "provider": symbol.get("provider").cloned().unwrap_or(json!("tree-sitter")),
        "precision": symbol.get("precision").cloned().unwrap_or(json!("syntax")),
    })
}

fn compact_hot_source(source: &Value, max_chars: usize) -> Value {
    let body = source.get("body").cloned().unwrap_or(Value::Null);
    let content = body
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (content, content_truncated) = short_text_with_truncation(content, max_chars);
    let calls = source
        .get("syntax_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
        .map(|call| {
            json!({
                "name": call.get("name").cloned().unwrap_or(Value::Null),
                "line": call.pointer("/range/start_line").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "id": source.pointer("/symbol/id").cloned().unwrap_or(Value::Null),
        "path": source.pointer("/symbol/path").cloned().unwrap_or(Value::Null),
        "qualified_name": source.pointer("/symbol/qualified_name").cloned().unwrap_or(Value::Null),
        "signature": source.pointer("/symbol/signature").cloned().unwrap_or(Value::Null),
        "sha256": source.get("sha256").cloned().unwrap_or(Value::Null),
        "body": {
            "start_line": body.get("start_line").cloned().unwrap_or(Value::Null),
            "end_line": body.get("end_line").cloned().unwrap_or(Value::Null),
            "content": content,
            "redacted": body.get("redacted").cloned().unwrap_or(Value::Bool(false)),
            "truncated": body.get("truncated").and_then(Value::as_bool).unwrap_or(false) || content_truncated,
        },
        "calls": calls,
    })
}

fn trace_target_path(target: &str) -> Option<String> {
    let candidate = target.split_once("::").map_or(target, |(path, _)| path);
    (candidate.contains('/') || candidate.contains('\\') || candidate.contains('.'))
        .then(|| candidate.to_owned())
}

fn short_text(value: &str, limit: usize) -> String {
    short_text_with_truncation(value, limit).0
}

fn short_text_with_truncation(value: &str, limit: usize) -> (String, bool) {
    let mut chars = value.chars();
    let mut text = chars.by_ref().take(limit).collect::<String>();
    let truncated = chars.next().is_some();
    if truncated {
        text.push('…');
    }
    (text, truncated)
}

fn format_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

fn adaptive_agent_budget(
    context: &SoftwareContext,
    query: &str,
    requested_scopes: &[String],
) -> usize {
    let mut budget = 1_400usize;
    let symbol_count = context.symbols.len();
    if symbol_count == 0 {
        budget = budget.saturating_add(1_000);
    } else if symbol_count > 2 {
        budget = budget.saturating_add(300);
    }

    let requirement_count = context.coverage.requirements.len();
    if requirement_count > 1 {
        budget = budget.saturating_add(300);
    }
    if requirement_count > 3 {
        budget = budget.saturating_add(300);
    }
    if context.design_items.len() > 4 {
        budget = budget.saturating_add(300);
    }
    if context.graph_context.truncated {
        budget = budget.saturating_add(300);
    }
    if !context.known_risks.is_empty() {
        budget = budget.saturating_add(300);
    }
    if requested_scopes.len() > 1 {
        budget = budget.saturating_add(250);
    }
    let query_terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.chars().count() >= 2)
        .count();
    let query_chars = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    // Whitespace-delimited term counts systematically underestimate CJK and
    // other compact natural-language requests. Character length is a bounded
    // language-agnostic fallback so a long instruction can buy enough context
    // without changing the 4k adaptive ceiling.
    if query_terms > 12 || query_chars > 80 {
        budget = budget.saturating_add(250);
    }

    budget.clamp(1_200, 4_000)
}

fn finalize_agent_context(
    value: &mut Value,
    baseline_context_bytes: u64,
    budget: usize,
) -> Result<()> {
    for _ in 0..32 {
        let previous = (
            value["serialized_bytes"].clone(),
            value["estimated_tokens"].clone(),
            value["context_bytes_avoided"].clone(),
            value["context_reduction_percent"].clone(),
        );
        let bytes = serialized_json_bytes(value)? as u64;
        let avoided = baseline_context_bytes.saturating_sub(bytes);
        let reduction_percent = if baseline_context_bytes == 0 {
            0.0
        } else {
            ((avoided as f64 / baseline_context_bytes as f64) * 10_000.0).round() / 100.0
        };
        value["serialized_bytes"] = json!(bytes);
        value["estimated_tokens"] = json!(bytes.div_ceil(4));
        value["context_bytes_avoided"] = json!(avoided);
        value["context_reduction_percent"] = json!(reduction_percent);

        if estimated_json_tokens(value)? > budget {
            trim_agent_context(value, budget)?;
            update_agent_readiness(value);
            continue;
        }

        let current = (
            value["serialized_bytes"].clone(),
            value["estimated_tokens"].clone(),
            value["context_bytes_avoided"].clone(),
            value["context_reduction_percent"].clone(),
        );
        if current == previous {
            return Ok(());
        }
    }
    for _ in 0..8 {
        let bytes = serialized_json_bytes(value)? as u64;
        let tokens = bytes.div_ceil(4);
        value["serialized_bytes"] = json!(bytes);
        value["estimated_tokens"] = json!(tokens);
        let actual_bytes = serialized_json_bytes(value)? as u64;
        if actual_bytes == bytes
            && value["estimated_tokens"].as_u64() == Some(actual_bytes.div_ceil(4))
        {
            break;
        }
    }
    let bytes = serialized_json_bytes(value)? as u64;
    value["serialized_bytes"] = json!(bytes);
    value["estimated_tokens"] = json!(bytes.div_ceil(4));
    let bytes = serialized_json_bytes(value)?;
    if bytes.div_ceil(4) > budget {
        bail!("agent context could not satisfy the {budget}-token budget after finalization");
    }
    Ok(())
}
