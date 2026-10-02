//! Bounded historical advice, not Evidence, an Acceptance gate or authorization.
//! Only native callers may submit trusted workspace Evidence. Labels and imported
//! JSON do not authenticate that provenance. Same-check recovery is an observed
//! association, never proof of a bug's cause or of the whole change's acceptance.

use super::workspace_access::WorkspaceStoreAccess;
use super::{
    bounded_token, digest_bytes, now_ms, valid_repository_path, valid_revision_digest,
    EngineeringMilestone,
};
use crate::evidence::{
    Confidence, Evidence, EvidenceAuthority, EvidenceKind, EvidenceResult,
    RequiredVerificationCheck, Revision, VerificationCheckExecution,
};
use crate::evidence_store::workspace_state_directory;
use crate::verification::change::ExecutionGitBinding;
use crate::workspace::Workspace;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const VERSION: u8 = 1;
const MAX_RECORDS: usize = 512;
const MAX_BYTES: u64 = 32 * 1024;
const MAX_PATHS: usize = 16;
const MAX_RULES: usize = 32;
const RULES_PATH: &str = ".wcode/failure-rules.yaml";
static ACCESS: WorkspaceStoreAccess = WorkspaceStoreAccess::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuleMatch {
    id: String,
    digest: String,
    guidance: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckObservation {
    evidence_id: String,
    check: RequiredVerificationCheck,
    level: String,
    revision: Revision,
    git: Option<ExecutionGitBinding>,
    policy_binding: Option<String>,
    result: EvidenceResult,
    execution: VerificationCheckExecution,
    native_complete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    failure: CheckObservation,
    pass: CheckObservation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MemoryRecord {
    schema_version: u8,
    id: String,
    root_digest: String,
    timestamp_ms: u64,
    event_id: Option<String>,
    tool: Option<String>,
    paths: Vec<String>,
    rules: Vec<RuleMatch>,
    check: Option<CheckObservation>,
    recovery: Option<Recovery>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct FailureMemoryUpdate {
    pub observations_written: usize,
    pub recoveries_written: usize,
    pub rules_available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FailureMemoryLesson {
    pub id: String,
    pub status: &'static str,
    pub source: &'static str,
    pub check: Option<RequiredVerificationCheck>,
    pub paths: Vec<String>,
    pub observations: usize,
    pub failure_evidence_id: Option<String>,
    pub pass_evidence_id: Option<String>,
    pub guidance: String,
    pub cause_proven: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FailureMemoryRecall {
    pub retained_records: usize,
    pub partial: bool,
    pub omitted: usize,
    pub items: Vec<FailureMemoryLesson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    schema_version: u8,
    rules: Vec<RepositoryRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryRule {
    id: String,
    literal: Option<String>,
    check: Option<String>,
    path: Option<String>,
    guidance: String,
}

struct Rules {
    entries: Vec<RepositoryRule>,
    digest: String,
    available: bool,
}

impl Rules {
    fn matches(
        &self,
        check: Option<&str>,
        paths: &[String],
        error: Option<&str>,
    ) -> Vec<RuleMatch> {
        self.entries
            .iter()
            .filter(|rule| {
                rule.literal
                    .as_deref()
                    .is_none_or(|literal| error.is_some_and(|error| error.contains(literal)))
                    && rule.check.as_deref().is_none_or(|id| check == Some(id))
                    && rule.path.as_deref().is_none_or(|prefix| {
                        paths
                            .iter()
                            .any(|path| path == prefix || path.starts_with(&format!("{prefix}/")))
                    })
            })
            .map(|rule| RuleMatch {
                id: rule.id.clone(),
                digest: self.digest.clone(),
                guidance: rule.guidance.clone(),
            })
            .collect()
    }
}

fn load_rules(workspace: &Workspace) -> Rules {
    let absent = || Rules {
        entries: Vec::new(),
        digest: String::new(),
        available: true,
    };
    if !workspace.root().join(RULES_PATH).exists() {
        return absent();
    }
    let parsed = (|| -> Result<Rules> {
        let file = workspace.read_file(RULES_PATH, 1, Some(512))?;
        ensure!(
            !file.redacted && file.total_lines <= 512 && file.content.len() as u64 <= MAX_BYTES,
            "failure rules exceed their safe read bound"
        );
        let input: RuleFile = serde_yaml::from_str(&file.content)?;
        ensure!(
            input.schema_version == VERSION && input.rules.len() <= MAX_RULES,
            "unsupported or oversized failure rules"
        );
        let mut identities = BTreeSet::new();
        for rule in &input.rules {
            let safe_guidance = crate::workspace::redact_sensitive_text(&rule.guidance);
            ensure!(
                bounded_token(&rule.id, 80)
                    && identities.insert(&rule.id)
                    && bounded_token(&rule.guidance, 384)
                    && !safe_guidance.1
                    && rule
                        .literal
                        .as_ref()
                        .is_none_or(|value| bounded_token(value, 160))
                    && rule
                        .check
                        .as_ref()
                        .is_none_or(|value| bounded_token(value, 160))
                    && rule
                        .path
                        .as_ref()
                        .is_none_or(|path| valid_repository_path(path)
                            && path != "."
                            && !path.contains('\\'))
                    && (rule.literal.is_some() || rule.check.is_some() || rule.path.is_some()),
                "invalid repository advisory failure rule"
            );
        }
        Ok(Rules {
            entries: input.rules,
            digest: format!("sha256:{}", file.sha256),
            available: true,
        })
    })();
    parsed.unwrap_or_else(|_| Rules {
        entries: Vec::new(),
        digest: String::new(),
        available: false,
    })
}

/// Called by the native journal/dispatch path; transient text is matched only,
/// never copied into a record. Repository guidance remains unverified advice.
pub(crate) fn observe_tool_failure(
    workspace: &Workspace,
    milestone: &EngineeringMilestone,
    transient_error: Option<&str>,
) -> Result<FailureMemoryUpdate> {
    milestone.validate()?;
    let rules = load_rules(workspace);
    let mut update = FailureMemoryUpdate {
        rules_available: rules.available,
        ..Default::default()
    };
    if !matches!(milestone.outcome.as_str(), "failed" | "blocked") || milestone.event_id.is_none() {
        return Ok(update);
    }
    let paths = bounded_paths(workspace, &milestone.paths);
    let error = transient_error.filter(|error| error.len() <= 4096);
    let record = MemoryRecord {
        schema_version: VERSION,
        id: String::new(),
        root_digest: root_digest(workspace)?,
        timestamp_ms: milestone.timestamp_ms,
        event_id: milestone.event_id.clone(),
        tool: Some(milestone.tool.clone()),
        paths: paths.clone(),
        rules: rules.matches(None, &paths, error),
        check: None,
        recovery: None,
    };
    let access = ACCESS.for_workspace(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("failure memory lock poisoned"))?;
    let mut history = load_records(workspace)?;
    update.observations_written = usize::from(append(workspace, &mut history, record)?);
    Ok(update)
}

/// The caller obtains the Evidence slice from the selected workspace's native
/// persistence hook. Complete current Git/Revision and exact Policy (including
/// None) are required; producer names or public JSON cannot mint native trust.
pub(crate) fn observe_native_verification(
    workspace: &Workspace,
    current_revision: &Revision,
    current_git: Option<&ExecutionGitBinding>,
    current_policy_binding: Option<&str>,
    evidence: &[Evidence],
    changed_paths: &[String],
) -> Result<FailureMemoryUpdate> {
    ensure!(
        valid_revision(current_revision)
            && current_git.is_none_or(ExecutionGitBinding::valid)
            && current_policy_binding.is_none_or(|value| bounded_token(value, 256))
            && evidence.len() <= 4096,
        "invalid failure memory verification context"
    );
    let mut identities = BTreeMap::new();
    for source in evidence {
        source.validate()?;
        if let Some(previous) = identities.insert(&source.id, source) {
            ensure!(
                previous == source,
                "conflicting Evidence identity in failure memory input"
            );
        }
    }
    let rules = load_rules(workspace);
    let paths = bounded_paths(workspace, changed_paths);
    let mut observations = BTreeMap::new();
    for source in crate::evidence::latest_current(evidence.iter(), current_revision) {
        let Some(receipt) = &source.execution_receipt else {
            continue;
        };
        if source.timestamp_ms == 0
            || source.timestamp_ms > now_ms().saturating_add(super::MAX_CLOCK_SKEW_MS)
        {
            continue;
        }
        let native = native_context(
            source,
            current_revision,
            current_git,
            current_policy_binding,
        ) && complete_receipt(receipt)
            && receipt.result() == source.result;
        for item in &receipt.checks {
            // A claimed Pass without a complete native project run teaches
            // nothing. Unknown/unavailable negatives remain observations.
            let complete_project_pass = native
                && source.kind == EvidenceKind::Verification
                && source.result == EvidenceResult::Pass;
            if item.result == EvidenceResult::Pass && !complete_project_pass {
                continue;
            }
            let observation = CheckObservation {
                evidence_id: source.id.clone(),
                check: item.check.clone(),
                level: receipt.level.clone(),
                revision: source.revision.clone(),
                git: source.execution_git_binding.clone(),
                policy_binding: source.execution_policy_binding.clone(),
                result: item.result,
                execution: item.execution,
                native_complete: native
                    && item.execution == VerificationCheckExecution::Executed
                    && item.reused_from.is_none()
                    && (item.result != EvidenceResult::Pass || complete_project_pass),
            };
            let key = (item.check.clone(), receipt.level.clone());
            let candidate = (
                source.timestamp_ms,
                crate::evidence::result_severity(item.result),
                observation.native_complete,
                source.id.clone(),
                observation,
            );
            // A later advisory/criterion copy must not hide the independent
            // executed native failure that can support a future recovery link.
            if observations.get(&key).is_none_or(
                |previous: &(u64, u8, bool, String, CheckObservation)| {
                    (candidate.2, candidate.0, candidate.1, &candidate.3)
                        > (previous.2, previous.0, previous.1, &previous.3)
                },
            ) {
                observations.insert(key, candidate);
                ensure!(
                    observations.len() <= crate::evidence::MAX_RECEIPT_CHECKS,
                    "failure memory check observations exceed the per-run bound"
                );
            }
        }
    }
    let access = ACCESS.for_workspace(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("failure memory lock poisoned"))?;
    let mut history = load_records(workspace)?;
    let root = root_digest(workspace)?;
    let mut update = FailureMemoryUpdate {
        rules_available: rules.available,
        ..Default::default()
    };
    for (_, (timestamp_ms, _, _, _, observation)) in observations {
        if observation.result == EvidenceResult::Pass && observation.native_complete {
            let previous = history
                .iter()
                .filter_map(|record| record.check.as_ref().map(|check| (record, check)))
                .filter(|(record, check)| {
                    record.timestamp_ms < timestamp_ms && recoverable(check, &observation)
                })
                .max_by_key(|(record, _)| (record.timestamp_ms, record.id.as_str()));
            if let Some((_, failure)) = previous {
                let recovery = Recovery {
                    failure: failure.clone(),
                    pass: observation.clone(),
                };
                let record = MemoryRecord {
                    schema_version: VERSION,
                    id: String::new(),
                    root_digest: root.clone(),
                    timestamp_ms,
                    event_id: None,
                    tool: None,
                    paths: paths.clone(),
                    rules: Vec::new(),
                    check: None,
                    recovery: Some(recovery),
                };
                update.recoveries_written += usize::from(append(workspace, &mut history, record)?);
            }
        }
        let record = MemoryRecord {
            schema_version: VERSION,
            id: String::new(),
            root_digest: root.clone(),
            timestamp_ms,
            event_id: None,
            tool: None,
            paths: paths.clone(),
            rules: rules.matches(Some(&observation.check.id), &paths, None),
            check: Some(observation),
            recovery: None,
        };
        update.observations_written += usize::from(append(workspace, &mut history, record)?);
    }
    Ok(update)
}

fn complete_receipt(receipt: &crate::evidence::VerificationExecutionReceipt) -> bool {
    receipt.valid()
        && receipt.skipped_checks.is_empty()
        && receipt.covers(&receipt.required_checks)
        && receipt.checks.iter().all(|item| {
            item.execution == VerificationCheckExecution::Executed && item.reused_from.is_none()
        })
}

fn native_context(
    source: &Evidence,
    revision: &Revision,
    git: Option<&ExecutionGitBinding>,
    policy: Option<&str>,
) -> bool {
    source.authority == EvidenceAuthority::NativeVerification
        && source.confidence == Confidence::Deterministic
        && source.revision == *revision
        && git.is_some()
        && source.execution_git_binding.as_ref() == git
        && source.execution_policy_binding.as_deref() == policy
        && source
            .execution_receipt
            .as_ref()
            .is_some_and(|receipt| receipt.execution_git_binding.as_ref() == git)
        && source.execution_receipt.as_ref().is_some_and(|receipt| {
            source.policy.as_deref() == Some(format!("deterministic/{}/v2", receipt.level).as_str())
                && ((source.kind == EvidenceKind::Verification
                    && source.producer == "verify_project"
                    && source.subject == format!("change:{}", source.revision.code))
                    || (receipt.checks.len() == 1
                        && source.subject
                            == format!("verification:{}", receipt.checks[0].check.id)
                        && matches!(
                            source.kind,
                            EvidenceKind::Compiler
                                | EvidenceKind::StaticAnalysis
                                | EvidenceKind::UnitTest
                                | EvidenceKind::IntegrationTest
                                | EvidenceKind::EndToEndTest
                                | EvidenceKind::ContractTest
                                | EvidenceKind::Benchmark
                        )))
        })
}

fn recoverable(failure: &CheckObservation, pass: &CheckObservation) -> bool {
    failure.native_complete
        && pass.native_complete
        && failure.result == EvidenceResult::Fail
        && pass.result == EvidenceResult::Pass
        && failure.execution == VerificationCheckExecution::Executed
        && pass.execution == VerificationCheckExecution::Executed
        && failure.check == pass.check
        && failure.level == pass.level
        && failure.policy_binding == pass.policy_binding
        && failure
            .git
            .as_ref()
            .zip(pass.git.as_ref())
            .is_some_and(|(old, new)| old.repository == new.repository)
}

/// Recall only bounded, historical advice. Missing/error stores must be exposed
/// by callers as unavailable; they cannot replace current diagnostics or proof.
pub(crate) fn recall(
    workspace: &Workspace,
    query: &str,
    paths: &[String],
    known_checks: &HashSet<String>,
    limit: usize,
) -> Result<FailureMemoryRecall> {
    ensure!(
        known_checks.len() <= crate::evidence::MAX_RECEIPT_CHECKS
            && known_checks.iter().all(|id| bounded_token(id, 160)),
        "failure memory check relevance exceeds its bounded native context"
    );
    let access = ACCESS.for_workspace(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("failure memory lock poisoned"))?;
    let history = load_records(workspace)?;
    let query = query.chars().take(1024).collect::<String>().to_lowercase();
    let selected = bounded_paths(workspace, paths);
    let mut items = BTreeMap::<String, FailureMemoryLesson>::new();
    let rules = load_rules(workspace);
    let mut observed_checks = BTreeSet::new();
    let mut observed_rules = BTreeSet::new();
    for record in history.iter().rev() {
        let check = record
            .recovery
            .as_ref()
            .map(|link| &link.pass.check)
            .or_else(|| record.check.as_ref().map(|check| &check.check));
        let relevant = selected.is_empty() && query.is_empty()
            || record
                .paths
                .iter()
                .any(|path| selected.contains(path) || query.contains(&path.to_lowercase()))
            || check.is_some_and(|check| {
                // IDs select historical advice only. Exact signature/Policy/Git
                // bindings remain mandatory for recovery and current proof.
                known_checks.contains(&check.id) || query.contains(&check.id.to_lowercase())
            });
        if !relevant {
            continue;
        }
        let recovery = record.recovery.as_ref();
        // Pass observations alone do not create a learned lesson.
        if recovery.is_some()
            || record
                .check
                .as_ref()
                .is_some_and(|check| check.result != EvidenceResult::Pass)
        {
            let key = format!(
                "check:{}:{}",
                check.expect("checked observation").id,
                check.expect("checked observation").signature
            );
            let lesson = items.entry(key).or_insert_with(|| FailureMemoryLesson {
                id: record.id.clone(), status: if recovery.is_some() { "verified_same_check_recovery" } else { "unverified" },
                source: if recovery.is_some() { "native_recovery" } else { "check_observation" },
                check: check.cloned(), paths: record.paths.clone(), observations: 0,
                failure_evidence_id: recovery.map(|link| link.failure.evidence_id.clone()).or_else(|| record.check.as_ref().map(|check| check.evidence_id.clone())),
                pass_evidence_id: recovery.map(|link| link.pass.evidence_id.clone()),
                guidance: if recovery.is_some() { "A later complete native run passed this same check; inspect the linked revisions and edits. This association does not prove the cause or current Acceptance." }
                    else { "A retained check observation remains unverified; inspect current diagnostics and run the required native checks." }.into(),
                cause_proven: false,
            });
            if let Some(check) = record
                .check
                .as_ref()
                .filter(|check| check.result != EvidenceResult::Pass)
            {
                if observed_checks.insert((
                    check.evidence_id.clone(),
                    check.check.clone(),
                    check.level.clone(),
                )) {
                    lesson.observations += 1;
                }
            }
        }
        for rule in &record.rules {
            // Removed/changed repository rules do not keep injecting old prose.
            if !rules.available || rule.digest != rules.digest {
                continue;
            }
            let key = format!("rule:{}", rule.id);
            let lesson = items.entry(key).or_insert_with(|| FailureMemoryLesson {
                id: rule.id.clone(),
                status: "unverified",
                source: "repository_advisory",
                check: check.cloned(),
                paths: record.paths.clone(),
                observations: 0,
                failure_evidence_id: None,
                pass_evidence_id: None,
                guidance: rule.guidance.clone(),
                cause_proven: false,
            });
            let event = record
                .event_id
                .clone()
                .or_else(|| record.check.as_ref().map(|check| check.evidence_id.clone()))
                .unwrap_or_else(|| record.id.clone());
            if observed_rules.insert((rule.id.clone(), event)) {
                lesson.observations += 1;
            }
        }
    }
    let total = items.len();
    let mut items = items.into_values().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        (
            right.status == "verified_same_check_recovery",
            right.observations,
        )
            .cmp(&(
                left.status == "verified_same_check_recovery",
                left.observations,
            ))
            .then_with(|| left.id.cmp(&right.id))
    });
    let items = items.into_iter().take(limit.clamp(1, 8)).collect();
    Ok(FailureMemoryRecall {
        retained_records: history.len(),
        partial: history.len() >= MAX_RECORDS || !rules.available,
        omitted: total.saturating_sub(limit.clamp(1, 8)),
        items,
    })
}

fn bounded_paths(workspace: &Workspace, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| valid_repository_path(path) && workspace.source_metadata_stamp(path).is_ok())
        .take(MAX_PATHS)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn valid_revision(revision: &Revision) -> bool {
    valid_revision_digest(&revision.code)
        && !revision.code.ends_with(":partial")
        && revision
            .design
            .as_ref()
            .is_none_or(|value| valid_revision_digest(value))
}

fn root_digest(workspace: &Workspace) -> Result<String> {
    let root = workspace.root().canonicalize()?;
    let root = root
        .to_str()
        .context("failure memory workspace root is not lossless UTF-8")?;
    Ok(format!(
        "sha256:{}",
        digest_bytes(format!("failure-memory-root-v1\0{root}").as_bytes())
    ))
}

fn directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("failure-memory"))
}

fn ensure_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "failure memory requires a regular directory"
    );
    Ok(())
}

fn record_id(record: &MemoryRecord) -> Result<String> {
    let mut body = record.clone();
    body.id.clear();
    Ok(format!(
        "sha256:{}",
        digest_bytes(&serde_json::to_vec(&body)?)
    ))
}

fn validate_record(record: &MemoryRecord, root: &str) -> Result<()> {
    ensure!(
        record.schema_version == VERSION
            && record.root_digest == root
            && record.id == record_id(record)?
            && record.timestamp_ms > 0
            && record.timestamp_ms <= now_ms().saturating_add(super::MAX_CLOCK_SKEW_MS)
            && record.paths.len() <= MAX_PATHS
            && record.paths.iter().all(|path| valid_repository_path(path))
            && record.rules.len() <= MAX_RULES
            && record
                .event_id
                .as_ref()
                .is_none_or(|id| uuid::Uuid::parse_str(id).is_ok())
            && record
                .tool
                .as_ref()
                .is_none_or(|tool| bounded_token(tool, 80))
            && (record.event_id.is_some() as u8
                + record.check.is_some() as u8
                + record.recovery.is_some() as u8
                == 1),
        "invalid failure memory record"
    );
    for rule in &record.rules {
        ensure!(
            bounded_token(&rule.id, 80)
                && bounded_token(&rule.guidance, 384)
                && valid_revision_digest(&rule.digest),
            "invalid failure memory advisory"
        );
    }
    for check in record.check.iter().chain(
        record
            .recovery
            .iter()
            .flat_map(|link| [&link.failure, &link.pass]),
    ) {
        ensure!(
            check.check.valid()
                && bounded_token(&check.evidence_id, 160)
                && matches!(check.level.as_str(), "quick" | "full")
                && valid_revision(&check.revision)
                && check.git.as_ref().is_none_or(ExecutionGitBinding::valid)
                && check
                    .policy_binding
                    .as_deref()
                    .is_none_or(|policy| bounded_token(policy, 256))
                && (!check.native_complete
                    || (check.git.is_some()
                        && check.execution == VerificationCheckExecution::Executed)),
            "invalid failure memory check observation"
        );
    }
    ensure!(
        record
            .recovery
            .as_ref()
            .is_none_or(|link| recoverable(&link.failure, &link.pass)),
        "invalid failure memory recovery association"
    );
    Ok(())
}

fn load_records(workspace: &Workspace) -> Result<Vec<MemoryRecord>> {
    let directory = directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    ensure_directory(&directory)?;
    let root = root_digest(workspace)?;
    let mut records = Vec::new();
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        ensure!(
            index < MAX_RECORDS + 1,
            "failure memory exceeds bounded directory capacity"
        );
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= MAX_BYTES,
            "failure memory record is not a bounded regular file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.nlink() == 1,
                "failure memory record must not be hard-linked"
            );
        }
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "failure memory record exceeds size bound"
        );
        let record: MemoryRecord =
            serde_json::from_slice(&bytes).context("invalid failure memory JSON")?;
        validate_record(&record, &root)?;
        ensure!(
            path.file_name().and_then(|name| name.to_str()) == Some(filename(&record).as_str()),
            "failure memory filename does not match its record"
        );
        records.push(record);
    }
    records
        .sort_by(|left, right| (left.timestamp_ms, &left.id).cmp(&(right.timestamp_ms, &right.id)));
    Ok(records)
}

fn filename(record: &MemoryRecord) -> String {
    format!(
        "{:020}-{}.json",
        record.timestamp_ms,
        record.id.strip_prefix("sha256:").expect("validated digest")
    )
}

fn append(
    workspace: &Workspace,
    history: &mut Vec<MemoryRecord>,
    mut record: MemoryRecord,
) -> Result<bool> {
    record.id = record_id(&record)?;
    validate_record(&record, &root_digest(workspace)?)?;
    if history.iter().any(|previous| previous.id == record.id) {
        return Ok(false);
    }
    let directory = directory(workspace)?;
    ensure_directory(&directory)?;
    let path = directory.join(filename(&record));
    let bytes = serde_json::to_vec(&record)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "failure memory record exceeds size bound"
    );
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                load_records(workspace)?
                    .iter()
                    .any(|existing| existing == &record),
                "conflicting failure memory record"
            );
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    history.push(record);
    history
        .sort_by(|left, right| (left.timestamp_ms, &left.id).cmp(&(right.timestamp_ms, &right.id)));
    while history.len() > MAX_RECORDS {
        fs::remove_file(directory.join(filename(&history[0])))?;
        history.remove(0);
    }
    #[cfg(unix)]
    {
        File::open(&directory)?.sync_all()?;
    }
    // Windows does not promise directory fsync durability. The append/prune
    // mutex is process-local; this advisory store is not an audit transaction,
    // same-OS-user isolation, complete lifetime history or authenticated proof.
    Ok(true)
}

#[cfg(test)]
#[path = "../../tests/unit/evidence/failure_memory.rs"]
mod tests;
