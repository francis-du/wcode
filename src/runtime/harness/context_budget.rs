use super::*;

pub(super) fn estimated_json_tokens(value: &Value) -> Result<usize> {
    let bytes = serde_json::to_vec(value)?.len();
    Ok(bytes.div_ceil(4))
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

pub(super) fn trim_agent_context(value: &mut Value, budget: usize) -> Result<()> {
    let mut truncated = value["truncated"].as_bool().unwrap_or(false);
    while estimated_json_tokens(value)? > budget {
        let changed = pop_array(value, "risks", 0)
            || pop_nested_array(value, "relations", "edges", 0)
            || pop_nested_array(value, "relations", "nodes", 0)
            || pop_array(value, "guidance", 0)
            || pop_array(value, "workflow", 0)
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
            || shrink_hot_source_body(value, budget)?
            || pop_array(value, "hot_source", 0);
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
