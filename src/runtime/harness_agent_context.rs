use super::*;

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
        let profile_ms = profile_started.elapsed().as_millis();
        let internal_budget = requested_budget
            .map(|budget| budget.saturating_mul(2).clamp(2_000, 12_000))
            .unwrap_or(4_000);
        let software_context_started = Instant::now();
        let context = self.software_context(
            workspace_id.clone(),
            workspace,
            query,
            "implement",
            internal_budget,
            requested_scopes,
        )?;
        let software_context_ms = software_context_started.elapsed().as_millis();
        let budget = requested_budget
            .unwrap_or_else(|| adaptive_agent_budget(&context, query, requested_scopes));
        let budget_mode = if requested_budget.is_some() {
            "explicit"
        } else {
            "adaptive"
        };
        let full_context_bytes = serde_json::to_vec(&context)?.len() as u64;
        // This pack replaces the old coding startup path of loading repository/project
        // guidance and then a separate Software Context payload. Use that conservative
        // two-payload lower bound for savings telemetry rather than claiming savings
        // against Software Context alone.
        let baseline_context_bytes =
            full_context_bytes.saturating_add(serde_json::to_vec(profile.as_ref())?.len() as u64);

        let guidance = profile
            .guidance
            .iter()
            .take(MAX_AGENT_GUIDANCE)
            .map(|document| {
                json!({
                    "path": document.path,
                    "excerpt": short_text(&document.excerpt, 640),
                    "truncated": document.truncated || document.excerpt.chars().count() > 640,
                })
            })
            .collect::<Vec<_>>();
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
        let repo_map = self.ranked_repo_map(
            &workspace_id,
            workspace,
            query,
            &context,
            MAX_AGENT_REPO_MAP,
        )?;
        let hot_source_items = if budget >= 3_000 { 2 } else { 1 };
        let hot_source_chars = budget.saturating_mul(4).saturating_div(3).clamp(900, 3_200);
        let hot_source = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("id").and_then(Value::as_str))
            .take(hot_source_items)
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
        let files = paths
            .into_iter()
            .filter_map(|(path, reasons)| {
                let info = workspace.path_info(&path).ok()?;
                (info.kind == "file").then(|| {
                    json!({
                        "path": info.path,
                        "sha256": info.sha256,
                        "size": info.size,
                        "readonly": info.readonly,
                        "reasons": reasons,
                    })
                })
            })
            .take(MAX_AGENT_FILES)
            .collect::<Vec<_>>();
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
        let checks = profile
            .recommended_checks
            .iter()
            .take(MAX_AGENT_CHECKS)
            .map(|check| {
                json!({
                    "id": check.id,
                    "level": check.level,
                    "phase": check.phase,
                    "command": format_command(&check.program, &check.args),
                })
            })
            .collect::<Vec<_>>();
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
            "readiness": {},
            "provenance_defaults": {
                "targets": {"provider": "tree-sitter", "precision": "syntax"},
                "repo_map_symbols": {"provider": "tree-sitter", "precision": "syntax"},
                "hot_source": {"provider": "tree-sitter", "precision": "syntax"}
            },
            "scopes": context.scopes,
            "project": {
                "root": profile.root,
                "project_types": profile.project_types,
                "manifests": profile.manifests,
                "write_enabled": profile.write_enabled,
                "exec_enabled": profile.exec_enabled,
            },
            "guidance": guidance,
            "design": design,
            "targets": targets,
            "repo_map": repo_map,
            "hot_source": hot_source,
            "files": files,
            "tests": tests,
            "relations": {"nodes": graph_nodes, "edges": graph_edges},
            "risks": risks,
            "checks": checks,
            "workflow": [
                "Start from hot_source; open additional bodies only when the edit requires them.",
                "Reuse existing components/helpers before adding branches, wrappers, or new modules.",
                "Edit with listed SHA preconditions; justify any file outside this pack.",
                "After edits run review_changes, then verify_project at the recommended level."
            ],
        });
        update_agent_readiness(&mut pack);
        trim_agent_context(&mut pack, budget)?;
        update_agent_readiness(&mut pack);
        trim_agent_context(&mut pack, budget)?;
        update_agent_readiness(&mut pack);
        pack["timing"]["build_ms"] = json!(total_started.elapsed().as_millis());
        trim_agent_context(&mut pack, budget)?;
        update_agent_readiness(&mut pack);
        finalize_agent_context(&mut pack, baseline_context_bytes)?;
        Ok(pack)
    }
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
    })
}

fn compact_hot_source(source: &Value, max_chars: usize) -> Value {
    let body = source.get("body").cloned().unwrap_or(Value::Null);
    let content = body
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content_truncated = content.chars().count() > max_chars;
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
            "content": short_text(content, max_chars),
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
    let mut text = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        text.push('…');
    }
    text
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
    if query_terms > 12 {
        budget = budget.saturating_add(250);
    }

    budget.clamp(1_200, 4_000)
}

fn update_agent_readiness(value: &mut Value) {
    let targets = value
        .get("targets")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let hot_source = value
        .get("hot_source")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let target_paths = value
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("path").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let target_files = files
        .iter()
        .filter(|file| {
            file.get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| target_paths.contains(path))
        })
        .collect::<Vec<_>>();
    let sha_files = target_files
        .iter()
        .filter(|file| file.get("sha256").and_then(Value::as_str).is_some())
        .count();
    let editable_files = target_files
        .iter()
        .filter(|file| {
            file.get("sha256").and_then(Value::as_str).is_some()
                && file.get("readonly").and_then(Value::as_bool) != Some(true)
        })
        .count();
    let write_enabled = value
        .pointer("/project/write_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tests = value
        .get("tests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let resolved_tests = tests
        .iter()
        .filter(|test| test.get("resolved").and_then(Value::as_bool) == Some(true))
        .count();
    let graph_truncated = value
        .pointer("/repo_map/truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let graph_precision = strongest_repo_map_precision(value);

    let edit = if !write_enabled {
        "read_only_workspace"
    } else if targets == 0 {
        "needs_target"
    } else if sha_files > 0 && editable_files == 0 {
        "read_only_target"
    } else if editable_files == 0 {
        "needs_sha"
    } else if hot_source == 0 {
        "needs_source"
    } else {
        "ready"
    };
    let verify = if tests.is_empty() {
        "needs_mapping"
    } else if resolved_tests == tests.len() {
        "ready"
    } else if resolved_tests == 0 {
        "unresolved"
    } else {
        "partial"
    };
    let mut advisories = Vec::new();
    if !write_enabled {
        advisories.push("workspace_write_disabled");
    } else if sha_files > 0 && editable_files == 0 {
        advisories.push("target_files_read_only");
    }
    if graph_truncated {
        advisories.push("repo_map_truncated");
    }
    if graph_precision == "syntax" {
        advisories.push("syntax_only_relationships");
    }
    if hot_source == 0 && targets > 0 {
        advisories.push("source_body_not_in_pack");
    }
    if tests.is_empty() {
        advisories.push("no_verification_mapping");
    } else if resolved_tests < tests.len() {
        advisories.push("verification_mapping_incomplete");
    }

    let edit_tool = if editable_files <= 1 {
        "apply_edits"
    } else {
        "apply_file_edits"
    };
    let mut next_actions = Vec::<&str>::new();
    match edit {
        "ready" => next_actions.push(edit_tool),
        "needs_source" => {
            next_actions.push("symbol_context");
            next_actions.push(edit_tool);
        }
        "needs_target" => {
            next_actions.push("find_symbol");
            next_actions.push("symbol_context");
            next_actions.push(edit_tool);
        }
        "needs_sha" => {
            next_actions.push("path_info");
            next_actions.push(edit_tool);
        }
        "read_only_workspace" | "read_only_target" => {}
        _ => {}
    }
    if !matches!(edit, "read_only_workspace" | "read_only_target") {
        if verify != "ready" {
            next_actions.push("traceability_status");
        }
        next_actions.push("review_changes");
        next_actions.push("verify_project");
    }

    value["readiness"] = json!({
        "edit": edit,
        "verify": verify,
        "graph_precision": graph_precision,
        "next_actions": next_actions,
        "direct_targets": targets,
        "hot_source_items": hot_source,
        "direct_target_files": target_paths.len(),
        "sha_targets": sha_files,
        "editable_sha_targets": editable_files,
        "recommended_edit_tool": edit_tool,
        "verification_refs": tests.len(),
        "resolved_verification_refs": resolved_tests,
        "graph_truncated": graph_truncated,
        "advisories": advisories,
    });
}

fn strongest_repo_map_precision(value: &Value) -> &'static str {
    let mut strongest = "syntax";
    let mut rank = precision_rank(strongest);
    for relationship in value
        .pointer("/repo_map/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|item| {
            item.get("relationships")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
    {
        let Some(precision) = relationship.get("precision").and_then(Value::as_str) else {
            continue;
        };
        let candidate_rank = precision_rank(precision);
        if candidate_rank > rank {
            rank = candidate_rank;
            strongest = match precision {
                "runtime" => "runtime",
                "semantic" => "semantic",
                "deterministic" => "deterministic",
                "syntax" => "syntax",
                "declared" => "declared",
                "heuristic" => "heuristic",
                _ => strongest,
            };
        }
    }
    strongest
}

fn precision_rank(value: &str) -> u8 {
    match value {
        "runtime" => 6,
        "semantic" => 5,
        "deterministic" => 4,
        "syntax" => 3,
        "declared" => 2,
        "heuristic" => 1,
        _ => 0,
    }
}

fn estimated_json_tokens(value: &Value) -> Result<usize> {
    let bytes = serde_json::to_vec(value)?.len();
    Ok(bytes.div_ceil(4))
}

fn pop_array(value: &mut Value, key: &str, minimum: usize) -> bool {
    let Some(items) = value.get_mut(key).and_then(Value::as_array_mut) else {
        return false;
    };
    if items.len() <= minimum {
        return false;
    }
    items.pop();
    true
}

fn pop_nested_array(value: &mut Value, parent: &str, key: &str, minimum: usize) -> bool {
    let Some(items) = value
        .get_mut(parent)
        .and_then(|parent| parent.get_mut(key))
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    if items.len() <= minimum {
        return false;
    }
    items.pop();
    true
}

fn trim_agent_context(value: &mut Value, budget: usize) -> Result<()> {
    let mut truncated = false;
    while estimated_json_tokens(value)? > budget {
        let changed = pop_array(value, "risks", 0)
            || pop_nested_array(value, "relations", "edges", 0)
            || pop_nested_array(value, "relations", "nodes", 0)
            || pop_array(value, "guidance", 0)
            || pop_array(value, "workflow", 0)
            || pop_nested_array(value, "repo_map", "items", 1)
            || pop_array(value, "design", 1)
            || pop_array(value, "checks", 1)
            || pop_array(value, "tests", 1)
            || pop_array(value, "targets", 1)
            || pop_array(value, "files", 1)
            || pop_array(value, "hot_source", 0);
        if !changed {
            break;
        }
        truncated = true;
    }
    value["truncated"] = json!(truncated);
    Ok(())
}

fn finalize_agent_context(value: &mut Value, baseline_context_bytes: u64) -> Result<()> {
    for _ in 0..2 {
        let bytes = serde_json::to_vec(value)?.len() as u64;
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
    }
    Ok(())
}
