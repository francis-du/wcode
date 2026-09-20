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
    augment_verification_checkpoint(state, &report, &mut value).await;
    Ok(tool_result(value, is_error))
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
