use super::*;

pub(super) struct AttentionSignal {
    pub(super) severity: String,
    pub(super) subject: String,
    pub(super) message: String,
    pub(super) provenance: String,
    pub(super) action: String,
    pub(super) target: ConsoleTab,
    pub(super) task_id: Option<u64>,
    pub(super) authorization: bool,
}

pub(super) fn attention_signals(
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
) -> Vec<AttentionSignal> {
    let mut signals = Vec::new();
    let mut local = |severity: &str,
                     subject: &str,
                     message: String,
                     action: &str,
                     target,
                     task_id,
                     authorization| {
        signals.push(AttentionSignal {
            severity: severity.to_owned(),
            subject: subject.to_owned(),
            message,
            provenance: "runtime monitor · observed".to_owned(),
            action: action.to_owned(),
            target,
            task_id,
            authorization,
        });
    };
    let approvals = ui
        .pending_authorizations
        .iter()
        .filter(|request| request.workspace == workspace)
        .count();
    if approvals > 0 {
        local(
            "high",
            "Authorization pending",
            format!("{approvals} operation(s) await an operator decision."),
            "Enter opens the authorization surface; inspect the selected operation.",
            ConsoleTab::Summary,
            None,
            true,
        );
    }
    if snapshot.tunnel_running == Some(false) {
        local(
            "high",
            "Tunnel process exited",
            snapshot
                .tunnel_error
                .clone()
                .unwrap_or_else(|| "The provider process is no longer running.".to_owned()),
            "Inspect provider recovery and health probes.",
            ConsoleTab::Providers,
            None,
            false,
        );
    } else if snapshot.public_url_healthy == Some(false) {
        local(
            "high",
            "Public endpoint unavailable",
            snapshot
                .public_url_error
                .clone()
                .unwrap_or_else(|| "No verified public endpoint is available.".to_owned()),
            "Inspect provider recovery and health probes.",
            ConsoleTab::Providers,
            None,
            false,
        );
    }
    for task in console_tasks(snapshot, workspace)
        .into_iter()
        .filter(|task| task.status == TaskStatus::Failed)
    {
        local(
            "high",
            "Tool call failed",
            format!(
                "#{} {} · {}",
                task.id,
                task.tool,
                truncate_end(&task.detail, 240)
            ),
            "Enter opens retained task details; inspect the tool result before retrying.",
            ConsoleTab::Tasks,
            Some(task.id),
            false,
        );
    }
    if let Some(stats) = snapshot.intelligence.get(workspace) {
        if let Some(error) = stats.refresh_error.as_deref() {
            local(
                "medium",
                "Partial refresh",
                truncate_end(error, 400),
                "R refreshes the current workspace observation.",
                ConsoleTab::Summary,
                None,
                false,
            );
        }
        if stats
            .project_observed_at
            .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300))
        {
            local("medium", "Project snapshot aging", "The project observation is older than five minutes; its revision may differ from the working tree.".to_owned(),
                "R refreshes the current workspace observation.", ConsoleTab::Summary, None, false);
        }
        if stats.lsp_stale > 0 {
            local("medium", "Semantic evidence stale", format!("{} stale provider snapshot(s); syntax facts retain syntax precision.", stats.lsp_stale),
                "Inspect provider freshness; semantic_status and semantic_navigation expose current provenance.", ConsoleTab::Providers, None, false);
        }
        // The shared project projection owns proof/trace/drift findings. Scalar
        // fallback is only used before that bounded snapshot becomes available.
        if stats.project_attention.is_none() {
            // Unobserved evidence is not a clean slate: only report failures once
            // the counts exist, so the fallback never reads as a measured zero.
            if let (Some(failed), Some(disagreed)) =
                (stats.evidence_failed, stats.evidence_disagreed)
            {
                if failed > 0 || disagreed > 0 {
                    local("high", "Proof needs attention", format!("{failed} failed · {disagreed} disagreed evidence item(s)."),
                        "evidence_status shows producer and revision; review_changes then verify_project refreshes checks.", ConsoleTab::Summary, None, false);
                }
            }
            if stats.verification_ready == Some(false) {
                local(
                    "medium",
                    "Verification blocked",
                    format!(
                        "{} blocker(s) in the latest observed verification plan.",
                        stats.verification_blockers.unwrap_or(0)
                    ),
                    "verification_status exposes plan blockers and revision.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
            if stats.policy_errors > 0 {
                local(
                    "high",
                    "Policy findings",
                    format!(
                        "{} error(s) · {} warning(s).",
                        stats.policy_errors, stats.policy_warnings
                    ),
                    "convention_status exposes exact policy locations.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
            if let Some(findings) = stats.drift_findings.filter(|findings| *findings > 0) {
                local(
                    "medium",
                    "Design drift",
                    format!(
                        "{findings} observed finding(s) · risk {}.",
                        stats.risk_level.as_deref().unwrap_or("unassessed")
                    ),
                    "drift_status and risk_status expose affected subjects.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
        }
        if let Some(items) = stats
            .project_attention
            .as_ref()
            .and_then(|attention| attention["items"].as_array())
        {
            for issue in items.iter().take(200) {
                let section = json_text(issue, "section", "overview");
                let subject = json_text(issue, "subject", "project");
                let location = issue["path"]
                    .as_str()
                    .or_else(|| issue["requirement"].as_str());
                signals.push(AttentionSignal {
                    severity: json_text(issue, "severity", "info").to_owned(),
                    subject: subject.to_owned(),
                    message: format!(
                        "{}{}",
                        json_text(issue, "message", "Observed project finding"),
                        location
                            .map(|location| format!(" · {location}"))
                            .unwrap_or_default()
                    ),
                    provenance: format!(
                        "{} · {} · cached project snapshot",
                        json_text(issue, "provider", "project_observatory"),
                        json_text(issue, "precision", "unknown")
                    ),
                    action: format!("W opens the workspace observatory; inspect {section}."),
                    target: ConsoleTab::Summary,
                    task_id: None,
                    authorization: false,
                });
            }
        }
    }
    signals.sort_by_key(|signal| severity_rank(&signal.severity));
    signals
}
