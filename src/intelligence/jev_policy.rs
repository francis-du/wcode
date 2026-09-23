use super::*;

pub(crate) fn agent_context_questions() -> Value {
    json!({
        "context_sufficient": {
            "type": "noul",
            "instructions": "Using only the bounded repository state supplied here, is there enough precise evidence to start the intended localized edit without broadening retrieval?",
            "criteria": {
                "true": "Targets, source context, current edit preconditions and verification references are specific enough that no missing repository fact could materially change the target, safety boundary or verification plan.",
                "false": "An exact source, worktree, design, semantic or verification fact is still missing or ambiguous enough to change the target, safety boundary or verification plan."
            }
        },
        "continue_retrieval": {
            "type": "noul",
            "instructions": "Is additional bounded repository retrieval necessary before a safe localized edit?",
            "criteria": {
                "true": "Another exact source, worktree, design, semantic or verification fact is required because it could materially change the edit or its safety.",
                "false": "The target, current source, edit preconditions and verification route are already sufficiently established; unrelated or merely nice-to-have context does not count."
            }
        },
        "semantic_navigation_required": {
            "type": "noul",
            "instructions": "Is semantic navigation such as callers, callees, references, implementations or hover necessary evidence before a safe localized edit?",
            "criteria": {
                "true": "Relationship information is required to understand impact, resolve the intended symbol or locate the correct implementation before editing.",
                "false": "Ordinary source or test retrieval, worktree review, or the already-localized target is sufficient; semantic navigation would only be optional extra context."
            }
        },
        "verification_escalation_value": {
            "type": "noul",
            "instructions": "Must verification depth be preserved or increased because of semantic risk?",
            "criteria": {
                "true": "Observed risk, uncertainty or cross-boundary impact justifies preserving or increasing deterministic verification work.",
                "false": "No additional semantic risk in this bounded state justifies verification beyond the deterministic plan already selected."
            }
        },
        "next_action": {
            "type": "choice",
            "instructions": "Which bounded next action best fits this coding-agent state? This is advisory only and must not reduce deterministic safety, worktree review or verification.",
            "criteria": {
                "retrieve": {
                    "use_when": "Exact source, tests, design, schema or other repository evidence required for a safe edit is still missing.",
                    "do_not_use_when": "The primary missing evidence is a caller/reference/implementation relationship or the worktree must be reviewed first."
                },
                "semantic_navigation": {
                    "use_when": "Caller, callee, reference, implementation or hover relationships are necessary evidence before editing.",
                    "do_not_use_when": "Ordinary retrieval or worktree review is the actual missing step, or the edit is already localized."
                },
                "review_worktree": {
                    "use_when": "Existing target changes, ownership, staging or merge state must be understood before editing.",
                    "do_not_use_when": "The relevant worktree state is already known safe for the localized edit."
                },
                "edit_then_verify": {
                    "use_when": "The exact target, current source, edit preconditions and verification route are sufficiently established for a localized edit.",
                    "do_not_use_when": "Any required retrieval, semantic relationship or worktree review remains."
                },
                "other_review": {
                    "use_when": "The bounded state is too underspecified or none of the other actions safely fits.",
                    "do_not_use_when": "One of the other bounded actions is clearly supported."
                }
            }
        },
        "capability_group": {
            "type": "choice",
            "instructions": "Which optional read/advisory capability group would most improve the next bounded step? This may only expose additional tools; it never authorizes mutation, command execution, or weaker verification.",
            "criteria": {
                "repository_read": "More bounded source, test, path, or structural repository evidence would help.",
                "semantics": "Symbol relationships, definitions, references, implementations, or provider status would help.",
                "graph": "A bounded software-graph snapshot, query, or history view would help.",
                "governance": "Design, traceability, impact, risk, or convention evidence would help.",
                "verification": "Verification-plan, status, or evidence inspection would help.",
                "execution": "Execution, Worklist, or execution-policy state would help.",
                "reconciliation": "Reconciliation plan or convergence state would help.",
                "quality": "Declared language-quality capability/status would help.",
                "runtime": "Workspace/runtime launch capability metadata would help.",
                "none": "No additional capability group is needed beyond the deterministic task manifest."
            }
        },
        "risk_surface": {
            "type": "choice",
            "instructions": "Which semantic risk surface is most worth checking before or immediately after the localized change? This is a review-priority signal only and never authorizes an edit.",
            "criteria": {
                "stale_state": "Async or cached state may no longer belong to the current request or workspace.",
                "response_contract": "A structurally valid response may not semantically match the request or contract.",
                "workspace_isolation": "State, authorization, or evidence may cross workspace boundaries.",
                "graph_semantics": "Graph relationships, direction, precision, roots, or provenance may be semantically misleading.",
                "verification_gap": "Existing tests may not exercise the real production behavior or failure path.",
                "ui_truthfulness": "The UI may display success, freshness, health, or evidence more strongly than the facts justify.",
                "none": "No specific additional semantic risk surface stands out from the supplied state."
            }
        },
        "evidence_density": {
            "type": "score",
            "instructions": "How dense and edit-ready is the supplied context evidence?",
            "criteria": [
                "Little or no relevant evidence",
                "Some relevant evidence but important gaps remain",
                "Enough localized evidence for a cautious edit",
                "Strong localized evidence plus verification references",
                "Highly edit-ready context with precise targets, source, and checks"
            ]
        }
    })
}

pub(super) fn signal_mode(id: &str) -> DecisionMode {
    match id {
        "continue_retrieval"
        | "semantic_navigation_required"
        | "next_action"
        | "capability_group" => DecisionMode::Assist,
        _ => DecisionMode::Shadow,
    }
}

pub(super) fn recommendation(id: &str) -> &'static str {
    match id {
        "context_sufficient" => "observe_semantic_context_sufficiency",
        "continue_retrieval" => "semantic_retrieval_advice",
        "semantic_navigation_required" => "semantic_navigation_required_before_edit",
        "verification_escalation_value" => "increase_only_verification_advice",
        "next_action" => "typed_jev_routing_without_bypassing_gates",
        "capability_group" => "typed_capability_disclosure_without_new_authority",
        "risk_surface" => "semantic_adversarial_review_priority",
        "evidence_density" => "semantic_context_quality_score",
        _ => "jev_advisory",
    }
}

pub(super) fn score_max_index(id: &str) -> f64 {
    match id {
        "evidence_density" => 4.0,
        _ => 1.0,
    }
}
