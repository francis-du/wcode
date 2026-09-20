use super::*;
use crate::harness::{ChangeReviewReport, VerificationReport};

pub(super) async fn augment_review_checkpoint(
    state: &AppState,
    report: &ChangeReviewReport,
    adversarial: bool,
    value: &mut Value,
) {
    if report.clean {
        return;
    }
    let changed_paths = report
        .files
        .iter()
        .take(32)
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let findings = report
        .findings
        .iter()
        .take(16)
        .map(|finding| {
            json!({
                "severity": finding.severity,
                "code": finding.code,
                "paths": finding.paths.iter().take(8).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let checkpoint_state = json!({
        "files_changed": report.files_changed,
        "source_changed": report.source_changed,
        "tests_changed": report.tests_changed,
        "additions": report.additions,
        "deletions": report.deletions,
        "risk_level": report.risk_level,
        "recommended_verification": report.recommended_verification,
        "changed_paths": changed_paths,
        "findings": findings,
        "adversarial_review_requested": adversarial
    });
    let Ok(telemetry) =
        crate::jev::evaluate_checkpoint("post_edit_review", &checkpoint_state, "edit_then_verify")
            .await
    else {
        return;
    };

    state
        .monitor
        .record_jev_decision(&report.workspace, "post_edit_review", &telemetry);
    let mut next_actions = crate::jev::checkpoint_actions("post_edit_review", &telemetry);
    if adversarial {
        next_actions.retain(|action| action != "review_changes(adversarial=true)");
    }
    let verification_level = if next_actions
        .iter()
        .any(|action| action == "verify_project(level=full)")
    {
        "full"
    } else {
        report.recommended_verification.as_str()
    };
    next_actions.retain(|action| !action.starts_with("verify_project("));
    next_actions.push(format!("verify_project(level={verification_level})"));

    value["decision_assist"] = json!({
        "provider": "jev",
        "checkpoint": "post_edit_review",
        "authority": "increase_only",
        "next_actions": next_actions,
        "telemetry": telemetry
    });
    value["next_actions"] = value["decision_assist"]["next_actions"].clone();
}

pub(super) async fn verification_tool_result(
    state: &AppState,
    report: VerificationReport,
) -> Result<Value, String> {
    let is_error = !report.passed;
    let mut value = serde_json::to_value(&report).map_err(|error| error.to_string())?;
    if !report.passed {
        let locations = verification_failure_locations(state, &report);
        if !locations.is_empty() {
            let contexts = verification_failure_contexts(state, &report, &locations);
            value["failure_locations"] = json!(locations);
            if !contexts.is_empty() {
                value["failure_context"] = json!(contexts);
            }
        }
    }
    augment_verification_checkpoint(state, &report, &mut value).await;
    Ok(tool_result(value, is_error))
}

fn verification_failure_locations(state: &AppState, report: &VerificationReport) -> Vec<Value> {
    let Ok((_workspace_id, workspace)) = state.workspaces.select(Some(&report.workspace)) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut locations = Vec::new();
    for check in report.checks.iter().filter(|check| !check.success).take(8) {
        let diagnostic_text = format!("{}\n{}", check.stderr_tail, check.stdout_tail);
        for location in crate::harness::diagnostic_locations(&diagnostic_text) {
            let reported_path = location
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let line = location
                .get("line")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let column = location.get("column").and_then(Value::as_u64);
            let Some(path) = workspace_relative_diagnostic_path(&workspace, reported_path) else {
                continue;
            };
            if push_failure_location(&mut locations, &mut seen, &check.id, path, line, column) {
                return locations;
            }
        }
        for (path, line, column) in verification_file_uri_locations(&workspace, &diagnostic_text) {
            if push_failure_location(&mut locations, &mut seen, &check.id, path, line, column) {
                return locations;
            }
        }
    }
    locations
}

fn push_failure_location(
    locations: &mut Vec<Value>,
    seen: &mut std::collections::BTreeSet<(String, u64)>,
    check: &str,
    path: String,
    line: u64,
    column: Option<u64>,
) -> bool {
    if line == 0 {
        return false;
    }
    let identity = (path.clone(), line);
    if !seen.insert(identity) {
        if let Some(column) = column {
            if let Some(existing) = locations.iter_mut().find(|location| {
                location.get("path").and_then(Value::as_str) == Some(path.as_str())
                    && location.get("line").and_then(Value::as_u64) == Some(line)
            }) {
                if existing.get("column").is_none_or(Value::is_null) {
                    existing["column"] = json!(column);
                }
            }
        }
        return false;
    }
    locations.push(json!({
        "check": check,
        "path": path,
        "line": line,
        "column": column,
        "precision": "diagnostic_text"
    }));
    locations.len() == 16
}

fn verification_file_uri_locations(
    workspace: &crate::workspace::Workspace,
    diagnostic_text: &str,
) -> Vec<(String, u64, Option<u64>)> {
    let mut locations = Vec::new();
    for line_text in diagnostic_text.lines().take(256) {
        let mut offset = 0usize;
        while let Some(found) = line_text[offset..].find("file://") {
            let start = offset + found;
            let token = line_text[start..]
                .split(|ch: char| ch.is_whitespace() || matches!(ch, ')' | ']' | '>' | ','))
                .next()
                .unwrap_or_default()
                .trim_end_matches(['.', ';']);
            if let Some((uri, line, column)) = split_file_uri_location(token) {
                if let Some(path) = workspace_relative_file_uri(workspace, uri) {
                    locations.push((path, line, column));
                    if locations.len() == 16 {
                        return locations;
                    }
                }
            }
            offset = start.saturating_add(token.len().max(7));
            if offset >= line_text.len() {
                break;
            }
        }
    }
    locations
}

fn split_file_uri_location(token: &str) -> Option<(&str, u64, Option<u64>)> {
    let (head, trailing) = token.rsplit_once(':')?;
    if trailing.is_empty() || !trailing.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let trailing = trailing.parse::<u64>().ok()?;
    if let Some((uri, line)) = head.rsplit_once(':') {
        if uri.starts_with("file://")
            && !line.is_empty()
            && line.chars().all(|ch| ch.is_ascii_digit())
        {
            return Some((uri, line.parse().ok()?, Some(trailing)));
        }
    }
    head.starts_with("file://")
        .then_some((head, trailing, None))
}

fn workspace_relative_file_uri(
    workspace: &crate::workspace::Workspace,
    uri: &str,
) -> Option<String> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let canonical = url.to_file_path().ok()?.canonicalize().ok()?;
    let relative = canonical.strip_prefix(workspace.root()).ok()?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let info = workspace.path_info(&path).ok()?;
    (info.kind == "file").then_some(info.path)
}

fn verification_failure_contexts(
    state: &AppState,
    report: &VerificationReport,
    locations: &[Value],
) -> Vec<Value> {
    let Ok((_workspace_id, workspace)) = state.workspaces.select(Some(&report.workspace)) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut contexts = Vec::new();
    for location in locations.iter().take(8) {
        let Some(path) = location.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Some(line) = location.get("line").and_then(Value::as_u64) else {
            continue;
        };
        if !seen.insert(path.to_owned()) {
            continue;
        }
        let Ok(line) = usize::try_from(line) else {
            continue;
        };
        let start = line.saturating_sub(3).max(1);
        let end = line.saturating_add(3);
        let Ok(view) = workspace.read_file(path, start, Some(end)) else {
            continue;
        };
        contexts.push(json!({
            "check": location.get("check").cloned().unwrap_or(Value::Null),
            "path": view.path,
            "line": line,
            "column": location.get("column").cloned().unwrap_or(Value::Null),
            "sha256": view.sha256,
            "start_line": view.start_line,
            "end_line": view.end_line,
            "total_lines": view.total_lines,
            "content": view.content,
            "redacted": view.redacted,
            "editable": !view.redacted,
            "precision": "diagnostic_text+source_window"
        }));
    }
    contexts
}

fn workspace_relative_diagnostic_path(
    workspace: &crate::workspace::Workspace,
    reported_path: &str,
) -> Option<String> {
    if reported_path.is_empty() {
        return None;
    }
    let candidate = std::path::Path::new(reported_path);
    let relative = if candidate.is_absolute() {
        let canonical = candidate.canonicalize().ok()?;
        let relative = canonical.strip_prefix(workspace.root()).ok()?;
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        reported_path.replace('\\', "/")
    };
    let info = workspace.path_info(&relative).ok()?;
    (info.kind == "file").then_some(info.path)
}

async fn augment_verification_checkpoint(
    state: &AppState,
    report: &VerificationReport,
    value: &mut Value,
) {
    if report.passed && report.level != "full" {
        return;
    }
    let (checkpoint, baseline_next_action) = if report.passed {
        ("completion_readiness", "ready_to_finish")
    } else {
        ("verification_failure", "fix_then_retry")
    };
    let failed_checks = report
        .checks
        .iter()
        .filter(|check| !check.success)
        .take(6)
        .map(|check| {
            json!({
                "id": check.id,
                "command": check.command,
                "reason": check.reason,
                "exit_code": check.exit_code,
                "stderr_tail": crate::workspace::redact_sensitive_text(&check.stderr_tail)
                    .0
                    .chars()
                    .take(2_000)
                    .collect::<String>(),
            })
        })
        .collect::<Vec<_>>();
    let checkpoint_state = json!({
        "level": report.level,
        "passed": report.passed,
        "checks_run": report.checks_run,
        "checks_reused": report.checks_reused,
        "checks_failed": report.checks_failed,
        "skipped_checks": report.skipped_checks,
        "failed_checks": failed_checks,
        "summary": report.summary.chars().take(2_000).collect::<String>()
    });
    let Ok(telemetry) =
        crate::jev::evaluate_checkpoint(checkpoint, &checkpoint_state, baseline_next_action).await
    else {
        return;
    };

    state
        .monitor
        .record_jev_decision(&report.workspace, checkpoint, &telemetry);
    let mut next_actions = crate::jev::checkpoint_actions(checkpoint, &telemetry);
    if report.passed && report.level == "full" {
        next_actions.retain(|action| action != "verify_project(level=full)");
    }
    if !next_actions.is_empty() {
        value["next_actions"] = json!(next_actions);
    }
    value["decision_assist"] = json!({
        "provider": "jev",
        "checkpoint": checkpoint,
        "authority": "increase_only",
        "next_actions": next_actions,
        "telemetry": telemetry
    });
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/checkpoint.rs"]
mod tests;
