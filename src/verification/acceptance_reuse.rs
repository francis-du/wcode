use super::*;

pub(super) fn reuse_valid(
    input: &AcceptanceInput<'_>,
    receipt: &VerificationExecutionReceipt,
) -> bool {
    receipt.checks.iter().all(|item| {
        item.reused_from.as_ref().is_none_or(|id| {
            input.evidence.iter().any(|source| {
                source.id == *id
                    && source.revision == *input.revision
                    && source.subject == format!("verification:{}", item.check.id)
                    && source.result == EvidenceResult::Pass
                    && current_git(input, source)
                    && native_receipt(source).is_some_and(|prior| {
                        prior.level == receipt.level
                            && prior.checks.len() == 1
                            && prior.checks[0].check == item.check
                            && prior.checks[0].result == EvidenceResult::Pass
                            && prior.checks[0].reused_from.is_none()
                    })
            })
        })
    })
}

pub(in crate::verification::acceptance) fn project_checks(
    input: &AcceptanceInput<'_>,
    status: Option<&VerificationStatus>,
    required: &BTreeMap<String, Option<RequiredVerificationCheck>>,
    reasons: &mut Vec<AcceptanceReason>,
) -> Result<Vec<AcceptanceCheck>> {
    let current = crate::evidence::latest_current(
        input
            .evidence
            .iter()
            .filter(|record| current_git(input, record)),
        input.revision,
    );
    let mut rows = Vec::new();
    for (id, binding) in required {
        let discovered = binding
            .as_ref()
            .is_some_and(|binding| input.discovery.checks.contains(binding));
        let mapped = binding.as_ref().is_some_and(|binding| {
            status.is_some_and(|status| {
                status
                    .plan
                    .required_checks
                    .as_ref()
                    .is_some_and(|checks| checks.contains(binding))
            })
        });
        let mut row = AcceptanceCheck {
            id: id.clone(),
            signature: binding.as_ref().map(|binding| binding.signature.clone()),
            required: true,
            discovered,
            mapped,
            execution: if discovered {
                AcceptanceExecution::Unknown
            } else {
                AcceptanceExecution::Unavailable
            },
            outcome: AcceptanceOutcome::Unknown,
            freshness: AcceptanceFreshness::Missing,
            level: None,
            required_level: minimum_level(input).into(),
            evidence_ids: Vec::new(),
        };
        if !discovered {
            rules::reason(
                reasons,
                "required_check_unavailable",
                Some(id),
                AcceptanceAction::ResolveDiscovery,
            );
        }
        if !mapped {
            rules::reason(
                reasons,
                "required_check_unmapped",
                Some(id),
                AcceptanceAction::PlanVerification,
            );
        }
        let Some(binding) = binding else {
            rows.push(row);
            continue;
        };
        let relevant = |record: &&Evidence| {
            status.is_some_and(|status| record.subject == status.plan.subject)
                || record.subject == format!("verification:{id}")
        };
        let observed = current
            .iter()
            .copied()
            .filter(|record| {
                relevant(record)
                    && native_receipt(record).is_some_and(|receipt| {
                        receipt.required_checks.contains(binding)
                            && (receipt.checks.iter().any(|check| check.check == *binding)
                                || receipt.skipped_checks.contains(id))
                    })
            })
            .collect::<Vec<_>>();
        if !observed.is_empty() {
            row.freshness = AcceptanceFreshness::Current;
            row.evidence_ids = observed.iter().map(|record| record.id.clone()).collect();
            let mut results = Vec::new();
            let mut strong = false;
            let mut skipped = false;
            for record in observed {
                let receipt = native_receipt(record).expect("validated native receipt");
                row.level = Some(
                    if receipt.level == "full" || row.level.as_deref() == Some("full") {
                        "full".into()
                    } else {
                        "quick".into()
                    },
                );
                strong |= receipt.satisfies_level(minimum_level(input));
                skipped |= receipt.skipped_checks.contains(id);
                if !reuse_valid(input, receipt) {
                    rules::reason(
                        reasons,
                        "cached_proof_source_missing",
                        Some(id),
                        AcceptanceAction::RunVerification,
                    );
                    results.push(EvidenceResult::Inconclusive);
                } else if let Some(item) =
                    receipt.checks.iter().find(|check| check.check == *binding)
                {
                    match item.execution {
                        VerificationCheckExecution::Executed => results.push(item.result),
                        VerificationCheckExecution::Unavailable
                        | VerificationCheckExecution::TimedOut => {
                            row.execution = AcceptanceExecution::Unavailable;
                            rules::reason(
                                reasons,
                                if item.execution == VerificationCheckExecution::TimedOut {
                                    "required_check_timed_out"
                                } else {
                                    "required_check_execution_unavailable"
                                },
                                Some(id),
                                AcceptanceAction::RunVerification,
                            );
                        }
                        VerificationCheckExecution::Unknown => {
                            rules::reason(
                                reasons,
                                "required_check_execution_unknown",
                                Some(id),
                                AcceptanceAction::RunVerification,
                            );
                        }
                    }
                }
            }
            row.execution = if !results.is_empty() {
                AcceptanceExecution::Executed
            } else if skipped {
                AcceptanceExecution::Skipped
            } else {
                row.execution
            };
            if let Some(result) = results
                .into_iter()
                .max_by_key(|result| crate::evidence::result_severity(*result))
            {
                row.outcome = result.into();
                match result {
                    EvidenceResult::Fail => rules::reason(
                        reasons,
                        "native_check_failed",
                        Some(id),
                        AcceptanceAction::InspectFailure,
                    ),
                    EvidenceResult::Pass => {}
                    _ => rules::reason(
                        reasons,
                        "native_check_inconclusive",
                        Some(id),
                        AcceptanceAction::RunVerification,
                    ),
                }
            }
            if !strong {
                rules::reason(
                    reasons,
                    "minimum_level_unproved",
                    Some(id),
                    AcceptanceAction::RunVerification,
                );
            }
            if skipped {
                rules::reason(
                    reasons,
                    "required_check_skipped",
                    Some(id),
                    AcceptanceAction::RunVerification,
                );
            }
        } else {
            let old = input
                .evidence
                .iter()
                .filter(|record| {
                    relevant(record)
                        && native_receipt(record)
                            .is_some_and(|receipt| receipt.required_checks.contains(binding))
                })
                .collect::<Vec<_>>();
            if !old.is_empty() {
                row.evidence_ids = old.iter().map(|record| record.id.clone()).collect();
                row.freshness = if old.iter().any(|record| {
                    record.execution_git_binding.is_none()
                        || record.execution_policy_binding.is_none()
                }) {
                    AcceptanceFreshness::Unbound
                } else {
                    AcceptanceFreshness::Stale
                };
                rules::reason(
                    reasons,
                    if row.freshness == AcceptanceFreshness::Stale {
                        "check_evidence_stale"
                    } else {
                        "check_evidence_unbound"
                    },
                    Some(id),
                    AcceptanceAction::RunVerification,
                );
            } else {
                rules::reason(
                    reasons,
                    "required_check_not_executed",
                    Some(id),
                    AcceptanceAction::RunVerification,
                );
            }
        }
        row.evidence_ids.sort();
        row.evidence_ids.dedup();
        ensure!(
            row.evidence_ids.len() <= 4096,
            "acceptance check Evidence identity budget exceeded"
        );
        rows.push(row);
    }
    let native_matrix = required
        .values()
        .filter_map(Clone::clone)
        .collect::<Vec<_>>();
    let aggregate = status.is_some_and(|status| {
        current.iter().any(|record| {
            record.subject == status.plan.subject
                && record.kind == EvidenceKind::Verification
                && native_receipt(record).is_some_and(|receipt| {
                    receipt.satisfies_level(minimum_level(input))
                        && receipt.covers(&native_matrix)
                        && native_matrix.len() == required.len()
                        && receipt.result() == EvidenceResult::Pass
                        && reuse_valid(input, receipt)
                })
        })
    });
    if !aggregate {
        rules::reason(
            reasons,
            "native_aggregate_coverage_missing",
            None,
            AcceptanceAction::RunVerification,
        );
    }
    Ok(rows)
}

pub(in crate::verification::acceptance) fn summarize(
    rows: &[AcceptanceCheck],
) -> AcceptanceSummary {
    let mut summary = AcceptanceSummary::default();
    for row in rows {
        summary.required += usize::from(row.required);
        summary.discovered += usize::from(row.discovered);
        summary.mapped += usize::from(row.mapped);
        summary.executed += usize::from(row.execution == AcceptanceExecution::Executed);
        summary.passed += usize::from(
            row.outcome == AcceptanceOutcome::Pass && row.freshness == AcceptanceFreshness::Current,
        );
        summary.failed += usize::from(
            row.outcome == AcceptanceOutcome::Fail && row.freshness == AcceptanceFreshness::Current,
        );
        summary.skipped += usize::from(row.execution == AcceptanceExecution::Skipped);
        summary.unavailable += usize::from(row.execution == AcceptanceExecution::Unavailable);
        summary.stale += usize::from(row.freshness == AcceptanceFreshness::Stale);
        summary.unknown += usize::from(row.outcome == AcceptanceOutcome::Unknown);
    }
    summary
}
