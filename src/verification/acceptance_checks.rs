use super::*;
use crate::evidence::{
    Confidence, EvidenceAuthority, EvidenceKind, VerificationCheckExecution,
    VerificationExecutionReceipt,
};

#[path = "acceptance_reuse.rs"]
mod acceptance_reuse;
pub(super) use acceptance_reuse::{project_checks, summarize};

pub(super) fn required_bindings(
    input: &AcceptanceInput<'_>,
    status: Option<&VerificationStatus>,
) -> Result<BTreeMap<String, Option<RequiredVerificationCheck>>> {
    let mut required = BTreeMap::new();
    if let Some(policy) = input.policy {
        for binding in &policy.required_checks {
            required.insert(binding.id.clone(), Some(binding.clone()));
        }
        for id in &policy.selection.requirements.checks {
            required.entry(id.clone()).or_insert(None);
        }
    }
    if let Some(status) = status {
        for binding in status.plan.required_checks.iter().flatten() {
            required
                .entry(binding.id.clone())
                .or_insert_with(|| Some(binding.clone()));
        }
    }
    ensure!(
        required.len() <= 64,
        "acceptance required check budget exceeded"
    );
    Ok(required)
}

fn current_git(input: &AcceptanceInput<'_>, record: &Evidence) -> bool {
    input.git.binding.is_some()
        && input.policy.is_some_and(|policy| {
            record.execution_policy_binding.as_deref() == Some(policy.plan_policy().as_str())
        })
        && record.execution_git_binding.as_ref() == input.git.binding.as_ref()
        && record.execution_receipt.as_ref().is_none_or(|receipt| {
            receipt.execution_git_binding.as_ref() == input.git.binding.as_ref()
        })
}

pub(super) fn usable_for_git(input: &AcceptanceInput<'_>, record: &Evidence) -> bool {
    !matches!(
        record.authority,
        EvidenceAuthority::NativeVerification
            | EvidenceAuthority::NativeStage
            | EvidenceAuthority::LocalOperator
            | EvidenceAuthority::LegacyUnknown
            | EvidenceAuthority::ExternalReceipt
    ) || current_git(input, record)
}

fn native_receipt(record: &Evidence) -> Option<&VerificationExecutionReceipt> {
    record.execution_receipt.as_ref().filter(|receipt| {
        record.authority == EvidenceAuthority::NativeVerification
            && receipt.valid()
            && record.result == receipt.result()
            && record.policy.as_deref()
                == Some(format!("deterministic/{}/v2", receipt.level).as_str())
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceEvidenceSummary {
    pub id: String,
    pub producer: String,
    pub kind: EvidenceKind,
    pub revision: Revision,
    pub timestamp_ms: u64,
    pub confidence: Confidence,
    pub authority: EvidenceAuthority,
    pub execution_policy_binding: Option<String>,
    pub targets: Vec<String>,
    pub freshness: AcceptanceFreshness,
    pub verification_relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_timing: Option<AcceptanceCheckTiming>,
}

fn execution_timing(
    input: &AcceptanceInput<'_>,
    record: &Evidence,
) -> Option<AcceptanceCheckTiming> {
    if record.confidence != Confidence::Deterministic
        || record.revision != *input.revision
        || !current_git(input, record)
        || record.timestamp_ms == 0
        || record.timestamp_ms > input.now_ms
    {
        return None;
    }
    let receipt = native_receipt(record)?;
    let [item] = receipt.checks.as_slice() else {
        return None;
    };
    if receipt.required_checks.as_slice() != std::slice::from_ref(&item.check)
        || record.subject != format!("verification:{}", item.check.id)
        || item.execution != VerificationCheckExecution::Executed
        || item.reused_from.is_some()
    {
        return None;
    }
    let summary = record.summary.as_deref()?;
    let payload = summary.strip_prefix("verification-metrics-v1;elapsed_ms=")?;
    let (execution_ms, phase) = payload.split_once(";phase=")?;
    let execution_ms = execution_ms.parse::<u64>().ok()?;
    let phase = phase.parse::<u8>().ok()?;
    if summary != crate::report_types::verification_metrics_summary(u128::from(execution_ms), phase)
    {
        return None;
    }
    Some(AcceptanceCheckTiming {
        source: "native_check_execution/v1".into(),
        check_id: item.check.id.clone(),
        signature: item.check.signature.clone(),
        level: receipt.level.clone(),
        phase,
        execution_ms,
    })
}

pub(super) fn evidence_summary(
    input: &AcceptanceInput<'_>,
    ids: &BTreeSet<String>,
) -> Vec<AcceptanceEvidenceSummary> {
    let mut summaries = BTreeMap::new();
    for record in input
        .evidence
        .iter()
        .filter(|record| ids.contains(&record.id))
    {
        summaries
            .entry(record.id.clone())
            .or_insert_with(|| AcceptanceEvidenceSummary {
                id: record.id.clone(),
                producer: record.producer.clone(),
                kind: record.kind,
                revision: record.revision.clone(),
                timestamp_ms: record.timestamp_ms,
                confidence: record.confidence,
                authority: record.authority,
                execution_policy_binding: record.execution_policy_binding.clone(),
                targets: record.targets.clone(),
                execution_timing: execution_timing(input, record),
                freshness: if record.revision != *input.revision {
                    AcceptanceFreshness::Stale
                } else if record.execution_git_binding.is_none()
                    || record.execution_policy_binding.is_none()
                {
                    AcceptanceFreshness::Unbound
                } else if !current_git(input, record) {
                    AcceptanceFreshness::Stale
                } else {
                    AcceptanceFreshness::Current
                },
                verification_relation: match record.authority {
                    EvidenceAuthority::NativeVerification => "native_check",
                    EvidenceAuthority::NativeStage => "native_stage",
                    EvidenceAuthority::LocalOperator => "operator_decision",
                    _ => "historical_advisory",
                }
                .into(),
            });
    }
    summaries.into_values().collect()
}

pub(super) fn verification_summary(
    input: &AcceptanceInput<'_>,
    status: &VerificationStatus,
) -> AcceptanceVerificationSummary {
    let plan = &status.plan;
    let mut stages = BTreeMap::new();
    for (label, kind, stage) in [
        (
            "property",
            EvidenceKind::Property,
            super::super::VerificationStage::Property,
        ),
        (
            "mutation",
            EvidenceKind::Mutation,
            super::super::VerificationStage::Mutation,
        ),
        (
            "fuzz",
            EvidenceKind::Fuzz,
            super::super::VerificationStage::Fuzz,
        ),
        (
            "runtime_canary",
            EvidenceKind::Runtime,
            super::super::VerificationStage::RuntimeCanary,
        ),
    ] {
        let expected = format!("{}/stage/{stage:?}", plan.policy).to_ascii_lowercase();
        let records = crate::evidence::latest_current(
            input.evidence.iter().filter(|record| {
                record.subject == plan.subject
                    && record.kind == kind
                    && record.authority == EvidenceAuthority::NativeStage
                    && record.policy.as_deref() == Some(&expected)
                    && current_git(input, record)
            }),
            input.revision,
        );
        let targets: Vec<Option<&str>> = if plan.stage_targets.is_empty() {
            vec![None]
        } else {
            plan.stage_targets
                .iter()
                .map(|target| Some(target.as_str()))
                .collect()
        };
        let results = targets
            .iter()
            .map(|target| {
                records
                    .iter()
                    .filter(|record| {
                        target.is_none_or(|target| {
                            record.targets.iter().any(|covered| covered == target)
                        })
                    })
                    .map(|record| record.result)
                    .max_by_key(|result| crate::evidence::result_severity(*result))
            })
            .collect::<Vec<_>>();
        // A passing subset never appears as a passing stage. A real failure
        // stays visible even when another required target has no proof.
        let failure = results
            .iter()
            .flatten()
            .copied()
            .find(|result| *result == EvidenceResult::Fail);
        let result = failure.or_else(|| {
            results
                .iter()
                .all(Option::is_some)
                .then(|| {
                    results
                        .iter()
                        .flatten()
                        .copied()
                        .max_by_key(|result| crate::evidence::result_severity(*result))
                })
                .flatten()
        });
        if let Some(result) = result {
            stages.insert(label.into(), result);
        }
    }
    let expected = format!(
        "human-approval/v1/sha256:{:x}",
        Sha256::digest(
            serde_json::to_vec(plan).expect("validated Verification plan is serializable")
        )
    );
    let humans = input
        .evidence
        .iter()
        .filter(|record| {
            record.subject == plan.subject
                && record.kind == EvidenceKind::HumanApproval
                && record.revision == *input.revision
                && record.authority == EvidenceAuthority::LocalOperator
                && record.policy.as_deref() == Some(&expected)
                && current_git(input, record)
        })
        .collect::<Vec<_>>();
    let newest = humans.iter().map(|record| record.timestamp_ms).max();
    let human = humans
        .iter()
        .filter(|record| Some(record.timestamp_ms) == newest)
        .map(|record| record.result)
        .max_by_key(|result| crate::evidence::result_severity(*result));
    let deterministic = crate::evidence::latest_current(
        input.evidence.iter().filter(|record| {
            record.subject == plan.subject
                && record.kind == EvidenceKind::Verification
                && current_git(input, record)
                && native_receipt(record).is_some()
        }),
        input.revision,
    );
    let deterministic_result = deterministic
        .iter()
        .map(|record| record.result)
        .max_by_key(|result| crate::evidence::result_severity(*result));
    AcceptanceVerificationSummary {
        deterministic_result,
        stage_results: stages,
        human_approval: human,
        queued: status.queued,
        claimed: status.claimed,
        submitted: status.submitted,
        reviewer_failures: status.reviewer_failures,
        reviewer_inconclusive: status.reviewer_inconclusive,
        disagreements: status.disagreements,
    }
}
