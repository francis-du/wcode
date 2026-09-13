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

fn shrink_hot_source_body(value: &mut Value, budget: usize) -> Result<bool> {
    let excess_bytes = estimated_json_tokens(value)?
        .saturating_sub(budget)
        .saturating_mul(4);
    let Some(body) = value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .and_then(|items| items.first_mut())
        .and_then(|item| item.get_mut("body"))
    else {
        return Ok(false);
    };
    let Some(content) = body
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    let chars = content.chars().count();
    if chars <= 64 {
        return Ok(false);
    }
    let target = chars
        .saturating_sub(excess_bytes.saturating_add(16))
        .max(64);
    if target >= chars {
        return Ok(false);
    }
    body["content"] = json!(short_text(&content, target));
    body["truncated"] = json!(true);
    Ok(true)
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
        "architecture-moves=>design-sync"
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
    if object.len() <= 7
        && object.contains_key("provider")
        && object.contains_key("precision")
        && object.contains_key("items")
        && object.contains_key("truncated")
        && object.contains_key("cache_hit")
        && object.contains_key("scope_path")
        && object.contains_key("files_indexed")
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
    *repo_map = json!({
        "provider": provider,
        "precision": precision,
        "items": items,
        "truncated": truncated,
        "cache_hit": cache_hit,
        "scope_path": scope_path,
        "files_indexed": files_indexed,
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
    for entry in defaults.values_mut() {
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

fn compact_primary_hot_source(value: &mut Value) -> bool {
    let Some(source) = value
        .get_mut("hot_source")
        .and_then(Value::as_array_mut)
        .and_then(|items| items.first_mut())
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let already_compact = source.len() <= 3
        && source.contains_key("qualified_name")
        && source.contains_key("sha256")
        && source.contains_key("body");
    if already_compact {
        return false;
    }
    let qualified_name = source.get("qualified_name").cloned().unwrap_or(Value::Null);
    let sha256 = source.get("sha256").cloned().unwrap_or(Value::Null);
    let body = source.get("body").cloned().unwrap_or(Value::Null);
    let compact_body = json!({
        "start_line": body.get("start_line").cloned().unwrap_or(Value::Null),
        "content": body.get("content").cloned().unwrap_or_else(|| json!("")),
        "truncated": body.get("truncated").cloned().unwrap_or(Value::Bool(false)),
    });
    *source = serde_json::Map::from_iter([
        ("qualified_name".to_owned(), qualified_name),
        ("sha256".to_owned(), sha256),
        ("body".to_owned(), compact_body),
    ]);
    true
}

pub(super) fn trim_agent_context(value: &mut Value, budget: usize) -> Result<()> {
    let mut truncated = value["truncated"].as_bool().unwrap_or(false);
    while estimated_json_tokens(value)? > budget {
        let changed = pop_array(value, "risks", 0)
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
            || compact_provenance_explanation(value)
            || compact_timing_explanation(value)
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
            || pop_nested_array(value, "worklist", "parallel_runnable", 0)
            || pop_nested_array(value, "worklist", "items", 0)
            || pop_nested_array(value, "worklist", "runnable", 0)
            || compact_worklist_summary(value)
            || pop_nested_array(value, "repo_map", "items", 1)
            || pop_array(value, "design", 1)
            || pop_array(value, "checks", 1)
            || pop_array(value, "semantic_provider_hints", 1)
            || pop_array(value, "tests", 1)
            || pop_nested_array(value, "retrieval", "anchors", 1)
            || pop_array(value, "targets", 1)
            || pop_array(value, "files", 1)
            // Hot Source is relevance-ranked. Drop secondary bodies before
            // shrinking the strongest direct body so tight budgets preserve
            // the most useful edit context for as long as possible.
            || pop_array(value, "hot_source", 1)
            || compact_primary_hot_source(value)
            || shrink_hot_source_body(value, budget)?;
        if !changed {
            break;
        }
        truncated = true;
    }
    value["truncated"] = json!(truncated);
    Ok(())
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
