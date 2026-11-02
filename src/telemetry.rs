//! Opt-in local telemetry export.
//!
//! There is no default collection upload and no background sender: the only
//! way to produce telemetry output is an explicit operator command. Exported
//! events never contain credentials, tokens, raw source, prompts or query text.

use crate::engineering_journal::{self, EngineeringMilestone};
use crate::evidence_store::workspace_root_key;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const TELEMETRY_SCHEMA_VERSION: u32 = 1;
const MAX_EVENTS: usize = 512;
const MAX_TOOL: usize = 80;
const MAX_STAGE: usize = 32;
const MAX_OUTCOME: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TelemetryKind {
    Command,
    Verification,
}

impl TelemetryKind {
    fn of(milestone: &EngineeringMilestone) -> Self {
        if milestone.tool == "verify_project" || milestone.stage == "prove" {
            Self::Verification
        } else {
            Self::Command
        }
    }
}

/// One exported observation. Field names and bounds are the stable contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelemetryEvent {
    pub version: u32,
    pub timestamp_ms: u64,
    pub kind: TelemetryKind,
    pub tool: String,
    pub stage: String,
    pub outcome: String,
    pub duration_ms: u64,
    /// Non-reversible workspace key, never a path.
    pub workspace_key: String,
    /// Correlates with the retained journal record; not a secret.
    pub event_id: String,
}

impl TelemetryEvent {
    fn from_milestone(milestone: &EngineeringMilestone, workspace_key: &str) -> Result<Self> {
        let event_id = milestone
            .event_id
            .clone()
            .context("telemetry event requires a journal event id")?;
        let event = Self {
            version: TELEMETRY_SCHEMA_VERSION,
            timestamp_ms: milestone.timestamp_ms,
            kind: TelemetryKind::of(milestone),
            tool: milestone.tool.clone(),
            stage: milestone.stage.clone(),
            outcome: milestone.outcome.clone(),
            duration_ms: milestone.duration_ms,
            workspace_key: workspace_key.to_owned(),
            event_id,
        };
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<()> {
        if self.version != TELEMETRY_SCHEMA_VERSION
            || !bounded(&self.tool, MAX_TOOL)
            || !bounded(&self.stage, MAX_STAGE)
            || !bounded(&self.outcome, MAX_OUTCOME)
            || !bounded(&self.workspace_key, 128)
            || !bounded(&self.event_id, 64)
        {
            bail!("invalid telemetry event");
        }
        Ok(())
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

/// Load the retained journal window and map it into the telemetry schema.
pub(crate) fn export(workspace: &Workspace, limit: usize) -> Result<Vec<TelemetryEvent>> {
    let limit = limit.clamp(1, MAX_EVENTS);
    let history = engineering_journal::load_recent(workspace, limit)?;
    let workspace_key = workspace_root_key(workspace.root())?;
    history
        .records
        .iter()
        .map(|record| TelemetryEvent::from_milestone(record, &workspace_key))
        .collect()
}

pub(crate) fn encode(events: &[TelemetryEvent]) -> Result<String> {
    let mut out = String::new();
    for event in events {
        out.push_str(&serde_json::to_string(event).context("cannot encode telemetry event")?);
        out.push('\n');
    }
    Ok(out)
}

pub(crate) fn write_to(path: &Path, events: &[TelemetryEvent]) -> Result<()> {
    let encoded = encode(events)?;
    std::fs::write(path, encoded).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
#[path = "../tests/unit/telemetry.rs"]
mod tests;
