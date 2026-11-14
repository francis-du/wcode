use super::*;

// An absent Design digest is an explicitly captured absence of Design State.
// A missing plan Revision or a bounded partial digest is not a complete binding.
pub(super) fn revision_identity_complete(revision: &Revision) -> bool {
    !revision.code.trim().is_empty()
        && !revision.code.trim().ends_with(":partial")
        && revision
            .design
            .as_deref()
            .is_none_or(|design| !design.trim().is_empty() && !design.trim().ends_with(":partial"))
}

/// A typed process execution still needs a real exit result. Native
/// metadata predicates have fixed producer identities and do not invent one.
pub(super) fn check_execution(
    check: &crate::report_types::VerificationCheck,
) -> crate::evidence::VerificationCheckExecution {
    use crate::evidence::VerificationCheckExecution;
    let native_predicate = match check.id.as_str() {
        "profile-discovery-completeness" => {
            !check.success && check.command.starts_with("wcode-discovery ")
        }
        "core-policy" => !check.success && check.command == "wcode internal core policy",
        "migration-audit" => check.command == "wcode internal migration audit",
        _ => {
            !check.success
                && check.id.starts_with("polyglot-gap:")
                && check.command == "wcode internal polyglot verification coverage"
        }
    };
    if check.execution == VerificationCheckExecution::Executed
        && check.exit_code.is_none()
        && !native_predicate
        && !check.reused
    {
        VerificationCheckExecution::Unavailable
    } else {
        check.execution
    }
}

pub(super) fn verification_reference_executed(
    reference: &VerificationRef,
    report: &VerificationReport,
) -> bool {
    match reference {
        VerificationRef::Check { id } => report.checks.iter().any(|check| {
            !check.reused
                && check_execution(check) == crate::evidence::VerificationCheckExecution::Executed
                && check.id == *id
                && report_check_is_bound(check, report)
        }),
        VerificationRef::Test { .. } => false,
    }
}

pub(super) fn verification_reference_outcome(
    reference: &VerificationRef,
    report: &VerificationReport,
) -> Option<bool> {
    match reference {
        VerificationRef::Check { id } => report
            .checks
            .iter()
            .find(|check| {
                (check_execution(check) == crate::evidence::VerificationCheckExecution::Executed
                    || check.reused)
                    && check.id == *id
                    && report_check_is_bound(check, report)
            })
            .map(|check| check.success),
        VerificationRef::Test { .. } => None,
    }
}

pub(super) fn report_check_is_bound(
    check: &crate::report_types::VerificationCheck,
    report: &VerificationReport,
) -> bool {
    report.required_checks.as_ref().is_some_and(|required| {
        required.iter().any(|binding| {
            binding.id == check.id && check.signature.as_ref() == Some(&binding.signature)
        })
    })
}

pub(super) fn native_report_receipt(
    workspace: &Workspace,
    revision: &Revision,
    report: &VerificationReport,
    policy_binding: Option<&str>,
) -> Result<Option<VerificationExecutionReceipt>> {
    let Some(required) = report.required_checks.as_ref() else {
        return Ok(None);
    };
    if !matches!(report.level.as_str(), "quick" | "full") || required.is_empty() {
        return Ok(None);
    }
    if report.checks_run != report.checks.len()
        || report.checks_reused != report.checks.iter().filter(|check| check.reused).count()
        || report.checks_failed != report.checks.iter().filter(|check| !check.success).count()
    {
        bail!("verification receipt report counters are inconsistent; no evidence was recorded");
    }
    let prior = if report.checks.iter().any(|check| check.reused) {
        evidence_store::load(workspace)?
    } else {
        Vec::new()
    };
    let mut checks = Vec::new();
    for check in &report.checks {
        let binding = RequiredVerificationCheck {
            id: check.id.clone(),
            signature: check.signature.clone().unwrap_or_else(|| {
                RequiredVerificationCheck::from_command(
                    &check.id,
                    &check.command,
                    &[],
                    ".",
                    "native-policy",
                )
                .signature
            }),
        };
        if required
            .iter()
            .any(|required| required.id == check.id && required != &binding)
        {
            bail!(
                "verification receipt command signature mismatch for {}; no evidence was recorded",
                check.id
            );
        }
        let reused_from = if check.reused {
            let source = check
                .evidence_id
                .as_ref()
                .ok_or_else(|| anyhow!("reused check {} has no source evidence", check.id))?;
            let valid = prior.iter().any(|record| {
                record.id == *source
                    && record.effective_authority() == EvidenceAuthority::NativeVerification
                    && record.revision == *revision
                    && record.execution_policy_binding.as_deref() == policy_binding
                    && record.subject == format!("verification:{}", check.id)
                    && record.result == EvidenceResult::Pass
                    && record.execution_receipt.as_ref().is_some_and(|receipt| {
                        receipt.valid()
                            && receipt.execution_git_binding == report.execution_git_binding
                            && receipt.level == report.level
                            && receipt.checks.len() == 1
                            && receipt.checks[0].check == binding
                            && receipt.checks[0].result == EvidenceResult::Pass
                            && receipt.checks[0].execution
                                == crate::evidence::VerificationCheckExecution::Executed
                            && receipt.checks[0].reused_from.is_none()
                    })
            });
            if !valid {
                bail!("reused check {} source evidence is missing or does not match revision/signature", check.id);
            }
            Some(source.clone())
        } else {
            None
        };
        checks.push(VerificationCheckReceipt {
            execution: if check.reused {
                crate::evidence::VerificationCheckExecution::Executed
            } else {
                check_execution(check)
            },
            check: binding,
            result: if check.success {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
            reused_from,
        });
    }
    let receipt = VerificationExecutionReceipt {
        schema_version: 1,
        level: report.level.clone(),
        required_checks: required.clone(),
        checks,
        skipped_checks: report.skipped_checks.clone(),
        execution_git_binding: report.execution_git_binding.clone(),
    };
    if !receipt.valid() || report.passed != (receipt.result() == EvidenceResult::Pass) {
        bail!("verification receipt is incomplete or report verdict is inconsistent; no evidence was recorded");
    }
    Ok(Some(receipt))
}

/// A reused check is only as current as the evidence it came from. The receipt
/// validated that chain when it was written, but retention may since have pruned
/// the source, so a snapshot that can no longer see it must not read the reuse as
/// a current Pass. This mirrors the chain check the receipt applied at write
/// time, using only what this snapshot can observe.
pub(super) fn reuse_chain_is_observable(record: &Evidence, evidence: &[Evidence]) -> bool {
    // A record with no receipt at all is not a reuse case; the receipt guard
    // below reports it as an untrusted legacy record instead.
    let Some(receipt) = record.execution_receipt.as_ref() else {
        return true;
    };
    receipt.checks.iter().all(|item| {
        item.reused_from.as_ref().is_none_or(|id| {
            evidence.iter().any(|source| {
                source.id == *id
                    && source.effective_authority() == EvidenceAuthority::NativeVerification
                    && source.kind == EvidenceKind::Verification
                    && source.execution_policy_binding == record.execution_policy_binding
                    && source.execution_git_binding == record.execution_git_binding
                    && source.subject == format!("verification:{}", item.check.id)
                    && source.result == EvidenceResult::Pass
                    && source.execution_receipt.as_ref().is_some_and(|prior| {
                        prior.valid()
                            && prior.level == receipt.level
                            && prior.checks.len() == 1
                            && prior.checks[0].check == item.check
                            && prior.checks[0].result == EvidenceResult::Pass
                            && prior.checks[0].execution
                                == crate::evidence::VerificationCheckExecution::Executed
                            && prior.checks[0].reused_from.is_none()
                    })
            })
        })
    })
}

pub(super) fn deterministic_plan_result(
    status: &mut VerificationStatus,
    evidence: &[Evidence],
    git_binding: Option<&crate::verification::change::ExecutionGitBinding>,
) -> Option<EvidenceResult> {
    let Some(required) = status
        .plan
        .required_checks
        .as_ref()
        .filter(|checks| !checks.is_empty())
    else {
        status
            .blockers
            .push("deterministic-required-checks-unknown".into());
        return None;
    };
    let mut latest = BTreeMap::new();
    let mut legacy = false;
    for record in evidence.iter().filter(|record| {
        evidence_matches_plan_revision(record, &status.plan)
            && record.kind == EvidenceKind::Verification
            && record.effective_authority() == EvidenceAuthority::NativeVerification
            // A receipt records the policy it executed under. Evidence bound to
            // a different policy than the plan's cannot satisfy it, even when the
            // code and design revision are unchanged.
            && record
                .execution_policy_binding
                .as_deref()
                .is_none_or(|binding| binding == status.plan.policy.as_str())
            // A receipt executed against a different Git identity than the
            // workspace's current one does not establish the current input, even
            // when the code and design revisions are unchanged.
            && git_binding.is_none_or(|binding| {
                record.execution_git_binding.as_ref() == Some(binding)
                    && record
                        .execution_receipt
                        .as_ref()
                        .is_none_or(|receipt| receipt.execution_git_binding.as_ref() == Some(binding))
            })
            && reuse_chain_is_observable(record, evidence)
    }) {
        let Some(receipt) = record.execution_receipt.as_ref().filter(|receipt| {
            receipt.valid()
                && record.result == receipt.result()
                && record.policy.as_deref()
                    == Some(format!("deterministic/{}/v2", receipt.level).as_str())
        }) else {
            legacy = true;
            continue;
        };
        let key = (
            record.producer.as_str(),
            receipt.level.as_str(),
            &receipt.required_checks,
        );
        let entry: &mut &Evidence = latest.entry(key).or_insert(record);
        if record.timestamp_ms > entry.timestamp_ms
            || (record.timestamp_ms == entry.timestamp_ms
                && crate::evidence::result_severity(record.result)
                    > crate::evidence::result_severity(entry.result))
        {
            *entry = record;
        }
    }
    let effective = latest
        .values()
        .copied()
        .filter(|record| {
            let receipt = record
                .execution_receipt
                .as_ref()
                .expect("validated receipt");
            !latest.values().any(|newer| {
                newer.producer == record.producer
                    && newer.timestamp_ms > record.timestamp_ms
                    && newer.execution_receipt.as_ref().is_some_and(|stronger| {
                        stronger.satisfies_level(&receipt.level)
                            && stronger.covers(&receipt.required_checks)
                    })
            })
        })
        .collect::<Vec<_>>();
    let strong = effective
        .iter()
        .filter(|record| {
            record
                .execution_receipt
                .as_ref()
                .is_some_and(|receipt| receipt.satisfies_level(&status.plan.deterministic_level))
        })
        .collect::<Vec<_>>();
    if strong.is_empty() {
        status.blockers.push(format!(
            "deterministic-minimum-level-unproved:{}",
            status.plan.deterministic_level
        ));
        if legacy {
            status
                .blockers
                .push("deterministic-legacy-receipt-untrusted".into());
        }
    }
    for binding in required {
        let observed = strong.iter().any(|record| {
            record
                .execution_receipt
                .as_ref()
                .is_some_and(|receipt| receipt.checks.iter().any(|check| &check.check == binding))
        });
        if !observed {
            let skipped = strong.iter().any(|record| {
                record
                    .execution_receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.skipped_checks.contains(&binding.id))
            });
            status.blockers.push(format!(
                "deterministic-required-check-{}:{}",
                if skipped { "skipped" } else { "missing" },
                binding.id
            ));
        }
    }
    let failure = effective
        .iter()
        .map(|record| record.result)
        .filter(|result| *result != EvidenceResult::Pass)
        .max_by_key(|result| crate::evidence::result_severity(*result));
    if failure.is_some() {
        return failure;
    }
    strong
        .iter()
        .any(|record| {
            record.execution_receipt.as_ref().is_some_and(|receipt| {
                receipt.covers(required) && receipt.result() == EvidenceResult::Pass
            })
        })
        .then_some(EvidenceResult::Pass)
}
