use super::*;
use crate::evidence::Revision;
use crate::harness::ToolHarness;

const CLAIM_LEASE_MS: u64 = 15 * 60 * 1_000;
const MAX_WRITE_PATHS: usize = 32;
const MAX_RESULT_EVIDENCE: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkItemClaim {
    pub claim_id: String,
    pub actor: String,
    pub base_revision: Revision,
    pub claimed_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkItemOutcome {
    Complete,
    Blocked,
    Incomplete,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkItemResult {
    pub id: String,
    pub actor: String,
    pub outcome: WorkItemOutcome,
    pub summary: String,
    pub base_revision: Revision,
    pub repository_revision: Revision,
    pub reported_at_ms: u64,
    pub evidence: Vec<WorkItemEvidence>,
    pub proof_status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkItemEvidence {
    pub id: String,
    pub result: crate::evidence::EvidenceResult,
    pub kind: crate::evidence::EvidenceKind,
    pub producer: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WorklistClaimInput {
    pub expected_revision: u64,
    pub expected_repository_revision: Revision,
    pub item_id: String,
    pub actor: String,
    #[serde(default)]
    pub claim_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WorklistSubmitInput {
    pub expected_revision: u64,
    pub expected_repository_revision: Revision,
    pub item_id: String,
    pub claim_id: String,
    pub outcome: WorkItemOutcome,
    pub summary: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

pub(crate) fn claim(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    input: WorklistClaimInput,
) -> Result<Value> {
    claim_with_context(harness, workspace, input, |query| {
        harness.agent_context(workspace_id, workspace, query, 0, &[])
    })
}

fn claim_with_context(
    harness: &ToolHarness,
    workspace: &Workspace,
    input: WorklistClaimInput,
    context_builder: impl FnOnce(&str) -> Result<Value>,
) -> Result<Value> {
    let mut worklist = guarded_worklist(workspace, input.expected_revision)?;
    check_revision(harness, workspace, &input.expected_repository_revision)?;
    let now = now_ms();
    let actor = bounded_text(&input.actor, 160, "actor")?;
    let index = worklist
        .items
        .iter()
        .position(|item| item.id == input.item_id)
        .ok_or_else(|| anyhow::anyhow!("worklist item not found"))?;
    let item = &worklist.items[index];
    if !matches!(
        item.status,
        WorkItemStatus::Pending | WorkItemStatus::InProgress
    ) {
        bail!("worklist item is not claimable; inspect its status");
    }
    let done = worklist
        .items
        .iter()
        .filter(|item| item.status == WorkItemStatus::Done)
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    if !item
        .depends_on
        .iter()
        .all(|dependency| done.contains(dependency.as_str()))
    {
        bail!("worklist dependencies are not complete");
    }
    let paths = canonical_paths(workspace, &item.write_paths)?;
    let mut owned = if let Some(token) = input.claim_id.as_deref() {
        let existing = item
            .claim
            .as_ref()
            .filter(|claim| claim.claim_id == token && claim.expires_at_ms > now)
            .ok_or_else(|| {
                anyhow::anyhow!("claim token is invalid or expired; reread worklist_status")
            })?;
        if existing.actor != actor {
            bail!("claim renewal actor differs from the recorded actor");
        }
        existing.clone()
    } else {
        if active_claim(item, now) {
            bail!("worklist item already has an active claim");
        }
        WorkItemClaim {
            claim_id: format!("WC-{}", Uuid::new_v4().simple()),
            actor,
            base_revision: input.expected_repository_revision.clone(),
            claimed_at_ms: now,
            expires_at_ms: now.saturating_add(CLAIM_LEASE_MS),
        }
    };
    for other in &worklist.items {
        if other.id != item.id && active_claim(other, now) && overlaps(&paths, &other.write_paths) {
            bail!(
                "write scope conflicts with claimed worklist item {}; wait or split the scope",
                other.id
            );
        }
    }
    let query = format!(
        "{} {} {}",
        item.title,
        item.note.as_deref().unwrap_or(""),
        paths.join(" ")
    );
    // Retrieval is slow and may fail. Do it before taking the publication lock,
    // so a failed handoff neither reserves a lane nor blocks other Workspaces.
    let mut context = if input.claim_id.is_none() {
        context_builder(&query)?
    } else {
        Value::Null
    };
    if let Some(pack) = context.as_object_mut() {
        pack.remove("worklist");
        if let Some(execution) = pack.get_mut("execution").and_then(Value::as_object_mut) {
            execution.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "id" | "revision"
                        | "pending_directive"
                        | "verification_floor"
                        | "replan_required"
                )
            });
        }
    }
    ToolHarness::finalize_handoff_context(&mut context)?;
    let source = paths.iter().map(|path| {
        match workspace.path_info(path) {
            Ok(info) => json!({"path":path,"state":"existing","metadata":info}),
            Err(_) => json!({"path":path,"state":"new_or_unavailable","next_action":"read_files or path_info before editing"}),
        }
    }).collect::<Vec<_>>();
    let _guard = update_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("worklist update lock poisoned"))?;
    worklist = guarded_worklist(workspace, input.expected_revision)?;
    check_revision(harness, workspace, &input.expected_repository_revision)?;
    let current = &worklist.items[index];
    let now = now_ms();
    canonical_paths(workspace, &current.write_paths)?;
    if let Some(token) = input.claim_id.as_deref() {
        if !current
            .claim
            .as_ref()
            .is_some_and(|claim| claim.claim_id == token && claim.expires_at_ms > now)
        {
            bail!("claim token expired during handoff retrieval; reread worklist_status");
        }
    } else if active_claim(current, now) {
        bail!("worklist item already has an active claim");
    }
    for other in &worklist.items {
        if other.id != current.id
            && active_claim(other, now)
            && overlaps(&paths, &other.write_paths)
        {
            bail!(
                "write scope conflicts with claimed worklist item {}; wait or split the scope",
                other.id
            );
        }
    }
    owned.expires_at_ms = now_ms().saturating_add(CLAIM_LEASE_MS);
    let item = &mut worklist.items[index];
    item.write_paths = paths.clone();
    item.status = WorkItemStatus::InProgress;
    item.claim = Some(owned.clone());
    commit(workspace, &mut worklist)?;
    Ok(json!({
        "worklist": status_value(&worklist, false),
        "claim_id": owned.claim_id,
        "handoff": {
            "item": public_item(&worklist.items[index], now_ms()),
            "base_revision": owned.base_revision,
            "write_paths":paths,
            "read_only":paths.is_empty(),
            "source":source,
            "agent_context":context,
            "host_execution":"Use the host's supported agent-spawning primitive with this scoped handoff. wcode does not spawn a model agent.",
            "coordination":"Lease and write_paths coordinate cooperating agents only; existing Workspace authorization and native SHA guards still govern edits.",
            "result_contract":"Submit the report using worklist_submit and the private claim_id; reported completion is not verification Evidence or proof readiness.",
            "context_reuse":"Use the included context while its code/design revision and source SHAs match; refresh only missing, stale, or newly requested context. Renewal omits the previously supplied pack.",
            "next_actions":["host native spawn if supported","worklist_claim for renewal","worklist_submit"]
        }
    }))
}

pub(crate) fn submit(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    input: WorklistSubmitInput,
) -> Result<Value> {
    let mut worklist = guarded_worklist(workspace, input.expected_revision)?;
    check_revision(harness, workspace, &input.expected_repository_revision)?;
    let now = now_ms();
    let index = worklist
        .items
        .iter()
        .position(|item| item.id == input.item_id)
        .ok_or_else(|| anyhow::anyhow!("worklist item not found"))?;
    let item = &worklist.items[index];
    let owned = item
        .claim
        .as_ref()
        .filter(|claim| claim.claim_id == input.claim_id && claim.expires_at_ms > now)
        .ok_or_else(|| {
            anyhow::anyhow!("claim token is invalid or expired; reread worklist_status")
        })?
        .clone();
    canonical_paths(workspace, &item.write_paths)?;
    let summary = bounded_text(&input.summary, 1_000, "summary")?;
    if input.evidence_ids.len() > MAX_RESULT_EVIDENCE {
        bail!("too many result evidence references");
    }
    let records = if input.evidence_ids.is_empty() {
        None
    } else {
        Some(harness.evidence_status(workspace_id, workspace, None, 4_096)?)
    };
    let mut evidence = Vec::new();
    let mut unique = BTreeSet::new();
    for id in &input.evidence_ids {
        if !unique.insert(id) {
            bail!("duplicate result evidence reference");
        }
        let record = records
            .as_ref()
            .into_iter()
            .flat_map(|status| &status.evidence)
            .find(|record| {
                record.id == *id && record.revision == input.expected_repository_revision
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "evidence reference is missing, stale, or outside the retained snapshot"
                )
            })?;
        evidence.push(WorkItemEvidence {
            id: bounded_text(&record.id, 160, "evidence id")?,
            result: record.result,
            kind: record.kind,
            producer: bounded_text(&record.producer, 256, "producer")?,
        });
    }
    let _guard = update_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("worklist update lock poisoned"))?;
    worklist = guarded_worklist(workspace, input.expected_revision)?;
    check_revision(harness, workspace, &input.expected_repository_revision)?;
    let reported_at_ms = now_ms();
    if !worklist.items[index].claim.as_ref().is_some_and(|claim| {
        claim.claim_id == input.claim_id && claim.expires_at_ms > reported_at_ms
    }) {
        bail!("claim token expired during result retrieval; reread worklist_status");
    }
    let result = WorkItemResult {
        id: format!("WR-{}", Uuid::new_v4().simple()),
        actor: owned.actor,
        outcome: input.outcome,
        summary,
        base_revision: owned.base_revision,
        repository_revision: input.expected_repository_revision,
        reported_at_ms,
        proof_status: if evidence.is_empty() {
            "not_reported"
        } else {
            "current_references_only"
        }
        .into(),
        evidence,
    };
    let item = &mut worklist.items[index];
    item.status = match input.outcome {
        WorkItemOutcome::Complete => WorkItemStatus::Done,
        WorkItemOutcome::Blocked => WorkItemStatus::Blocked,
        WorkItemOutcome::Incomplete => WorkItemStatus::Pending,
    };
    item.claim = None;
    item.result = Some(result.clone());
    commit(workspace, &mut worklist)?;
    Ok(json!({
        "worklist":status_value(&worklist,false),
        "result":result,
        "verification_authority":"reported outcome only; use current verification_status/evidence_status and existing Execution gates for proof readiness"
    }))
}

fn guarded_worklist(workspace: &Workspace, expected_revision: u64) -> Result<Worklist> {
    let worklist = load(workspace)?.ok_or_else(|| anyhow::anyhow!("worklist does not exist"))?;
    if worklist.revision != expected_revision {
        bail!("worklist revision changed; reread worklist_status and merge before retrying");
    }
    Ok(worklist)
}

fn check_revision(harness: &ToolHarness, workspace: &Workspace, expected: &Revision) -> Result<()> {
    if harness.current_revision(workspace)? != *expected {
        bail!("repository revision changed; refresh the scoped handoff before retrying");
    }
    if expected.code.ends_with(":partial")
        || expected
            .design
            .as_ref()
            .is_some_and(|value| value.ends_with(":partial"))
    {
        bail!("partial repository revision cannot bind an agent claim");
    }
    Ok(())
}

fn commit(workspace: &Workspace, worklist: &mut Worklist) -> Result<()> {
    worklist.revision = worklist
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("worklist revision overflow"))?;
    worklist.updated_at_ms = now_ms();
    validate(worklist)?;
    persist(workspace, worklist)
}

fn bounded_text(value: &str, max: usize, field: &str) -> Result<String> {
    if value.trim().is_empty()
        || value.chars().count() > max
        || value.chars().any(|c| c.is_control() && c != '\n')
    {
        bail!("invalid {field}");
    }
    Ok(crate::workspace::redact_sensitive_text(value.trim())
        .0
        .chars()
        .take(max)
        .collect())
}

pub(super) fn canonical_paths(workspace: &Workspace, paths: &[String]) -> Result<Vec<String>> {
    if paths.len() > MAX_WRITE_PATHS {
        bail!("too many write paths");
    }
    let mut result = BTreeSet::new();
    for path in paths {
        if path.len() > 300 || path.contains('\\') {
            bail!("invalid portable write path");
        }
        let normalized = Workspace::normalize_relative_scope(path)?;
        if normalized.is_empty() {
            bail!("root-wide write paths are forbidden; use [] for a read-only lane");
        }
        let mut candidate = workspace.root().to_path_buf();
        for component in Path::new(&normalized).components() {
            candidate.push(component);
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("symlink write paths are forbidden")
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        result.insert(normalized);
    }
    Ok(result.into_iter().collect())
}

pub(super) fn active_claim(item: &WorkItem, now: u64) -> bool {
    item.claim
        .as_ref()
        .is_some_and(|claim| claim.expires_at_ms > now)
}

pub(super) fn overlaps(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            let (a, b) = if cfg!(any(target_os = "windows", target_os = "macos")) {
                (a.to_lowercase(), b.to_lowercase())
            } else {
                (a.clone(), b.clone())
            };
            a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
        })
    })
}

pub(super) fn public_item(item: &WorkItem, now: u64) -> Value {
    let mut value = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(claim) = value.get_mut("claim").and_then(Value::as_object_mut) {
        claim.remove("claim_id");
        claim.insert(
            "expired".into(),
            json!(item
                .claim
                .as_ref()
                .is_some_and(|claim| claim.expires_at_ms <= now)),
        );
    }
    value
}

pub(super) fn validate_metadata(item: &WorkItem) -> Result<()> {
    if item.write_paths.len() > MAX_WRITE_PATHS {
        bail!("too many write paths");
    }
    for path in &item.write_paths {
        if path.len() > 300
            || path.contains('\\')
            || Workspace::normalize_relative_scope(path)? != *path
            || path.is_empty()
        {
            bail!("invalid stored write path");
        }
    }
    if let Some(claim) = &item.claim {
        bounded_text(&claim.actor, 160, "claim actor")?;
        if !claim.claim_id.starts_with("WC-")
            || claim.claim_id.len() != 35
            || claim.expires_at_ms <= claim.claimed_at_ms
            || item.status != WorkItemStatus::InProgress
        {
            bail!("invalid stored worklist claim");
        }
    }
    if let Some(result) = &item.result {
        bounded_text(&result.summary, 1_000, "result summary")?;
        if result.evidence.len() > MAX_RESULT_EVIDENCE {
            bail!("invalid stored evidence references");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/worklist_delegation.rs"]
mod tests;
