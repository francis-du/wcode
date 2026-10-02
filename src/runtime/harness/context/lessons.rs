use crate::engineering_journal::{self, EngineeringFailureCode};
use crate::workspace::Workspace;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Default)]
struct ObservedRule {
    count: usize,
    latest: u64,
    paths: BTreeSet<String>,
}

/// Historical observations select fixed advice, never executable instructions,
/// verification evidence, current blockers or authorization.
pub(super) fn attach(
    pack: &mut Value,
    workspace: &Workspace,
    query: &str,
    budget: usize,
    known_checks: &HashSet<String>,
) {
    let paths = pack["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["path"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    match engineering_journal::failure_memory::recall(
        workspace,
        query,
        &paths,
        known_checks,
        if budget < 2_000 { 1 } else { 2 },
    ) {
        Ok(memory) if !memory.items.is_empty() || memory.partial => {
            pack["failure_memory"] = json!({
                "kind":"historical_advisory",
                "available":true,
                "coverage":if memory.partial {"partial"} else {"retained_history"},
                "retained_records":memory.retained_records,
                "omitted":memory.omitted,
                "items":memory.items,
                "can_authorize":false,
                "proves_current_verification":false,
            });
        }
        Ok(_) => {}
        Err(_) => {
            pack["failure_memory"] = json!({"kind":"historical_advisory","available":false});
        }
    }
    let history = match engineering_journal::load_recent(workspace, 512) {
        Ok(history) => history,
        Err(_) => {
            pack["lessons"] = json!({"kind":"historical_advisory","available":false});
            return;
        }
    };
    let mut rules = BTreeMap::<EngineeringFailureCode, ObservedRule>::new();
    let mut seen = BTreeSet::new();
    for event in history.records {
        if !matches!(event.outcome.as_str(), "failed" | "blocked") {
            continue;
        }
        // Duplicate identities cannot inflate recurrence. Legacy events have
        // no failure codes, so they never teach an inferred error category.
        let Some(id) = event.event_id else { continue };
        if !seen.insert(id) {
            continue;
        }
        for code in event.failure_codes {
            let rule = rules.entry(code).or_default();
            rule.count += 1;
            rule.latest = rule.latest.max(event.timestamp_ms);
            for path in &event.paths {
                if rule.paths.len() < 4 && workspace.source_metadata_stamp(path).is_ok() {
                    rule.paths.insert(path.clone());
                }
            }
        }
    }
    let query = query.to_lowercase();
    let selected_paths = pack["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["path"].as_str())
        .collect::<BTreeSet<_>>();
    let mut selected = rules
        .into_iter()
        .filter(|(code, rule)| {
            boundary_rule(*code)
                || rule
                    .paths
                    .iter()
                    .any(|path| selected_paths.contains(path.as_str()) || query.contains(path))
                || query_matches(*code, &query)
        })
        .collect::<Vec<_>>();
    selected.sort_by(|(left_code, left), (right_code, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest.cmp(&left.latest))
            .then_with(|| left_code.cmp(right_code))
    });
    let limit = if budget < 2_000 { 1 } else { 2 };
    let omitted = selected.len().saturating_sub(limit);
    let items = selected
        .into_iter()
        .take(limit)
        .map(|(code, rule)| {
            json!({
                "code":code.as_str(),
                "observations":rule.count,
                "recurring":rule.count >= 2,
                "action":action(code),
                "paths":rule.paths.into_iter().take(2).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    if !items.is_empty() || history.truncated {
        pack["lessons"] = json!({
            "kind":"historical_advisory",
            "available":true,
            "coverage":if history.truncated {"partial"} else {"retained_history"},
            "omitted":omitted,
            "rules":items,
        });
    }
}

/// Historical explanations must yield before current check/test/source context.
pub(super) fn compact(pack: &mut Value) -> bool {
    if let Some(memory) = pack.get_mut("failure_memory") {
        if let Some(items) = memory["items"].as_array_mut() {
            for item in items.iter_mut() {
                if item
                    .as_object_mut()
                    .is_some_and(|object| object.remove("paths").is_some())
                {
                    return true;
                }
            }
            if items.len() > 1 {
                items.pop();
                let omitted = memory["omitted"].as_u64().unwrap_or(0);
                memory["omitted"] = json!(omitted + 1);
                return true;
            }
        }
        return pack
            .as_object_mut()
            .is_some_and(|object| object.remove("failure_memory").is_some());
    }
    let Some(lessons) = pack.get_mut("lessons") else {
        return false;
    };
    if let Some(rules) = lessons.get_mut("rules").and_then(Value::as_array_mut) {
        for rule in rules.iter_mut() {
            if rule
                .as_object_mut()
                .is_some_and(|object| object.remove("paths").is_some())
            {
                return true;
            }
        }
        if rules.len() > 1 {
            rules.pop();
            let omitted = lessons["omitted"].as_u64().unwrap_or(0);
            lessons["omitted"] = json!(omitted + 1);
            return true;
        }
    }
    pack.as_object_mut()
        .is_some_and(|object| object.remove("lessons").is_some())
}

fn boundary_rule(code: EngineeringFailureCode) -> bool {
    matches!(
        code,
        EngineeringFailureCode::ShaMismatch
            | EngineeringFailureCode::AuthorizationRequired
            | EngineeringFailureCode::ProtectedPath
    )
}

fn query_matches(code: EngineeringFailureCode, query: &str) -> bool {
    use EngineeringFailureCode::*;
    let words: &[&str] = match code {
        ShaMismatch => &["edit", "change", "修改", "编辑"],
        AuthorizationRequired | ProtectedPath => &["permission", "authorize", "权限", "授权"],
        SourceLimit => &["refactor", "module", "split", "重构", "模块", "拆"],
        VerificationFailure | DiscoveryIncomplete => {
            &["test", "verify", "check", "cargo", "验证", "测试", "检查"]
        }
        Timeout => &["command", "run", "cargo", "执行", "运行", "超时"],
        RevisionStale => &["evidence", "revision", "acceptance", "证据", "验收", "版本"],
    };
    words.iter().any(|word| query.contains(word))
}

fn action(code: EngineeringFailureCode) -> &'static str {
    use EngineeringFailureCode::*;
    match code {
        ShaMismatch => "Re-read current source and SHA before editing; never retry an old SHA or overwrite concurrent work.",
        AuthorizationRequired => "Follow current native authorization requirements; only an operator's matching grant authorizes the action, and a model cannot self-approve.",
        ProtectedPath => "Respect the protected path boundary; do not retry through shell, another tool or an alias.",
        SourceLimit => "Split by cohesive responsibility before exceeding the source limit; do not weaken the convention gate.",
        VerificationFailure => "Inspect the failed checks and repair the cause; mapped, skipped, model review and historical pass do not prove this revision.",
        Timeout => "Inspect the existing execution before retrying. Long run_command uses task_mode=true; clients without Tasks poll command_task status/result/cancel. Never rerun to poll or duplicate a live process.",
        RevisionStale => "Capture the current revision and rerun required verification; old evidence cannot approve new code.",
        DiscoveryIncomplete => "Resolve incomplete discovery or narrow the workspace; unavailable checks are not passing checks.",
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/runtime/harness/context_lessons.rs"]
mod tests;
