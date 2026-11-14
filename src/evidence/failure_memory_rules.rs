use super::*;

pub(super) fn load_rules(workspace: &Workspace) -> Rules {
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
            || source.timestamp_ms > now_ms().saturating_add(super::super::MAX_CLOCK_SKEW_MS)
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
