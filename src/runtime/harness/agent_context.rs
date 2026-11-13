use super::*;

#[path = "agent_context_support.rs"]
mod agent_context_support;
use agent_context_support::*;
#[path = "agent_readiness.rs"]
mod agent_readiness;
#[path = "capability.rs"]
mod capability;
#[path = "context/anchors.rs"]
mod context_anchors;
#[path = "context/budget.rs"]
mod context_budget;
#[path = "context/dedup.rs"]
mod context_dedup;
#[path = "context/guidance.rs"]
mod context_guidance;
#[path = "context/handoff.rs"]
mod context_handoff;
#[path = "context/lessons.rs"]
mod context_lessons;
#[path = "context/operations.rs"]
mod context_operations;
#[path = "context/project.rs"]
mod context_project;
#[cfg(test)]
use agent_readiness::covered_repo_map_precision;
use agent_readiness::{
    query_needs_semantic_relationships, query_requests_architecture_change, update_agent_readiness,
};
#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/context_budget.rs"]
mod tests;
use context_budget::{serialized_json_bytes, trim_agent_context_from_tokens};
#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/fast_context.rs"]
mod fast_context_tests;

const MIN_AGENT_CONTEXT_BUDGET: usize = 1_000;

pub(crate) fn diagnostic_locations(query: &str) -> Vec<Value> {
    context_anchors::diagnostic_locations(query)
}
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
        self.build_agent_context(
            workspace_id,
            workspace,
            query,
            budget,
            requested_scopes,
            false,
        )
    }

    fn build_agent_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        budget: usize,
        requested_scopes: &[String],
        scoped_handoff: bool,
    ) -> Result<Value> {
        let total_started = Instant::now();
        let workspace_id = workspace_id.into();
        let query = query.trim();
        if query.is_empty() {
            bail!("agent context query must not be empty");
        }
        let requested_budget =
            (budget != 0).then(|| budget.clamp(MIN_AGENT_CONTEXT_BUDGET, MAX_AGENT_CONTEXT_BUDGET));
        // Execution recovery is intentionally fail-soft: a damaged runtime snapshot
        // must not prevent the agent from receiving repository context needed to repair it.
        let execution = crate::execution::active_summary(self, &workspace_id, workspace)
            .unwrap_or_else(|_| {
                Some(json!({
                    "available":false, "phase":"unknown",
                    "scope_completion":{
                        "allowed":false, "open_items":null,
                        "required_action":"restore_execution_state"
                    }
                }))
            });
        let capabilities = capability::manifest(query, requested_scopes, execution.as_ref());
        let profile_started = Instant::now();
        let (profile, cache_hit) = self.load_project_profile(workspace)?;
        let known_checks = harness_profile::known_checks_from_profile(&profile);
        if requested_scopes.is_empty() {
            if let Some(mut pack) = context_operations::build(
                &profile,
                &workspace_id,
                query,
                requested_budget,
                self.max_parallel,
                cache_hit,
            )? {
                if let Some(execution) = execution.clone() {
                    pack["execution"] = execution;
                }
                pack["capabilities"] = capabilities.clone();
                if scoped_handoff {
                    context_handoff::prune(&mut pack);
                }
                context_lessons::attach(
                    &mut pack,
                    workspace,
                    query,
                    requested_budget.unwrap_or(MIN_AGENT_CONTEXT_BUDGET),
                    &known_checks,
                );
                sync_readiness_capabilities(&mut pack);
                let fast_budget = pack["budget"]
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(MIN_AGENT_CONTEXT_BUDGET);
                finalize_agent_context(&mut pack, 0, fast_budget)?;
                return Ok(pack);
            }
        }
        let profile_ms = profile_started.elapsed().as_millis();
        let internal_budget = requested_budget
            .map(|budget| budget.saturating_mul(2).clamp(2_000, 12_000))
            .unwrap_or(4_000);
        let context_request = SoftwareContextRequest {
            query: query.to_owned(),
            intent: "implement".to_owned(),
            budget: internal_budget,
            scopes: requested_scopes.to_vec(),
        };
        // Context resolution and repository convention discovery are independent
        // after Project Profile is known. Keep their caches/freshness contracts
        // separate, but overlap the filesystem/index work on cold requests.
        let ((context_result, software_context_ms), convention_report) = rayon::join(
            || {
                let started = Instant::now();
                let result = context_anchors::build_context(
                    self,
                    &workspace_id,
                    workspace,
                    &known_checks,
                    &context_request,
                );
                (result, started.elapsed().as_millis())
            },
            || self.convention_status_cached(workspace),
        );
        let (context, anchors) = context_result?;
        let convention_report = convention_report?;
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
        let explicit_ids = explicit_hot_source_ids(&context.symbols, query);
        // Prepare a small relevant pair even at 1K. Selection is bounded here;
        // the final byte-aware packer decides what fits, not a 2K cliff.
        let hot_source_items = explicit_ids.len().max(2);
        let mut hot_source_ids = explicit_ids.clone();
        if hot_source_ids.len() < hot_source_items
            && !contains_implementation_hot_source(&context.symbols, &hot_source_ids)
        {
            if let Some(id) =
                primary_implementation_hot_source_id(&context.symbols, &hot_source_ids)
            {
                hot_source_ids.push(id);
            }
        }
        // Bodies are an edit/debugging resource, so executable/type definitions
        // should consume the tiny Hot Source budget before module/import wrappers.
        // Keep retrieval identity order unchanged; this only chooses which already
        // discovered symbols receive source bodies.
        for prefer_module in [false, true] {
            for symbol in &context.symbols {
                if hot_source_ids.len() >= hot_source_items {
                    break;
                }
                let is_module = symbol["kind"].as_str() == Some("module");
                if is_module != prefer_module {
                    continue;
                }
                let Some(id) = symbol["id"].as_str() else {
                    continue;
                };
                if !hot_source_ids.iter().any(|existing| existing == id) {
                    hot_source_ids.push(id.to_owned());
                }
            }
        }
        if !anchors.is_empty() {
            hot_source_ids.clear();
        }
        // Share a bounded body allowance instead of multiplying the payload by
        // the number of targets. Small explicit functions can all fit at once.
        let hot_source_chars = budget
            .saturating_mul(4)
            .saturating_div(3)
            .clamp(900, 3_200)
            .saturating_mul(2)
            .saturating_div(hot_source_items.max(2));
        // RepoMap ranking and direct Hot Source expansion share only immutable
        // context. CodeIndex parse flights already coalesce any overlapping file
        // parse, so these two retrieval lanes can safely make forward progress
        // together instead of serializing the cold graph behind exact source.
        let (repo_map, hot_source) = rayon::join(
            || {
                context_anchors::repo_map(
                    self,
                    &workspace_id,
                    workspace,
                    query,
                    &context,
                    anchors.is_empty(),
                    MAX_AGENT_REPO_MAP,
                )
            },
            || -> Result<Vec<Value>> {
                let mut sources = Vec::new();
                for batch in hot_source_ids.chunks(self.max_parallel.max(1)) {
                    let loaded = crate::resource::parallel_io(batch, |symbol_id| {
                        let source = self
                            .code_index
                            .symbol_hot_context(
                                &workspace_id,
                                workspace,
                                symbol_id,
                                MAX_AGENT_HOT_SOURCE_LINES,
                            )
                            .ok()?;
                        let mut source = compact_hot_source(&source, hot_source_chars);
                        source["selection"] = json!(if explicit_ids.contains(symbol_id) {
                            "explicit"
                        } else {
                            "ranked"
                        });
                        Some(source)
                    })?;
                    sources.extend(loaded.into_iter().flatten());
                }
                Ok(sources)
            },
        );
        let repo_map = repo_map?;
        let mut hot_source = hot_source?;
        if anchors.is_empty() && hot_source.len() < 4 && query_needs_semantic_relationships(query) {
            // Search past already delivered relationships. Finding one cached
            // body must not suppress the next unseen caller/callee.
            let existing = hot_source
                .iter()
                .filter_map(|source| source.get("id").and_then(Value::as_str))
                .collect::<BTreeSet<_>>();
            let related = repo_map
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|item| {
                    item.get("relationships")
                        .and_then(Value::as_array)
                        .is_some_and(|relations| !relations.is_empty())
                })
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .filter_map(|id| id.strip_prefix("symbol:"))
                .find(|id| !existing.contains(id));
            if let Some(symbol_id) = related {
                if let Ok(source) = self.code_index.symbol_hot_context(
                    &workspace_id,
                    workspace,
                    symbol_id,
                    MAX_AGENT_HOT_SOURCE_LINES,
                ) {
                    // Append below existing targets; budget trimming, not a
                    // relationship lookup, decides which lower-priority body yields.
                    let mut source = compact_hot_source(&source, hot_source_chars);
                    source["selection"] = json!("related");
                    hot_source.push(source);
                }
            }
        }

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
                    "install": status.install,
                    "reason": status.reason,
                })
            })
            .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Fail-soft like the old MCP-layer merge: a broken worklist store must not
        // take down the whole context pack.
        let worklist = if scoped_handoff {
            None
        } else {
            crate::worklist::active_summary(workspace).unwrap_or(None)
        };
        let migration_audit = crate::migration_audit::context_summary(workspace);
        let design_state = self.design_status(workspace_id.clone(), workspace).ok();

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
            "design_state": design_state,
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
            "execution": execution,
            "capabilities": capabilities,
            "workflow": [
                "Parallel-first is mandatory: if readiness.parallelism.required is true, launch independent lanes concurrently now. Prefer one-traversal bulk tools such as read_files, search_many and apply_file_edits before per-item fan-out; use parallel_tools only when no bulk primitive fits.",
                "Start from hot_source; open additional bodies only when the edit requires them.",
                "Reuse existing components/helpers before adding branches, wrappers, or new modules.",
                "Treat core_constraints as mandatory wcode policy in every workspace. Maintained source cannot cross 1000 lines; an already oversized source file may not grow and verification stays blocked until it is decomposed below the limit. Split by cohesive responsibility instead of mechanically slicing text; when conventions.errors is non-zero, reconciliation_plan can turn those violations into implementation tasks.",
                "Edit independent target files concurrently; serialize only edits with real data or path dependencies. Reuse shared snapshots/caches and never repeat a repository traversal that a bulk primitive can perform once.",
                "Edit with listed SHA preconditions; justify any file outside this pack.",
                "After edits run review_changes, then verify_project at the recommended level."
            ],
        });
        if let Some(migration_audit) = migration_audit {
            pack["migration_audit"] = migration_audit;
        }
        if scoped_handoff {
            context_handoff::prune(&mut pack);
        }
        context_anchors::merge(&mut pack, anchors);
        context_lessons::attach(&mut pack, workspace, query, budget, &known_checks);
        refresh_readiness_capabilities(&mut pack);
        pack["decision_plane"] =
            serde_json::to_value(crate::decision::agent_context_decisions(&pack, query))?;
        pack["timing"]["build_ms"] = json!(total_started.elapsed().as_millis());
        finalize_agent_context(&mut pack, baseline_context_bytes, budget)?;
        Ok(pack)
    }
}
