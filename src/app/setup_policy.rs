//! Repository draft suggestions only. Setup never consumes an operator grant,
//! activates Policy, executes a check, or treats installation consent as approval.
use crate::design::{self, AcceptancePolicy, PolicyLevel, PolicyRequirements, ProjectDesign};
use crate::harness::ToolHarness;
use crate::workspace::{TextEdit, Workspace};
use anyhow::{bail, Result};
use serde::Serialize;
use std::io::{BufRead, Write};

const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_CHECKS: usize = 32;

#[derive(Debug, Serialize)]
pub(super) struct SuggestedCheck {
    pub id: String,
    pub level: String,
}
#[derive(Debug, Serialize)]
pub(super) struct Suggestion {
    pub status: &'static str,
    pub authority: &'static str,
    pub activation_required: bool,
    pub provider: &'static str,
    pub checks: Vec<SuggestedCheck>,
    pub diagnostics: Vec<&'static str>,
    pub policy_yaml: Option<String>,
    #[serde(skip)]
    edit: Option<DraftEdit>,
}
#[derive(Debug)]
struct DraftEdit {
    original: String,
    updated: String,
    sha256: String,
}

pub(super) fn suggest(workspace: &Workspace) -> Result<Suggestion> {
    let mut suggestion = Suggestion {
        status: "unavailable",
        authority: "repository_draft_only",
        activation_required: true,
        provider: provider_hint(workspace),
        checks: Vec::new(),
        diagnostics: Vec::new(),
        policy_yaml: None,
        edit: None,
    };
    let harness = ToolHarness::new(2)?;
    let context = match harness.project_context("setup-project", workspace) {
        Ok(context) => context,
        Err(_) => {
            suggestion
                .diagnostics
                .push("Native project metadata is unavailable; no draft is writable.");
            return Ok(suggestion);
        }
    };
    for check in &context.recommended_checks {
        if suggestion.checks.len() == MAX_CHECKS
            || check.id.is_empty()
            || check.id.len() > 160
            || check.id.chars().any(char::is_control)
            || !matches!(check.level.as_str(), "quick" | "full")
            || suggestion.checks.iter().any(|seen| seen.id == check.id)
        {
            suggestion.diagnostics.push("Check inventory exceeds its bound or has an unknown binding; inspect it before drafting.");
            return Ok(suggestion);
        }
        suggestion.checks.push(SuggestedCheck {
            id: check.id.clone(),
            level: check.level.clone(),
        });
    }
    if !context.discovery.complete || suggestion.checks.is_empty() {
        suggestion
            .diagnostics
            .push("Complete native check discovery is required before generating a Policy draft.");
        return Ok(suggestion);
    }
    let load = design::load_design(workspace)?;
    if !load.initialized || load.error_count() != 0 || load.state.project.is_none() {
        suggestion.status = "design_required";
        suggestion
            .diagnostics
            .push("Initialize or repair Design State with design_init before drafting Policy.");
        return Ok(suggestion);
    }
    let source = workspace.load_source(design::PROJECT_FILE)?;
    if source.content.len() > MAX_CONFIG_BYTES || source.readonly {
        suggestion
            .diagnostics
            .push("Project Design is too large or read-only; use the guarded source editor.");
        return Ok(suggestion);
    }
    let value: serde_yaml::Value = serde_yaml::from_str(&source.content)?;
    let key = serde_yaml::Value::String("acceptance_policy".into());
    if value
        .as_mapping()
        .is_none_or(|mapping| mapping.contains_key(&key))
    {
        suggestion.status = "existing_policy_field";
        suggestion.diagnostics.push("An existing Policy field is preserved; inspect native preview/status and edit it explicitly.");
        return Ok(suggestion);
    }
    let policy = AcceptancePolicy {
        schema_version: 1,
        id: "local-baseline".into(),
        version: 1,
        requirements: PolicyRequirements {
            minimum_level: PolicyLevel::Full,
            checks: suggestion
                .checks
                .iter()
                .map(|check| check.id.clone())
                .collect(),
            stages: Vec::new(),
            reviewers: Vec::new(),
            human_approval: true,
            human_approval_min_risk: None,
        },
        docs_only: None,
        rules: Vec::new(),
    };
    if !policy.validate(&load.state).is_empty() {
        suggestion
            .diagnostics
            .push("The suggested Policy cannot be validated against current Design State.");
        return Ok(suggestion);
    }
    let policy_yaml = serde_yaml::to_string(&policy)?;
    let field = serde_yaml::to_string(&serde_json::json!({"acceptance_policy":policy}))?;
    let updated = match append_field(&source.content, &field) {
        Some(updated) => updated,
        None => {
            suggestion.diagnostics.push(
                "Project YAML format requires a manual guarded edit; its bytes are preserved.",
            );
            return Ok(suggestion);
        }
    };
    let parsed: ProjectDesign = serde_yaml::from_str(&updated)?;
    if parsed.name != load.state.project.as_ref().unwrap().name
        || parsed.description != load.state.project.as_ref().unwrap().description
        || parsed.acceptance_policy.as_ref() != Some(&policy)
    {
        bail!("suggested draft changed existing project fields");
    }
    suggestion.status = "draft_available";
    suggestion.policy_yaml = Some(policy_yaml);
    suggestion.edit = Some(DraftEdit {
        original: source.content,
        updated,
        sha256: source.sha256,
    });
    Ok(suggestion)
}

// Append one root field without rewriting existing content, comments or line endings.
// Flow-style mappings and mixed line endings require explicit manual editing.
fn append_field(original: &str, field: &str) -> Option<String> {
    let crlf = original.matches("\r\n").count();
    let lf = original.bytes().filter(|byte| *byte == b'\n').count();
    if crlf != 0 && crlf != lf || original.matches('\r').count() != crlf {
        return None;
    }
    let newline = if crlf == 0 { "\n" } else { "\r\n" };
    let field = field.replace('\n', newline);
    let mut offset = 0;
    let mut end_marker = None;
    for line in original.split_inclusive('\n') {
        if line.trim() == "..." {
            end_marker = Some(offset);
            break;
        }
        offset += line.len();
    }
    let position = end_marker.unwrap_or(original.len());
    let mut updated = original[..position].to_owned();
    if !updated.ends_with('\n') {
        updated.push_str(newline);
    }
    updated.push_str(&field);
    updated.push_str(&original[position..]);
    serde_yaml::from_str::<ProjectDesign>(&updated).ok()?;
    Some(updated)
}

pub(super) fn print(suggestion: &Suggestion, output: &mut impl Write) -> Result<()> {
    writeln!(
        output,
        "\nAcceptance Policy suggestion: {} · provider {}",
        suggestion.status, suggestion.provider
    )?;
    for diagnostic in &suggestion.diagnostics {
        writeln!(output, "  {diagnostic}")?;
    }
    if let Some(policy) = &suggestion.policy_yaml {
        writeln!(
            output,
            "Repository draft for {} (not active):\n{policy}",
            design::PROJECT_FILE
        )?;
        if let Some(edit) = &suggestion.edit {
            writeln!(output, "Reviewed source SHA256: {}", edit.sha256)?;
        }
    }
    writeln!(output, "Activation: request native acceptance_policy preview, then activate with its snapshot digest/current generation; approve the exact request in the TUI or protected WebUI and retry from the same requester.")?;
    writeln!(
        output,
        "Installation and draft consent do not activate Policy or approve a change."
    )?;
    Ok(())
}

pub(super) fn review_with_io(
    workspace: &Workspace,
    suggestion: &Suggestion,
    interactive: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool> {
    let Some(edit) = &suggestion.edit else {
        return Ok(false);
    };
    if !interactive {
        bail!("Policy draft writes require an interactive terminal review");
    }
    write!(
        output,
        "Write this repository Policy draft? This does not activate it. [y/N]: "
    )?;
    output.flush()?;
    let mut answer = String::new();
    if input.read_line(&mut answer)? == 0
        || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    {
        writeln!(output, "Policy draft was not written.")?;
        return Ok(false);
    }
    workspace.apply_edits(
        design::PROJECT_FILE,
        &[TextEdit {
            old_text: edit.original.clone(),
            new_text: edit.updated.clone(),
            start_line: None,
            end_line: None,
        }],
        &edit.sha256,
    )?;
    writeln!(output, "Repository draft written; Policy remains inactive until an explicit native operator decision.")?;
    Ok(true)
}

fn provider_hint(workspace: &Workspace) -> &'static str {
    let Ok(info) = workspace.path_info(".git/config") else {
        return "unknown";
    };
    if info.kind != "file" || info.size > MAX_CONFIG_BYTES as u64 {
        return "unknown";
    }
    let Ok(source) = workspace.load_source(".git/config") else {
        return "unknown";
    };
    if source.content.len() > MAX_CONFIG_BYTES || source.content.contains('\0') {
        return "unknown";
    }
    let mut remote = false;
    let mut detected = None;
    for line in source.content.lines() {
        if line.len() > 4096 {
            return "unknown";
        }
        let line = line.trim();
        if line.starts_with('[') {
            remote = line.starts_with("[remote \"") && line.ends_with("\"]");
            continue;
        }
        if !remote {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "url" {
            continue;
        }
        let provider = classify_remote(value.trim().trim_matches('"'));
        if detected.is_some_and(|seen| seen != provider) {
            return "unknown";
        }
        detected = Some(provider);
    }
    detected.unwrap_or("unknown")
}

// Never retain or emit a remote URL, username, password, query or SSH identity.
fn classify_remote(value: &str) -> &'static str {
    let host = if let Ok(url) = url::Url::parse(value) {
        url.host_str().map(str::to_owned)
    } else {
        value
            .split_once('@')
            .and_then(|(_, tail)| tail.split_once(':'))
            .map(|(host, _)| host.to_owned())
    };
    match host.as_deref() {
        Some(host) if host.eq_ignore_ascii_case("github.com") => "github",
        Some(host) if host.eq_ignore_ascii_case("gitlab.com") => "gitlab",
        Some(_) => "other",
        None => "unknown",
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/setup_policy.rs"]
mod tests;
