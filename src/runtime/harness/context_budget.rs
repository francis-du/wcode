use super::*;

#[derive(Default)]
struct ByteCounter(usize);

impl std::io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn serialized_json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> Result<usize> {
    let mut counter = ByteCounter::default();
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

pub(super) fn estimated_json_tokens(value: &Value) -> Result<usize> {
    Ok(serialized_json_bytes(value)?.div_ceil(4))
}

fn shrink_hot_source_body(value: &mut Value, budget: usize, current_tokens: usize) -> bool {
    let excess_bytes = current_tokens.saturating_sub(budget).saturating_mul(4);
    if let Some(source) = value["hot_source"]
        .as_array()
        .and_then(|items| items.first())
        .cloned()
    {
        restore_target_range(value, &source);
    }
    let Some(body) = value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .and_then(|items| items.first_mut())
        .and_then(|item| item.get_mut("body"))
    else {
        return false;
    };
    let Some(content) = body
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return false;
    };
    let chars = content.chars().count();
    if chars <= 64 {
        return false;
    }
    let target = chars
        .saturating_sub(excess_bytes.saturating_add(16))
        .max(64);
    if target >= chars {
        return false;
    }
    truncate_source_body(body, target)
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

fn drop_nested_key(value: &mut Value, parent: &str, key: &str) -> bool {
    value
        .get_mut(parent)
        .and_then(Value::as_object_mut)
        .and_then(|object| object.remove(key))
        .is_some()
}

fn drop_key(value: &mut Value, key: &str) -> bool {
    value
        .as_object_mut()
        .and_then(|object| object.remove(key))
        .is_some()
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

fn compact_conventions(value: &mut Value) -> bool {
    let Some(conventions) = value.get_mut("conventions") else {
        return false;
    };
    let Some(object) = conventions.as_object() else {
        return false;
    };
    if object.len() <= 2 && object.contains_key("errors") && object.contains_key("truncated") {
        return false;
    }
    let errors = object.get("errors").cloned().unwrap_or_else(|| json!(0));
    let truncated = object
        .get("truncated")
        .cloned()
        .unwrap_or(Value::Bool(false));
    *conventions = json!({"errors": errors, "truncated": truncated});
    true
}

fn compact_core_constraints(value: &mut Value) -> bool {
    let Some(constraints) = value.get_mut("core_constraints") else {
        return false;
    };
    if constraints
        .as_array()
        .is_some_and(|items| items.iter().all(|item| item.as_str().is_some()))
    {
        return false;
    }
    *constraints = json!([
        "source<=1000;oversized-no-growth;generated-exempt",
        "standalone-tests=>tests/",
        "architecture-moves=>design-sync",
        "independent-lanes=>parallel;bulk-first;serialize-true-deps-only"
    ]);
    true
}

fn compact_readiness_explanation(value: &mut Value) -> bool {
    let Some(readiness) = value.get_mut("readiness").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    if let Some(parallelism) = readiness
        .get_mut("parallelism")
        .and_then(Value::as_object_mut)
    {
        for key in [
            "execution_bias",
            "instruction",
            "serialize_only",
            "parallel_tools",
            "fallback_tool",
            "lane_targets",
        ] {
            changed |= parallelism.remove(key).is_some();
        }
    }
    for key in [
        "direct_targets",
        "hot_source_items",
        "direct_target_files",
        "sha_targets",
        "verification_refs",
        "resolved_verification_refs",
        "graph_truncated",
    ] {
        changed |= readiness.remove(key).is_some();
    }
    if let Some(complexity) = readiness
        .get_mut("complexity_budget")
        .and_then(Value::as_object_mut)
    {
        changed |= complexity.remove("public_api_changes").is_some();
    }
    changed
}

fn compact_project_explanation(value: &mut Value) -> bool {
    let Some(project) = value.get_mut("project").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    for key in [
        "islands",
        "contracts",
        "project_types",
        "manifests",
        "source_line_limit",
    ] {
        changed |= project.remove(key).is_some();
    }
    changed
}

fn compact_repo_map_explanation(value: &mut Value) -> bool {
    let Some(repo_map) = value.get_mut("repo_map") else {
        return false;
    };
    let Some(object) = repo_map.as_object() else {
        return false;
    };
    if object.len() <= 8
        && object.contains_key("provider")
        && object.contains_key("precision")
        && object.contains_key("items")
        && object.contains_key("truncated")
        && object.contains_key("cache_hit")
        && object.contains_key("scope_path")
        && object.contains_key("files_indexed")
        && object.contains_key("deferred")
    {
        return false;
    }
    let provider = object
        .get("provider")
        .cloned()
        .unwrap_or_else(|| json!("tree-sitter"));
    let precision = object
        .get("precision")
        .cloned()
        .unwrap_or_else(|| json!("syntax"));
    let items = object.get("items").cloned().unwrap_or_else(|| json!([]));
    let truncated = object
        .get("truncated")
        .cloned()
        .unwrap_or(Value::Bool(false));
    let cache_hit = object
        .get("cache_hit")
        .cloned()
        .unwrap_or(Value::Bool(false));
    let scope_path = object
        .get("scope_path")
        .cloned()
        .unwrap_or_else(|| json!("."));
    let files_indexed = object
        .get("files_indexed")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let deferred = object
        .get("deferred")
        .cloned()
        .unwrap_or(Value::Bool(false));
    *repo_map = json!({
        "provider": provider,
        "precision": precision,
        "items": items,
        "truncated": truncated,
        "cache_hit": cache_hit,
        "scope_path": scope_path,
        "files_indexed": files_indexed,
        "deferred": deferred,
    });
    true
}

fn compact_retrieval_explanation(value: &mut Value) -> bool {
    let Some(retrieval) = value.get_mut("retrieval") else {
        return false;
    };
    let Some(object) = retrieval.as_object() else {
        return false;
    };
    if object.len() <= 2 && object.contains_key("strategy") && object.contains_key("resolved") {
        return false;
    }
    let strategy = object
        .get("strategy")
        .cloned()
        .unwrap_or_else(|| json!("explicit-locations"));
    let resolved = object.get("resolved").cloned().unwrap_or_else(|| json!(0));
    *retrieval = json!({"strategy": strategy, "resolved": resolved});
    true
}

fn compact_provenance_explanation(value: &mut Value) -> bool {
    let Some(defaults) = value
        .get_mut("provenance_defaults")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut changed = false;
    for (name, entry) in defaults.iter_mut() {
        // Target records can inherit this provider after source-backed
        // compaction. Do not delete the sole remaining provenance value.
        if name == "targets" {
            continue;
        }
        if let Some(object) = entry.as_object_mut() {
            changed |= object.remove("provider").is_some();
        }
    }
    changed
}

fn compact_timing_explanation(value: &mut Value) -> bool {
    value
        .get_mut("timing")
        .and_then(Value::as_object_mut)
        .and_then(|timing| timing.remove("profile_ms"))
        .is_some()
}

fn compact_source_policy_metadata(value: &mut Value) -> bool {
    let mut changed = false;
    if let Some(project) = value.get_mut("project").and_then(Value::as_object_mut) {
        changed |= project.remove("source_line_limit").is_some();
    }
    if let Some(files) = value.get_mut("files").and_then(Value::as_array_mut) {
        for file in files {
            let oversized = file
                .get("source_oversized")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(object) = file.as_object_mut() else {
                continue;
            };
            changed |= object.remove("source_line_limit").is_some();
            if !oversized {
                changed |= object.remove("source_lines").is_some();
                changed |= object.remove("source_oversized").is_some();
            }
        }
    }
    changed
}

fn compact_semantic_hint_explanations(value: &mut Value) -> bool {
    let mut changed = false;
    for hint in value
        .get_mut("semantic_provider_hints")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = hint.as_object_mut() {
            // Keep language/provider/action/discovery and executable identity.
            // Repeated explanatory prose must yield before edit-critical data.
            changed |= object.remove("reason").is_some();
        }
    }
    changed
}

fn compact_hot_source_metadata(value: &mut Value) -> bool {
    let mut changed = false;
    for source in value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = source.as_object_mut() {
            // Signature/call summaries duplicate source or graph evidence.
            // Keep every body's identity, provenance, selection and SHA intact.
            changed |= object.remove("signature").is_some();
            changed |= object.remove("calls").is_some();
        }
    }
    changed
}

pub(super) fn compact_duplicate_symbol_metadata(value: &mut Value) -> bool {
    let complete = value["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|source| {
            source["body"]["truncated"] == false
                && source["body"]["redacted"] == false
                && source["body"]["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
        })
        .filter(|source| {
            value["files"].as_array().into_iter().flatten().any(|file| {
                source["sha256"].as_str().is_some()
                    && file["path"] == source["path"]
                    && file["sha256"] == source["sha256"]
            })
        })
        .filter_map(|source| {
            Some((
                source["path"].as_str()?.to_owned(),
                source["id"].as_str()?.to_owned(),
            ))
        })
        .collect::<BTreeSet<_>>();
    let mut changed = false;
    for pointer in ["/targets", "/repo_map/items"] {
        for item in value
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            let covered =
                item["path"]
                    .as_str()
                    .zip(item["id"].as_str())
                    .is_some_and(|(path, id)| {
                        complete.contains(&(
                            path.to_owned(),
                            id.strip_prefix("symbol:").unwrap_or(id).to_owned(),
                        ))
                    });
            if let Some(object) = item.as_object_mut() {
                if covered {
                    changed |= object.remove("signature").is_some();
                }
                // Scores explain ordering; they are not relationship evidence.
                changed |= object.remove("score").is_some();
                changed |= object.remove("degree").is_some();
            }
        }
    }
    for file in value
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = file.as_object_mut() {
            changed |= object.remove("size").is_some();
            changed |= object.remove("reasons").is_some();
        }
    }
    changed
}

pub(super) fn compact_source_backed_targets(value: &mut Value) -> bool {
    let defaults = value["provenance_defaults"]["targets"].clone();
    let backed = value["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|source| {
            source["body"]["truncated"] == false && source["body"]["redacted"] == false
        })
        .filter(|source| {
            source["body"]["content"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        })
        .filter(|source| {
            value["files"].as_array().into_iter().flatten().any(|file| {
                source["sha256"].as_str().is_some_and(|sha| {
                    sha.len() == 64 && sha.as_bytes().iter().all(u8::is_ascii_hexdigit)
                }) && file["path"] == source["path"]
                    && file["sha256"] == source["sha256"]
            })
        })
        .filter_map(|source| {
            Some((
                (
                    source["path"].as_str()?.to_owned(),
                    source["id"].as_str()?.to_owned(),
                ),
                (
                    source["body"]["start_line"].as_u64()?,
                    source["body"]["end_line"].as_u64()?,
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut changed = false;
    for target in value
        .get_mut("targets")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let range = target["path"]
            .as_str()
            .zip(target["id"].as_str())
            .and_then(|(path, id)| backed.get(&(path.to_owned(), id.to_owned())))
            .copied();
        let Some((start, end)) = range else { continue };
        if start == 0
            || end < start
            || target["start_line"].as_u64() != Some(start)
            || target["end_line"].as_u64() != Some(end)
        {
            continue;
        }
        let Some(object) = target.as_object_mut() else {
            continue;
        };
        // Full source retains the exact range and file identity. Under pressure,
        // reconstructible summaries yield before another requested source body.
        for key in ["kind", "language", "start_line", "end_line"] {
            changed |= object.remove(key).is_some();
        }
        for key in ["provider", "precision"] {
            if defaults[key].as_str().is_some() && object.get(key) == Some(&defaults[key]) {
                changed |= object.remove(key).is_some();
            }
        }
    }
    changed
}

fn compact_empty_explanations(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in [
        "guidance",
        "workflow",
        "risks",
        "design",
        "semantic_provider_hints",
        "worklist",
    ] {
        if object
            .get(key)
            .is_some_and(|entry| entry.is_null() || entry.as_array().is_some_and(Vec::is_empty))
        {
            changed |= object.remove(key).is_some();
        }
    }
    changed
}

fn pop_unreferenced_file(value: &mut Value) -> bool {
    let source_paths = value["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| source["path"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let Some(files) = value.get_mut("files").and_then(Value::as_array_mut) else {
        return false;
    };
    if files.len() <= 1 {
        return false;
    }
    let Some(index) = files.iter().rposition(|file| {
        !file["path"]
            .as_str()
            .is_some_and(|path| source_paths.contains(path))
    }) else {
        return false;
    };
    files.remove(index);
    true
}

fn compact_selection_explanation(value: &mut Value) -> bool {
    let mut changed = false;
    for source in value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = source.as_object_mut() {
            // Order remains authoritative; this label is only an explanation.
            changed |= object.remove("selection").is_some();
        }
    }
    if value["relations"].as_object().is_some_and(|object| {
        object.len() == 2
            && ["nodes", "edges"].iter().all(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
            })
    }) {
        changed |= drop_key(value, "relations");
    }
    if let Some(items) = value
        .pointer_mut("/repo_map/items")
        .and_then(Value::as_array_mut)
    {
        for item in items {
            if let Some(object) = item.as_object_mut() {
                changed |= object.remove("reason").is_some();
            }
        }
    }
    changed
}

fn restore_target_range(value: &mut Value, source: &Value) {
    if source["body"]["truncated"] != false
        || source["body"]["redacted"] != false
        || !value["files"].as_array().into_iter().flatten().any(|file| {
            source["sha256"].as_str().is_some_and(|sha| !sha.is_empty())
                && file["path"] == source["path"]
                && file["sha256"] == source["sha256"]
        })
    {
        return;
    }
    for target in value
        .get_mut("targets")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if source["id"].as_str().is_some()
            && target["id"] == source["id"]
            && target["path"] == source["path"]
        {
            if let Some(object) = target.as_object_mut() {
                for key in ["start_line", "end_line"] {
                    if source["body"][key].as_u64().is_some() {
                        object
                            .entry(key)
                            .or_insert_with(|| source["body"][key].clone());
                    }
                }
            }
        }
    }
}

fn pop_secondary_source(value: &mut Value) -> bool {
    let Some(items) = value.get_mut("hot_source").and_then(Value::as_array_mut) else {
        return false;
    };
    if items.len() <= 1 {
        return false;
    }
    let source = items.pop().expect("secondary source exists");
    // Metadata was compacted only while a matching full body existed. If that
    // body must yield, keep its original coordinates for a follow-up read.
    restore_target_range(value, &source);
    true
}

fn compact_primary_hot_source(value: &mut Value) -> bool {
    let Some(source) = value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .and_then(|items| items.first_mut())
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    // Identity, provenance, line bounds and redaction are edit-safety data,
    // not optional explanation. Never lose them while shrinking a payload.
    let previous_len = source.len();
    source.retain(|key, _| {
        matches!(
            key.as_str(),
            "id" | "path" | "qualified_name" | "sha256" | "body" | "provider" | "precision"
        )
    });
    let mut changed = source.len() != previous_len;
    if let Some(body) = source.get_mut("body").and_then(Value::as_object_mut) {
        let previous_len = body.len();
        body.retain(|key, _| {
            matches!(
                key.as_str(),
                "start_line" | "end_line" | "content" | "redacted" | "truncated"
            )
        });
        changed |= body.len() != previous_len;
    }
    changed
}

#[cfg(test)]
pub(super) fn trim_agent_context(value: &mut Value, budget: usize) -> Result<()> {
    let current_tokens = estimated_json_tokens(value)?;
    trim_agent_context_from_tokens(value, budget, current_tokens)
}

pub(super) fn trim_agent_context_from_tokens(
    value: &mut Value,
    budget: usize,
    mut current_tokens: usize,
) -> Result<()> {
    let mut truncated = value["truncated"].as_bool().unwrap_or(false);
    let explicit_target_min = value
        .get("query")
        .and_then(Value::as_str)
        .map(crate::intelligence::code_query_literals)
        .map(|literals| {
            let literals = literals.into_iter().take(4).collect::<HashSet<_>>();
            value
                .get("targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|target| target.get("qualified_name").and_then(Value::as_str))
                .filter(|name| literals.contains(&name.to_ascii_lowercase()))
                .count()
        })
        .unwrap_or(0)
        .max(1);
    while current_tokens > budget {
        // Decision Plane output is derived advisory observability. Under a
        // tight budget it yields before original risk/evidence and before
        // edit-critical SHA/source/test data.
        let changed = drop_key(value, "decision_plane")
            || drop_nested_key(value, "capabilities", "host_contract")
            || pop_nested_array(value, "capabilities", "deferred_product_scopes", 0)
            || pop_nested_array(value, "capabilities", "active_product_scopes", 0)
            || drop_key(value, "capabilities")
            || pop_array(value, "risks", 0)
            || pop_nested_array(value, "relations", "edges", 0)
            || pop_nested_array(value, "relations", "nodes", 0)
            || pop_array(value, "guidance", 0)
            || pop_array(value, "workflow", 0)
            // Hard-policy state stays present under tight budgets, but verbose
            // convention findings and prose-heavy constraint metadata yield to
            // edit-critical SHA/source/test context.
            || compact_conventions(value)
            || compact_core_constraints(value)
            || compact_source_policy_metadata(value)
            || compact_readiness_explanation(value)
            || compact_project_explanation(value)
            || compact_source_backed_targets(value)
            || compact_provenance_explanation(value)
            || compact_timing_explanation(value)
            || compact_semantic_hint_explanations(value)
            || compact_hot_source_metadata(value)
            || compact_duplicate_symbol_metadata(value)
            // Under extreme budgets preserve the ranked item itself, but remove
            // cache/build/experience/routing metadata that can be reconstructed
            // by a broader follow-up query. Exact diagnostic source and SHA win.
            || compact_repo_map_explanation(value)
            || compact_retrieval_explanation(value)
            || drop_key(value, "baseline_context_bytes")
            || drop_key(value, "cache_hit")
            || drop_key(value, "scopes")
            // Build and software-context timing remain part of the Agent Context
            // observability contract even under extreme token budgets. Profile
            // timing is already compacted above, so keep the two task-critical
            // timings and trim other explanatory metadata first.
            // Retrieval routing is explanatory heuristic metadata. Under a
            // tight edit budget it must disappear before SHA targets, tests,
            // the strongest repo-map item, or diagnostic Hot Source.
            || drop_nested_key(value, "repo_map", "routing")
            || compact_execution_summary(value)
            || pop_nested_array(value, "worklist", "parallel_runnable", 0)
            || pop_nested_array(value, "worklist", "items", 0)
            || pop_nested_array(value, "worklist", "runnable", 0)
            || compact_worklist_summary(value)
            || compact_empty_explanations(value)
            || pop_nested_array(value, "repo_map", "items", 1)
            || pop_array(value, "design", 1)
            || pop_array(value, "checks", 1)
            || pop_array(value, "semantic_provider_hints", 1)
            || pop_array(value, "tests", 1)
            || pop_nested_array(value, "retrieval", "anchors", 1)
            || pop_array(value, "targets", explicit_target_min)
            // A retained body and its file SHA/readonly precondition are one
            // edit unit. Drop unrelated file metadata first, never orphan a body.
            || pop_unreferenced_file(value)
            // Hot Source is relevance-ranked. Drop secondary bodies before
            // shrinking the strongest direct body so tight budgets preserve
            // the most useful edit context for as long as possible.
            || compact_selection_explanation(value)
            || pop_secondary_source(value)
            || compact_primary_hot_source(value)
            || shrink_hot_source_body(value, budget, current_tokens);
        if !changed {
            break;
        }
        truncated = true;
        current_tokens = estimated_json_tokens(value)?;
    }
    value["truncated"] = json!(truncated);
    Ok(())
}

fn compact_execution_summary(value: &mut Value) -> bool {
    let Some(execution) = value.get_mut("execution").and_then(Value::as_object_mut) else {
        return false;
    };
    if execution.get("compacted").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    let checkpoint = execution
        .get("checkpoint")
        .and_then(Value::as_object)
        .map(|checkpoint| {
            json!({
                "worklist_revision": checkpoint.get("worklist_revision"),
                "repository_revision": checkpoint.get("repository_revision"),
                "reconciliation_plan_id": checkpoint.get("reconciliation_plan_id"),
                "verification_plan_id": checkpoint.get("verification_plan_id"),
                "verification_ready": checkpoint.get("verification_ready"),
                "blockers": checkpoint.get("blockers"),
            })
        })
        .unwrap_or(Value::Null);
    let compact = json!({
        "id": execution.get("id"),
        "revision": execution.get("revision"),
        "phase": execution.get("phase"),
        "checkpoint": checkpoint,
        "pending_directive": execution.get("pending_directive"),
        "replan_required": execution.get("replan_required"),
        "verification_floor": execution.get("verification_floor"),
        "lineage": execution.get("lineage"),
        "compacted": true,
        "guidance": "Pending structured steering and its verification floor are never dropped by context compaction. Apply steering through Worklist/replan state first; call execution_status for the full checkpoint."
    });
    *execution = compact.as_object().cloned().unwrap_or_default();
    true
}

fn compact_worklist_summary(value: &mut Value) -> bool {
    let Some(worklist) = value.get_mut("worklist").and_then(Value::as_object_mut) else {
        return false;
    };
    if worklist.get("truncated").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    // Only abbreviate this context view; the revision-guarded durable list stays intact.
    if let Some(goal) = worklist.get("goal").and_then(Value::as_str) {
        let goal = short_text(goal, 120);
        worklist.insert("goal".to_owned(), json!(goal));
    }
    worklist.insert("truncated".to_owned(), json!(true));
    worklist.insert(
        "guidance".to_owned(),
        json!("Call worklist_status to resume omitted items; never overwrite unfinished work."),
    );
    true
}
