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
    for result in &summary.results {
        let target = result.target.as_deref().unwrap_or("manual");
        println!(
            "  {:>18}  {:<28}  {}",
            status_label(result.status),
            result.host,
            target
        );
        if !result.evidence.is_empty() {
            println!("  {:>18}  {}", "evidence", result.evidence.join("; "));
        }
        if matches!(
            result.status,
            AgentInstallStatus::Manual | AgentInstallStatus::Failed
        ) {
            println!("  {:>18}  {}", "guidance", result.detail);
        }
    }
    println!(
        "Summary: detected={} planned={} installed={} updated={} already={} manual={} unsupported={} failed={}",
        summary.detected.len(),
        summary.planned.len(),
        summary.installed.len(),
        summary.updated.len(),
        summary.already_configured.len(),
        summary.manual.len(),
        summary.unsupported.len(),
        summary.failed.len()
    );
}

fn status_label(status: AgentInstallStatus) -> &'static str {
    match status {
        AgentInstallStatus::Planned => "planned",
        AgentInstallStatus::Installed => "installed",
        AgentInstallStatus::Updated => "updated",
        AgentInstallStatus::AlreadyConfigured => "already_configured",
        AgentInstallStatus::Manual => "manual",
        AgentInstallStatus::Unsupported => "unsupported",
        AgentInstallStatus::Failed => "failed",
    }
}
