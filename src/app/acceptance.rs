//! Local CLI entry points reuse native capture, execution and protected history.
//! No CLI flag approves Policy, grants HumanDecision, imports Evidence or
//! constructs a publishable record from JSON.

use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::verification::acceptance::{AcceptanceState, ChangeAcceptanceRecord};
use crate::verification::change::{full_oid, GitChangeTarget};
use crate::workspace::{Workspace, Workspaces};
use anyhow::{bail, ensure, Result};
use clap::Subcommand;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub(super) enum AcceptanceCommand {
    /// Inspect current native Acceptance. This does not execute project checks.
    Inspect {
        /// Complete trusted base commit SHA (40 or 64 hexadecimal characters).
        #[arg(long, value_parser = parse_sha)]
        base: String,
        /// Complete current head SHA. Omit to inspect the explicitly selected worktree.
        #[arg(long, value_parser = parse_sha)]
        head: Option<String>,
        /// Exit nonzero unless the current canonical Record is Ready.
        #[arg(long)]
        check: bool,
        /// Print the canonical Record as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create a native Verification plan from the active approved Policy.
    Plan {
        #[arg(long, value_parser = parse_sha)]
        base: String,
        #[arg(long, value_parser = parse_sha)]
        head: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Execute approved native checks and stages, then retain a guarded Record.
    Verify {
        #[arg(long, value_parser = parse_sha)]
        base: String,
        #[arg(long, value_parser = parse_sha)]
        head: Option<String>,
        /// Maximum timeout for native project checks, in seconds.
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=1800))]
        timeout_seconds: u64,
        /// Exit nonzero unless the resulting current Record is Ready.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },
    /// Retain a guarded current native Record; this does not run checks.
    Record {
        #[arg(long, value_parser = parse_sha)]
        base: String,
        #[arg(long, value_parser = parse_sha)]
        head: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Inspect bounded protected history. Historical Records never authorize a gate.
    History {
        #[arg(long)]
        json: bool,
    },
    /// Export aggregate pilot observations without uploading source or evidence.
    Metrics {
        #[arg(long)]
        json: bool,
    },
}

fn parse_sha(value: &str) -> Result<String, String> {
    if full_oid(value) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("expected a complete Git SHA (40 or 64 hexadecimal characters)".into())
    }
}

impl AcceptanceCommand {
    fn candidate(&self) -> Result<Option<(&str, GitChangeTarget)>> {
        let (base, head) = match self {
            Self::Inspect { base, head, .. }
            | Self::Plan { base, head, .. }
            | Self::Verify { base, head, .. }
            | Self::Record { base, head, .. } => (base, head),
            Self::History { .. } | Self::Metrics { .. } => return Ok(None),
        };
        // Also validate programmatically constructed commands; Clap is not the
        // authority boundary. All native probes receive typed argument values.
        ensure!(
            full_oid(base),
            "Acceptance requires a complete base Git SHA"
        );
        if let Some(head) = head {
            ensure!(
                full_oid(head),
                "Acceptance requires a complete head Git SHA"
            );
        }
        Ok(Some((
            base,
            head.as_ref().map_or(GitChangeTarget::Worktree, |revision| {
                GitChangeTarget::Commit {
                    revision: revision.to_ascii_lowercase(),
                }
            }),
        )))
    }

    fn validate_permissions(&self, workspace: &Workspace) -> Result<()> {
        if matches!(
            self,
            Self::Plan { .. } | Self::Verify { .. } | Self::Record { .. }
        ) {
            ensure!(
                workspace.write_enabled(),
                "Acceptance plan/verify/record is disabled in read-only mode"
            );
        }
        if !matches!(self, Self::History { .. } | Self::Metrics { .. }) {
            ensure!(
                workspace.exec_enabled(),
                "Acceptance requires bounded Git metadata commands; execution is disabled"
            );
        }
        if let Self::Verify {
            timeout_seconds, ..
        } = self
        {
            ensure!(
                (1..=1800).contains(timeout_seconds),
                "Acceptance timeout must be 1..1800 seconds"
            );
        }
        Ok(())
    }
}

pub(super) async fn run_acceptance_cli(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    monitor: &TaskMonitor,
    command: &AcceptanceCommand,
) -> Result<()> {
    ensure!(
        // Discovered packages belong to the explicitly selected repository;
        // they are not independent CLI candidates.
        workspaces.configured_roots().len() == 1,
        "Acceptance accepts one --workspace; inspect each selected project separately"
    );
    let (workspace_id, workspace) = workspaces.select(None)?;
    let candidate = command.candidate()?;
    command.validate_permissions(&workspace)?;
    let mut output = io::stdout().lock();
    if let AcceptanceCommand::Metrics { json } = command {
        let metrics = harness.acceptance_metrics(&workspace_id, &workspace)?;
        if !json {
            writeln!(
                output,
                "WCode local pilot metrics (retained observations; no causal savings claim)"
            )?;
        }
        writeln!(output, "{}", serde_json::to_string_pretty(&metrics)?)?;
        return Ok(());
    }
    if let AcceptanceCommand::History { json } = command {
        let history = harness.acceptance_history(&workspace_id, &workspace)?;
        return write_history(&mut output, &history, *json);
    }
    let (base, target) = candidate.expect("current Acceptance action has a candidate");
    if let AcceptanceCommand::Plan { json, .. } = command {
        let plan = harness
            .acceptance_plan(&workspace_id, &workspace, base, target)
            .await?;
        if *json {
            writeln!(output, "{}", serde_json::to_string_pretty(&plan)?)?;
        } else {
            writeln!(output, "WCode Acceptance plan {}", plan.id)?;
            writeln!(
                output,
                "  Workspace: {}  Policy: {}",
                plan.workspace, plan.policy
            )?;
            writeln!(
                output,
                "  Native checks: {}  Minimum level: {}  Reviewer jobs: {}",
                plan.required_checks.as_ref().map_or(0, Vec::len),
                plan.deterministic_level,
                plan.job_ids.len()
            )?;
            writeln!(output, "  Planning is not execution or approval.")?;
        }
        return Ok(());
    }
    let native = match command {
        AcceptanceCommand::Inspect { .. } => {
            harness
                .capture_acceptance(&workspace_id, &workspace, base, target)
                .await?
        }
        AcceptanceCommand::Record { .. } => {
            harness
                .record_acceptance(&workspace_id, &workspace, base, target)
                .await?
        }
        AcceptanceCommand::Verify {
            timeout_seconds, ..
        } => {
            harness
                .acceptance_verify(
                    &workspace_id,
                    &workspace,
                    base,
                    target,
                    *timeout_seconds,
                    monitor,
                )
                .await?
        }
        _ => unreachable!("History and Plan return before current Record capture"),
    };
    let (json, check) = match command {
        AcceptanceCommand::Inspect { json, check, .. }
        | AcceptanceCommand::Verify { json, check, .. } => (*json, *check),
        AcceptanceCommand::Record { json, .. } => (*json, false),
        _ => unreachable!("only Record-producing actions remain"),
    };
    write_record(&mut output, native.record(), json)?;
    output.flush()?;
    if check && native.record().state != AcceptanceState::Ready {
        bail!(
            "current Change Acceptance is {:?}; this candidate cannot pass the gate",
            native.record().state
        );
    }
    Ok(())
}

fn write_record(
    writer: &mut impl Write,
    record: &ChangeAcceptanceRecord,
    json: bool,
) -> Result<()> {
    if json {
        writeln!(writer, "{}", serde_json::to_string_pretty(record)?)?;
        return Ok(());
    }
    writeln!(
        writer,
        "WCode Change Acceptance {:?}  {}",
        record.state, record.id
    )?;
    writeln!(
        writer,
        "  Workspace: {}  Code: {}  Design: {}",
        record.workspace,
        record.revision.code,
        record.revision.design.as_deref().unwrap_or("unbound")
    )?;
    writeln!(
        writer,
        "  Git base: {}  Head: {}  Capture complete: {}",
        record.git.base_sha.as_deref().unwrap_or("unknown"),
        record.git.target_sha.as_deref().unwrap_or("unknown"),
        record.git.complete
    )?;
    writeln!(
        writer,
        "  Required: {}  Executed: {}  Pass: {}  Fail: {}  Skipped: {}  Unavailable: {}",
        record.summary.required,
        record.summary.executed,
        record.summary.passed,
        record.summary.failed,
        record.summary.skipped,
        record.summary.unavailable
    )?;
    for reason in &record.reasons {
        writeln!(
            writer,
            "  {:?}: {}{}",
            reason.action,
            reason.code,
            reason
                .subject
                .as_ref()
                .map_or(String::new(), |subject| format!(" ({subject})"))
        )?;
    }
    if record.reasons.is_empty() {
        writeln!(
            writer,
            "  Current native checks and all required gates are satisfied."
        )?;
    }
    Ok(())
}

fn write_history(writer: &mut impl Write, history: &Value, json: bool) -> Result<()> {
    ensure!(
        history["authority"] == "historical_only" && history["current_acceptance"] == false,
        "Acceptance history has an invalid authority projection"
    );
    if json {
        writeln!(writer, "{}", serde_json::to_string_pretty(history)?)?;
    } else {
        writeln!(
            writer,
            "WCode Acceptance history — historical only, not current gate authority"
        )?;
        writeln!(
            writer,
            "  Workspace: {}  Retained: {} / {}",
            history["workspace"].as_str().unwrap_or("unknown"),
            history["retained"].as_u64().unwrap_or(0),
            history["capacity"].as_u64().unwrap_or(0)
        )?;
        for record in history["records"].as_array().into_iter().flatten() {
            writeln!(
                writer,
                "  {}  Historical state: {}  Code: {}",
                record["id"].as_str().unwrap_or("unknown"),
                record["state"].as_str().unwrap_or("unknown"),
                record["revision"]["code"].as_str().unwrap_or("unknown")
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/app/acceptance.rs"]
mod tests;
