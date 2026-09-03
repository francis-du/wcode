use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DetectedHost {
    pub id: String,
    pub name: String,
    pub detected: bool,
    pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentInstallDisposition {
    Install,
    Update,
    AlreadyConfigured,
    Manual,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AgentInstallAction {
    pub host_id: String,
    pub host: String,
    pub detected: bool,
    pub evidence: Vec<String>,
    pub target: Option<String>,
    pub method: String,
    pub disposition: AgentInstallDisposition,
    pub guidance: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentInstallStatus {
    Planned,
    Installed,
    Updated,
    AlreadyConfigured,
    Manual,
    Unsupported,
    Failed,
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentInstallResult {
    pub host_id: String,
    pub host: String,
    pub detected: bool,
    pub evidence: Vec<String>,
    pub target: Option<String>,
    pub method: String,
    pub status: AgentInstallStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentInstallSummary {
    pub dry_run: bool,
    pub scope: String,
    pub workspace: String,
    pub detected: Vec<String>,
    pub planned: Vec<String>,
    pub installed: Vec<String>,
    pub updated: Vec<String>,
    pub already_configured: Vec<String>,
    pub manual: Vec<String>,
    pub unsupported: Vec<String>,
    pub failed: Vec<String>,
    pub results: Vec<AgentInstallResult>,
}

pub(crate) fn result_status(
    disposition: AgentInstallDisposition,
    dry_run: bool,
) -> AgentInstallStatus {
    match disposition {
        AgentInstallDisposition::Install | AgentInstallDisposition::Update if dry_run => {
            AgentInstallStatus::Planned
        }
        AgentInstallDisposition::Install => AgentInstallStatus::Installed,
        AgentInstallDisposition::Update => AgentInstallStatus::Updated,
        AgentInstallDisposition::AlreadyConfigured => AgentInstallStatus::AlreadyConfigured,
        AgentInstallDisposition::Manual => AgentInstallStatus::Manual,
        AgentInstallDisposition::Unsupported => AgentInstallStatus::Unsupported,
        AgentInstallDisposition::Failed => AgentInstallStatus::Failed,
    }
}

pub(crate) fn summarize(
    dry_run: bool,
    scope: String,
    workspace: String,
    results: Vec<AgentInstallResult>,
) -> AgentInstallSummary {
    let collect = |status| {
        results
            .iter()
            .filter(|result| result.status == status)
            .map(|result| result.host.clone())
            .collect::<Vec<_>>()
    };
    AgentInstallSummary {
        dry_run,
        scope,
        workspace,
        detected: results
            .iter()
            .filter(|result| result.detected)
            .map(|result| result.host.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        planned: collect(AgentInstallStatus::Planned),
        installed: collect(AgentInstallStatus::Installed),
        updated: collect(AgentInstallStatus::Updated),
        already_configured: collect(AgentInstallStatus::AlreadyConfigured),
        manual: collect(AgentInstallStatus::Manual),
        unsupported: collect(AgentInstallStatus::Unsupported),
        failed: collect(AgentInstallStatus::Failed),
        results,
    }
}

pub(crate) fn print_human(summary: &AgentInstallSummary) {
    println!("\n  Scope: {} · Root: {}", summary.scope, summary.workspace);
    println!(
        "  ┌──────────┬──────────────────────────────┬──────────────────────────────────────┐"
    );
    println!(
        "  │ STATUS   │ HOST                         │ TARGET                               │"
    );
    println!(
        "  ├──────────┼──────────────────────────────┼──────────────────────────────────────┤"
    );
    let visible = summary
        .results
        .iter()
        .filter(|result| result.detected || result.status != AgentInstallStatus::Unsupported)
        .collect::<Vec<_>>();
    for result in &visible {
        let target = result.target.as_deref().unwrap_or("manual / host UI");
        println!(
            "  │ {:<8} │ {:<28} │ {:<36} │",
            status_label(result.status),
            truncate(&result.host, 28),
            truncate(target, 36)
        );
    }
    println!(
        "  └──────────┴──────────────────────────────┴──────────────────────────────────────┘"
    );
    for result in visible {
        if matches!(
            result.status,
            AgentInstallStatus::Manual | AgentInstallStatus::Failed
        ) {
            println!(
                "  {} {}: {}",
                status_icon(result.status),
                result.host,
                result.detail
            );
        }
    }
    println!(
        "\n  Summary: detected={} planned={} installed={} updated={} ready={} manual={} failed={} ({} undiscovered omitted)",
        summary.detected.len(),
        summary.planned.len(),
        summary.installed.len(),
        summary.updated.len(),
        summary.already_configured.len(),
        summary.manual.len(),
        summary.failed.len(),
        summary.unsupported.len()
    );
}

fn truncate(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    let mut output = value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

fn status_icon(status: AgentInstallStatus) -> &'static str {
    match status {
        AgentInstallStatus::Planned => "○",
        AgentInstallStatus::Installed | AgentInstallStatus::AlreadyConfigured => "✓",
        AgentInstallStatus::Updated => "↻",
        AgentInstallStatus::Manual => "!",
        AgentInstallStatus::Unsupported => "·",
        AgentInstallStatus::Failed => "×",
    }
}

fn status_label(status: AgentInstallStatus) -> &'static str {
    match status {
        AgentInstallStatus::Planned => "○ plan",
        AgentInstallStatus::Installed => "✓ install",
        AgentInstallStatus::Updated => "↻ update",
        AgentInstallStatus::AlreadyConfigured => "✓ ready",
        AgentInstallStatus::Manual => "! manual",
        AgentInstallStatus::Unsupported => "· absent",
        AgentInstallStatus::Failed => "× failed",
    }
}
