mod adapters;
mod merge;
mod report;

use crate::agent_plugin;
use crate::workspace::Workspace;
use adapters::{AdapterKind, AgentHost, HOSTS};
use merge::{FileOutcome, PlannedFile};
pub(crate) use report::print_human;
use report::{
    AgentInstallAction, AgentInstallDisposition, AgentInstallResult, AgentInstallStatus,
    AgentInstallSummary, DetectedHost,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub(crate) struct AgentInstallPlan {
    pub workspace: String,
    pub actions: Vec<AgentInstallAction>,
    #[serde(skip)]
    writes: BTreeMap<String, PlannedFile>,
}

pub(crate) fn detect_hosts(workspace: &Workspace) -> Vec<DetectedHost> {
    HOSTS
        .iter()
        .map(|host| {
            let evidence = detection_evidence(workspace.root(), host);
            DetectedHost {
                id: host.id.to_owned(),
                name: host.name.to_owned(),
                detected: !evidence.is_empty(),
                evidence,
            }
        })
        .collect()
}

pub(crate) fn plan_install(workspace: &Workspace) -> AgentInstallPlan {
    let detections = detect_hosts(workspace)
        .into_iter()
        .map(|host| (host.id.clone(), host))
        .collect::<BTreeMap<_, _>>();
    let server = agent_plugin::local_stdio_server(workspace.root());
    let mut writes = BTreeMap::<String, PlannedFile>::new();
    let mut actions = Vec::new();

    for host in HOSTS {
        let detected = detections.get(host.id).expect("host detection exists");
        let base = || AgentInstallAction {
            host_id: host.id.to_owned(),
            host: host.name.to_owned(),
            detected: detected.detected,
            evidence: detected.evidence.clone(),
            target: None,
            method: "manual setup".to_owned(),
            disposition: AgentInstallDisposition::Manual,
            guidance: host.guidance.to_owned(),
        };
        if matches!(host.adapter, AdapterKind::Manual) {
            actions.push(base());
            continue;
        }
        if !detected.detected {
            actions.push(AgentInstallAction {
                disposition: AgentInstallDisposition::Unsupported,
                method: "no project-local change".to_owned(),
                guidance: "Host was not detected; no configuration was created.".to_owned(),
                ..base()
            });
            continue;
        }
        let plan = match (&host.adapter, &server) {
            (_, Err(error)) => Err(anyhow::anyhow!(error.to_string())),
            (AdapterKind::JsonMcpServers { path }, Ok(server)) => {
                merge::plan_json(workspace, path, "mcpServers", server)
            }
            (AdapterKind::JsonServers { path }, Ok(server)) => {
                merge::plan_json(workspace, path, "servers", server)
            }
            (AdapterKind::CodexToml, Ok(_)) => {
                merge::plan_codex_toml(workspace, ".codex/config.toml", workspace.root())
            }
            (AdapterKind::OpenCode, Ok(_)) => {
                merge::plan_opencode(workspace, "opencode.json", workspace.root())
            }
            (AdapterKind::Manual, _) => unreachable!(),
        };
        match plan {
            Ok(plan) => {
                let target = plan.target.clone();
                let method = plan.method.clone();
                let disposition = disposition(plan.outcome);
                writes.entry(target.clone()).or_insert(plan);
                actions.push(AgentInstallAction {
                    target: Some(target),
                    method,
                    disposition,
                    ..base()
                });
            }
            Err(error) => actions.push(AgentInstallAction {
                method: "safe config merge rejected".to_owned(),
                disposition: AgentInstallDisposition::Failed,
                guidance: format!("{} ({error:#})", host.guidance),
                ..base()
            }),
        }
    }

    add_skill_plan(workspace, &detections, &mut writes, &mut actions);
    AgentInstallPlan {
        workspace: workspace.root().to_string_lossy().into_owned(),
        actions,
        writes,
    }
}

pub(crate) fn apply_install(
    workspace: &Workspace,
    plan: AgentInstallPlan,
    dry_run: bool,
) -> AgentInstallSummary {
    let mut write_errors = BTreeMap::<String, String>::new();
    if !dry_run {
        for (target, write) in &plan.writes {
            if let Err(error) = merge::apply(workspace, write) {
                write_errors.insert(target.clone(), format!("{error:#}"));
            }
        }
    }

    let results = plan
        .actions
        .into_iter()
        .map(|action| {
            let write_error = action
                .target
                .as_ref()
                .and_then(|target| write_errors.get(target));
            let status = if write_error.is_some() {
                AgentInstallStatus::Failed
            } else {
                report::result_status(action.disposition, dry_run)
            };
            AgentInstallResult {
                host_id: action.host_id,
                host: action.host,
                detected: action.detected,
                evidence: action.evidence,
                target: action.target,
                method: action.method,
                status,
                detail: write_error.cloned().unwrap_or(action.guidance),
            }
        })
        .collect::<Vec<_>>();
    report::summarize(dry_run, plan.workspace, results)
}

fn add_skill_plan(
    workspace: &Workspace,
    detections: &BTreeMap<String, DetectedHost>,
    writes: &mut BTreeMap<String, PlannedFile>,
    actions: &mut Vec<AgentInstallAction>,
) {
    let detected_names = HOSTS
        .iter()
        .filter(|host| !matches!(host.adapter, AdapterKind::Manual))
        .filter_map(|host| detections.get(host.id))
        .filter(|host| host.detected)
        .map(|host| host.name.clone())
        .collect::<Vec<_>>();
    if detected_names.is_empty() {
        return;
    }
    add_skill_action(
        workspace,
        ".agents/skills/wcode-software-intelligence/SKILL.md",
        "portable-agent-skill",
        "Portable Agent Skill",
        detected_names,
        writes,
        actions,
    );
    if detections
        .get("claude-code")
        .is_some_and(|host| host.detected)
    {
        add_skill_action(
            workspace,
            ".claude/skills/wcode-software-intelligence/SKILL.md",
            "claude-skill",
            "Claude Code Skill",
            vec!["Claude Code detected".to_owned()],
            writes,
            actions,
        );
    }
}

fn add_skill_action(
    workspace: &Workspace,
    target: &str,
    host_id: &str,
    host: &str,
    evidence: Vec<String>,
    writes: &mut BTreeMap<String, PlannedFile>,
    actions: &mut Vec<AgentInstallAction>,
) {
    match merge::plan_canonical_text(workspace, target, agent_plugin::canonical_skill()) {
        Ok(plan) => {
            let disposition = disposition(plan.outcome);
            let method = plan.method.clone();
            writes.insert(target.to_owned(), plan);
            actions.push(AgentInstallAction {
                host_id: host_id.to_owned(),
                host: host.to_owned(),
                detected: true,
                evidence,
                target: Some(target.to_owned()),
                method,
                disposition,
                guidance: if disposition == AgentInstallDisposition::Manual {
                    "Existing skill differs from wcode's canonical source; review it manually."
                        .to_owned()
                } else {
                    "Project-local skill; no hooks, scripts, or credentials are installed."
                        .to_owned()
                },
            });
        }
        Err(error) => actions.push(AgentInstallAction {
            host_id: host_id.to_owned(),
            host: host.to_owned(),
            detected: true,
            evidence,
            target: Some(target.to_owned()),
            method: "safe skill install rejected".to_owned(),
            disposition: AgentInstallDisposition::Failed,
            guidance: format!("{error:#}"),
        }),
    }
}

fn disposition(outcome: FileOutcome) -> AgentInstallDisposition {
    match outcome {
        FileOutcome::Create => AgentInstallDisposition::Install,
        FileOutcome::Update => AgentInstallDisposition::Update,
        FileOutcome::Already => AgentInstallDisposition::AlreadyConfigured,
        FileOutcome::ManualConflict => AgentInstallDisposition::Manual,
    }
}

fn detection_evidence(root: &Path, host: &AgentHost) -> Vec<String> {
    let mut evidence = Vec::new();
    for binary in host.binaries {
        if let Some(path) = find_executable(binary) {
            evidence.push(format!("executable {}", path.display()));
            break;
        }
    }
    for marker in host.project_markers {
        let path = root.join(marker);
        if path.exists() {
            evidence.push(format!("project path {}", path.display()));
            break;
        }
    }
    #[cfg(target_os = "macos")]
    for app in host.mac_apps {
        let path = Path::new("/Applications").join(app);
        if path.exists() {
            evidence.push(format!("application {}", path.display()));
            break;
        }
    }
    evidence
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let candidates = if cfg!(windows) {
        vec![
            name.to_owned(),
            format!("{name}.exe"),
            format!("{name}.cmd"),
        ]
    } else {
        vec![name.to_owned()]
    };
    env::split_paths(&path)
        .flat_map(|directory| candidates.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/agent/install.rs"]
mod tests;
