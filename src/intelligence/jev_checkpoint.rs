use super::*;

const RUNTIME_CHECKPOINT_QUESTION_SET_ID: &str = "wcode.runtime_checkpoint";
const RUNTIME_CHECKPOINT_QUESTION_SET_VERSION: u16 = 1;

pub(crate) async fn evaluate_checkpoint(
    checkpoint: &str,
    state: &Value,
    baseline_next_action: &str,
) -> Result<Value, String> {
    #[cfg(test)]
    {
        let _ = state;
        Ok(checkpoint_telemetry(
            checkpoint,
            baseline_next_action,
            "disabled",
            None,
            None,
            Vec::new(),
        ))
    }

    #[cfg(not(test))]
    {
        let baseline = checkpoint_baseline(checkpoint, baseline_next_action);
        let provider = match JevDecisionProvider::from_env() {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                return Ok(checkpoint_telemetry(
                    checkpoint,
                    baseline_next_action,
                    "disabled",
                    None,
                    None,
                    Vec::new(),
                ));
            }
            Err(error) => {
                tracing::warn!(%error, checkpoint, "Jev checkpoint configuration rejected");
                return Ok(checkpoint_telemetry(
                    checkpoint,
                    baseline_next_action,
                    "invalid_configuration",
                    None,
                    None,
                    Vec::new(),
                ));
            }
        };

        let request = DecisionRequest {
            scope: format!("runtime_checkpoint:{checkpoint}"),
            state: json!({
                "checkpoint": checkpoint,
                "deterministic_next_action": baseline_next_action,
                "state": state,
            }),
        };
        match provider
            .evaluate_with_questions(
                &request,
                RUNTIME_CHECKPOINT_QUESTION_SET_ID,
                RUNTIME_CHECKPOINT_QUESTION_SET_VERSION,
                checkpoint_questions(checkpoint),
            )
            .await
        {
            Ok(evaluation) => {
                let candidate = evaluation.batch;
                let comparison = compare_decision_batches(&baseline, &candidate);
                let guidance = advisory_guidance(&baseline, &candidate);
                let mut telemetry = checkpoint_telemetry(
                    checkpoint,
                    baseline_next_action,
                    "active",
                    Some(provider.model()),
                    choice(&candidate, "next_action"),
                    guidance,
                );
                telemetry["call"] = evaluation.metrics.as_json();
                telemetry["comparison"] = json!({
                    "shared_signals": comparison.shared_signals,
                    "missing_from_baseline": comparison.missing_from_baseline,
                    "missing_from_candidate": comparison.missing_from_candidate,
                    "choice_disagreements": comparison.choice_disagreement_count,
                    "safety_policy_violations": comparison.safety_policy_violation_count,
                    "shape_mismatches": comparison.primitive_mismatch_count
                        + comparison.mode_mismatch_count
                        + comparison.baseline_duplicate_signal_ids
                        + comparison.candidate_duplicate_signal_ids
                        + usize::from(comparison.scope_mismatch)
                        + usize::from(comparison.schema_mismatch)
                });
                Ok(telemetry)
            }
            Err(error) => {
                tracing::warn!(%error, checkpoint, "Jev checkpoint request failed");
                Ok(checkpoint_telemetry(
                    checkpoint,
                    baseline_next_action,
                    "unavailable",
                    Some(provider.model()),
                    None,
                    Vec::new(),
                ))
            }
        }
    }
}

fn checkpoint_telemetry(
    checkpoint: &str,
    baseline_next_action: &str,
    status: &str,
    model: Option<&str>,
    candidate_next_action: Option<&str>,
    guidance: Vec<String>,
) -> Value {
    json!({
        "provider": "jev",
        "checkpoint": checkpoint,
        "model": model,
        "status": status,
        "authority": "increase_only_assist",
        "baseline_next_action": baseline_next_action,
        "candidate_next_action": candidate_next_action,
        "question_set": {
            "id": RUNTIME_CHECKPOINT_QUESTION_SET_ID,
            "version": RUNTIME_CHECKPOINT_QUESTION_SET_VERSION
        },
        "comparison": {
            "shared_signals": 0,
            "choice_disagreements": usize::from(
                candidate_next_action.is_some_and(|candidate| candidate != baseline_next_action)
            ),
            "safety_policy_violations": 0,
            "shape_mismatches": 0
        },
        "guidance": guidance,
        "fallback": "deterministic"
    })
}

#[cfg(not(test))]
fn checkpoint_baseline(checkpoint: &str, next_action: &str) -> DecisionBatch {
    DecisionBatch {
        schema_version: DECISION_SCHEMA_VERSION.into(),
        provider: "wcode-deterministic".into(),
        scope: format!("runtime_checkpoint:{checkpoint}"),
        policy: DecisionPolicy {
            authority: "deterministic".into(),
            can_increase_work: true,
            can_reduce_safety: false,
            deterministic_verification_floor: true,
        },
        signals: vec![DecisionSignal {
            id: "next_action".into(),
            primitive: DecisionPrimitive::Choice,
            mode: DecisionMode::Assist,
            value: DecisionValue::Choice {
                selected: next_action.into(),
            },
            confidence_milli: 1000,
            native_confidence_milli: None,
            probabilities_milli: std::collections::BTreeMap::new(),
            recommendation: "deterministic_checkpoint_route".into(),
            evidence: vec!["provider:wcode-deterministic".into()],
        }],
    }
}

pub(crate) fn checkpoint_actions(checkpoint: &str, telemetry: &Value) -> Vec<String> {
    let mut actions = Vec::new();
    if telemetry.get("status").and_then(Value::as_str) != Some("active") {
        return with_verification_failure_floor(checkpoint, actions);
    }
    let guidance = telemetry
        .get("guidance")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let candidate = telemetry
        .get("candidate_next_action")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let needs_review = guidance.iter().any(|item| {
        matches!(
            *item,
            "jev:retrieve_more_evidence"
                | "jev:choice_uncertain_collect_evidence"
                | "jev:next_action:retrieve"
                | "jev:next_action:review_worktree"
        )
    }) || matches!(candidate, "retrieve" | "review_worktree");
    let needs_semantic = guidance.iter().any(|item| {
        matches!(
            *item,
            "jev:prefer_semantic_navigation" | "jev:next_action:semantic_navigation"
        )
    }) || candidate == "semantic_navigation";
    let needs_full_verification = guidance.contains(&"jev:preserve_or_raise_verification");

    if needs_review {
        actions.push("review_changes(adversarial=true)".to_owned());
    }
    if needs_semantic {
        actions.push("semantic_navigation(relevant changed/failed symbol)".to_owned());
    }
    if needs_full_verification {
        actions.push("verification_plan".to_owned());
        actions.push("verify_project(level=full)".to_owned());
    }
    with_verification_failure_floor(checkpoint, actions)
}

fn with_verification_failure_floor(checkpoint: &str, mut actions: Vec<String>) -> Vec<String> {
    if checkpoint == "verification_failure" && actions.is_empty() {
        actions.push("repair_from_failed_check_evidence".to_owned());
        actions.push("verify_project(retry_same_level)".to_owned());
    }
    actions
}

#[cfg(not(test))]
fn checkpoint_questions(checkpoint: &str) -> Value {
    let phase = match checkpoint {
        "post_edit_review" => "after the current worktree diff has been inspected and before deterministic verification",
        "verification_failure" => "after deterministic verification failed and before repair/retry",
        "completion_readiness" => "after deterministic verification passed and before claiming the task complete",
        _ => "at a bounded runtime decision checkpoint",
    };
    json!({
        "continue_retrieval": {
            "type": "noul",
            "instructions": format!("At this checkpoint ({phase}), is more bounded repository evidence needed before the next deterministic step?"),
            "criteria": {
                "true": "A missing source, worktree, semantic, design, or failure fact could materially change the safe next action.",
                "false": "The supplied checkpoint evidence is specific enough to continue with the deterministic route."
            }
        },
        "semantic_navigation_required": {
            "type": "noul",
            "instructions": format!("At this checkpoint ({phase}), are callers, callees, references, implementations, or hover relationships necessary evidence?"),
            "criteria": {
                "true": "A cross-file relationship is required to understand impact or failure before continuing.",
                "false": "The supplied diff/failure/verification evidence is sufficient without another semantic lookup."
            }
        },
        "verification_escalation_value": {
            "type": "noul",
            "instructions": format!("At this checkpoint ({phase}), should deterministic verification depth be preserved or increased?"),
            "criteria": {
                "true": "Risk, uncertainty, cross-boundary impact, or the observed failure justifies a stronger deterministic verification route.",
                "false": "The existing deterministic verification route is already sufficient."
            }
        },
        "next_action": {
            "type": "choice",
            "instructions": format!("At this checkpoint ({phase}), which bounded next action is most useful? This is increase-only advice and cannot skip review, repair, or verification."),
            "criteria": {
                "retrieve": "Collect additional bounded repository or failure evidence before proceeding.",
                "semantic_navigation": "Inspect a required cross-file semantic relationship before proceeding.",
                "review_worktree": "Perform another worktree/adversarial review before proceeding.",
                "edit_then_verify": "Continue the deterministic repair/edit then verification route already required by wcode.",
                "fix_then_retry": "Repair the concrete deterministic verification failure, then retry the same verification floor.",
                "verification_plan": "Inspect or strengthen the deterministic verification plan before continuing.",
                "ready_to_finish": "Full deterministic verification passed and the supplied evidence does not justify additional bounded work.",
                "other_review": "Use another bounded review step because the supplied checkpoint is still underspecified."
            }
        },
        "risk_surface": {
            "type": "choice",
            "instructions": "Which risk surface deserves the most additional scrutiny at this checkpoint?",
            "criteria": {
                "stale_state": "The evidence may not belong to the latest source/worktree revision.",
                "response_contract": "A structurally valid result may not satisfy the real contract.",
                "workspace_isolation": "Authorization or evidence may cross workspace boundaries.",
                "graph_semantics": "Relationship direction, precision, roots, or provenance may be misleading.",
                "verification_gap": "Existing checks may not cover the production behavior or failure path.",
                "ui_truthfulness": "The UI may overstate freshness, success, health, or evidence.",
                "none": "No extra risk surface stands out from the supplied state."
            }
        },
        "evidence_density": {
            "type": "score",
            "instructions": "How dense and action-ready is the checkpoint evidence?",
            "criteria": [
                "Little or no useful checkpoint evidence",
                "Some evidence but material gaps remain",
                "Enough evidence for a cautious deterministic next step",
                "Strong evidence plus relevant review/verification signals",
                "Highly action-ready evidence with clear deterministic next steps"
            ]
        }
    })
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/jev_checkpoint.rs"]
mod tests;
