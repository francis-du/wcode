use super::*;

#[test]
fn shell_profile_parser_reads_literals_and_rejects_expansions() {
    let profile = r#"
export JEV_API_KEY='secret-value'
JEV_BASE_URL="https://api.typesafe.ai"
export JEV_DEFAULT_MODEL=jev-latest
"#;
    assert_eq!(
        parse_shell_profile_value(profile, "JEV_API_KEY").as_deref(),
        Some("secret-value")
    );
    assert_eq!(
        parse_shell_profile_value(profile, "JEV_BASE_URL").as_deref(),
        Some("https://api.typesafe.ai")
    );
    for value in [
        "export JEV_API_KEY=$SECRET",
        "export JEV_API_KEY=$(security find-generic-password -w)",
        "export JEV_API_KEY=\"$SECRET\"",
        "export JEV_API_KEY=`command`",
    ] {
        assert_eq!(parse_shell_profile_value(value, "JEV_API_KEY"), None);
    }
}

#[test]
fn response_mapping_preserves_jev_semantics() {
    let request = DecisionRequest {
        scope: "agent_context".into(),
        state: json!({}),
    };
    let batch = response_to_batch(
        &request,
        &json!({
            "answers": {
                "context_sufficient": {"type":"noul","noul":0.73},
                "next_action": {"type":"choice","choice":"semantic_navigation","confidence":0.82,"probabilities":{"semantic_navigation":0.82,"retrieve":0.18}},
                "capability_group": {"type":"choice","choice":"governance","confidence":0.88,"probabilities":{"governance":0.88,"none":0.12}},
                "evidence_density": {"type":"score","score":3.0,"confidence":0.75,"legend":{"0":"a","1":"b","2":"c","3":"d","4":"e"},"probabilities":{"2":0.25,"3":0.75}}
            }
        }),
    )
    .unwrap();
    assert_eq!(batch.provider, "jev-v1");
    assert_eq!(batch.scope, "agent_context");
    assert!(!batch.policy.can_reduce_safety);
    assert!(batch.policy.deterministic_verification_floor);
    assert_eq!(probability(&batch, "context_sufficient"), Some(730));
    let sufficient = batch
        .signals
        .iter()
        .find(|signal| signal.id == "context_sufficient")
        .unwrap();
    assert_eq!(sufficient.confidence_milli, 0);
    assert_eq!(sufficient.native_confidence_milli, None);
    let next = batch
        .signals
        .iter()
        .find(|signal| signal.id == "next_action")
        .unwrap();
    assert_eq!(next.native_confidence_milli, Some(820));
    assert_eq!(next.probabilities_milli["semantic_navigation"], 820);
    assert_eq!(
        crate::decision::distribution_top1_margin_milli(next),
        Some(640)
    );
    assert!(crate::decision::distribution_entropy_milli(next).is_some());
    assert_eq!(choice(&batch, "capability_group"), Some("governance"));
}

#[test]
fn plain_http_requires_loopback() {
    assert!(JevConfig::new(
        "secret".into(),
        "http://example.com".into(),
        JEV_DEFAULT_MODEL.into(),
        10,
    )
    .is_err());
    let config = JevConfig::new(
        "secret".into(),
        "http://127.0.0.1:8080".into(),
        JEV_DEFAULT_MODEL.into(),
        10,
    )
    .unwrap();
    assert_eq!(config.api_key, "secret");
    assert_eq!(config.base_url, "http://127.0.0.1:8080");
    assert_eq!(config.model, JEV_DEFAULT_MODEL);
    assert_eq!(config.timeout_secs, 10);
    assert_eq!(JEV_DEFAULT_BASE_URL, "https://api.typesafe.ai");
}

#[test]
fn external_guidance_cannot_reduce_deterministic_work() {
    fn batch(next_action: &str, confidence_milli: u16) -> DecisionBatch {
        DecisionBatch {
            schema_version: DECISION_SCHEMA_VERSION.into(),
            provider: "fixture".into(),
            scope: "agent_context".into(),
            policy: DecisionPolicy {
                authority: "advisory_only".into(),
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
                confidence_milli,
                native_confidence_milli: Some(confidence_milli),
                probabilities_milli: std::collections::BTreeMap::new(),
                recommendation: "fixture".into(),
                evidence: vec![],
            }],
        }
    }

    let baseline = batch("retrieve", 1000);
    let candidate_edit = batch("edit", 900);
    assert!(advisory_guidance(&baseline, &candidate_edit)
        .iter()
        .all(|item| item != "jev:next_action:edit"));

    let candidate_edit_then_verify = batch("edit_then_verify", 900);
    assert!(advisory_guidance(&baseline, &candidate_edit_then_verify)
        .iter()
        .all(|item| item != "jev:next_action:edit_then_verify"));

    let candidate_retrieve = batch("retrieve", 900);
    assert!(advisory_guidance(&baseline, &candidate_retrieve)
        .iter()
        .any(|item| item == "jev:next_action:retrieve"));

    let candidate_uncertain = batch("edit", 500);
    assert!(advisory_guidance(&baseline, &candidate_uncertain)
        .iter()
        .any(|item| item == "jev:choice_uncertain_collect_evidence"));
}

#[test]
fn capability_group_guidance_requires_concentrated_supported_choice() {
    fn group_batch(selected: &str, confidence_milli: u16) -> DecisionBatch {
        DecisionBatch {
            schema_version: DECISION_SCHEMA_VERSION.into(),
            provider: "fixture".into(),
            scope: "agent_context".into(),
            policy: DecisionPolicy {
                authority: "advisory_only".into(),
                can_increase_work: true,
                can_reduce_safety: false,
                deterministic_verification_floor: true,
            },
            signals: vec![DecisionSignal {
                id: "capability_group".into(),
                primitive: DecisionPrimitive::Choice,
                mode: DecisionMode::Assist,
                value: DecisionValue::Choice {
                    selected: selected.into(),
                },
                confidence_milli,
                native_confidence_milli: Some(confidence_milli),
                probabilities_milli: std::collections::BTreeMap::new(),
                recommendation: "fixture".into(),
                evidence: vec![],
            }],
        }
    }

    let baseline = group_batch("none", 1000);
    assert!(
        advisory_guidance(&baseline, &group_batch("governance", 900))
            .iter()
            .any(|item| item == "jev:capability_group:governance")
    );
    assert!(
        advisory_guidance(&baseline, &group_batch("governance", 500))
            .iter()
            .all(|item| item != "jev:capability_group:governance")
    );
    assert!(
        advisory_guidance(&baseline, &group_batch("repository_write", 900))
            .iter()
            .all(|item| !item.starts_with("jev:capability_group:"))
    );
}

#[test]
fn jev_attachment_never_breaks_agent_context_budget() {
    let mut context = json!({
        "budget": 16,
        "payload": "x".repeat(31),
    });
    assert!(context_fits_budget(&context));
    attach_jev_if_budget_allows(
        &mut context,
        json!({"status":"active","guidance":["retrieve_more_evidence"]}),
    );
    assert!(context.get("jev").is_none());
    assert!(context_fits_budget(&context));

    let mut roomy = json!({"budget": 128, "payload": "x"});
    attach_jev_if_budget_allows(
        &mut roomy,
        json!({"status":"active","guidance":["retrieve_more_evidence"]}),
    );
    assert_eq!(roomy["jev"]["status"], "active");
    assert!(context_fits_budget(&roomy));
}

#[test]
fn active_jev_can_only_promote_additional_work_before_edit() {
    let mut context = json!({
        "budget": 1000,
        "targets": [{"id":"target","path":"src/a.rs"}],
        "hot_source": [{"id":"target","path":"src/a.rs"}],
        "readiness": {
            "next_actions": ["apply_edits","review_changes","verify_project"],
            "advisories": ["lsp_install_required"]
        },
        "capabilities": {
            "recommended_tool_count": 1,
            "recommended_tools": ["agent_context"],
            "recommended_actions": [{"tool":"agent_context","group":"context","disclosure":"core"}]
        }
    });
    let routing = apply_agent_context_guidance(
        &mut context,
        &json!({
            "status":"active",
            "candidate_next_action":"semantic_navigation",
            "guidance":[
                "jev:prefer_semantic_navigation",
                "jev:next_action:review_worktree",
                "jev:preserve_or_raise_verification"
            ]
        }),
    );
    assert_eq!(routing["applied"], true);
    assert_eq!(
        context["readiness"]["next_actions"],
        json!([
            "review_changes",
            "semantic_provider_install",
            "semantic_provider_refresh",
            "semantic_navigation",
            "apply_edits",
            "verification_plan",
            "verify_project"
        ])
    );
    assert_eq!(context["readiness"]["decision_assist"]["provider"], "jev");
    assert_eq!(
        context["readiness"]["decision_assist"]["authority"],
        "increase_only"
    );
    assert!(context["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "jev_semantic_navigation"));
    assert!(context["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "jev_worktree_review"));
    let recommended = context["capabilities"]["recommended_tools"]
        .as_array()
        .unwrap();
    assert!(recommended.iter().any(|tool| tool == "agent_context"));
    for promoted in [
        "review_changes",
        "semantic_provider_install",
        "semantic_provider_refresh",
        "semantic_navigation",
        "verification_plan",
    ] {
        assert!(
            recommended.iter().any(|tool| tool == promoted),
            "missing Jev-promoted tool {promoted}"
        );
    }
    assert!(context["capabilities"]["recommended_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["tool"] == "semantic_navigation"
            && action["group"] == "semantics"
            && action["disclosure"] == "on_demand"));
}

#[test]
fn jev_capability_group_promotes_only_read_advisory_tools() {
    let mut context = json!({
        "budget": 1000,
        "targets": [{"id":"target","path":"src/a.rs"}],
        "hot_source": [{"id":"target","path":"src/a.rs"}],
        "readiness": {"next_actions":["apply_edits","verify_project"],"advisories":[]},
        "capabilities": {
            "recommended_tool_count": 1,
            "recommended_tools": ["agent_context"],
            "recommended_actions": [{"tool":"agent_context","group":"context","disclosure":"core"}]
        }
    });
    let routing = apply_agent_context_guidance(
        &mut context,
        &json!({
            "status":"active",
            "candidate_next_action":"edit_then_verify",
            "guidance":["jev:capability_group:governance"]
        }),
    );
    assert_eq!(routing["applied"], true);
    assert_eq!(routing["actions"], json!([]));
    assert_eq!(routing["capability_group"], "governance");
    assert_eq!(
        context["readiness"]["next_actions"],
        json!(["apply_edits", "verify_project"])
    );
    let recommended = context["capabilities"]["recommended_tools"]
        .as_array()
        .unwrap();
    for promoted in [
        "design_status",
        "traceability_status",
        "impact_analysis",
        "risk_status",
    ] {
        assert!(recommended.iter().any(|tool| tool == promoted));
    }
    for forbidden in [
        "run_command",
        "apply_edits",
        "apply_file_edits",
        "write_file",
    ] {
        assert!(!recommended.iter().any(|tool| tool == forbidden));
    }
}

#[test]
fn unsupported_jev_capability_group_is_inert() {
    let mut context = json!({
        "budget": 1000,
        "targets": [],
        "hot_source": [],
        "readiness": {"next_actions":["apply_edits","verify_project"],"advisories":[]},
        "capabilities": {
            "recommended_tool_count": 1,
            "recommended_tools": ["agent_context"],
            "recommended_actions": [{"tool":"agent_context","group":"context","disclosure":"core"}]
        }
    });
    let before = context.clone();
    let routing = apply_agent_context_guidance(
        &mut context,
        &json!({
            "status":"active",
            "candidate_next_action":"edit_then_verify",
            "guidance":["jev:capability_group:repository_write"]
        }),
    );
    assert_eq!(routing["applied"], false);
    assert_eq!(context, before);
}

#[test]
fn jev_retrieval_routes_to_bounded_evidence_without_dropping_existing_actions() {
    let mut context = json!({
        "budget": 1000,
        "targets": [{"id":"target","path":"src/a.rs"}],
        "hot_source": [],
        "readiness": {
            "next_actions": ["apply_file_edits","review_changes","verify_project"],
            "advisories": []
        }
    });
    let routing = apply_agent_context_guidance(
        &mut context,
        &json!({
            "status":"active",
            "candidate_next_action":"retrieve",
            "guidance":["jev:retrieve_more_evidence"]
        }),
    );
    assert_eq!(routing["actions"], json!(["symbol_context"]));
    assert_eq!(
        context["readiness"]["next_actions"],
        json!([
            "symbol_context",
            "apply_file_edits",
            "review_changes",
            "verify_project"
        ])
    );
}

#[test]
fn jev_routing_is_inert_when_disabled_or_over_budget() {
    let original = json!({
        "budget": 1000,
        "targets": [{"id":"target","path":"src/a.rs"}],
        "hot_source": [{"id":"target","path":"src/a.rs"}],
        "readiness": {"next_actions":["apply_edits","verify_project"],"advisories":[]}
    });
    let mut disabled = original.clone();
    let routing = apply_agent_context_guidance(
        &mut disabled,
        &json!({"status":"disabled","guidance":["jev:prefer_semantic_navigation"]}),
    );
    assert_eq!(routing["applied"], false);
    assert_eq!(disabled, original);

    let mut tiny = json!({
        "budget": 20,
        "payload":"x".repeat(40),
        "targets": [{"id":"target","path":"src/a.rs"}],
        "hot_source": [{"id":"target","path":"src/a.rs"}],
        "readiness": {"next_actions":["apply_edits"],"advisories":[]},
        "capabilities": {
            "recommended_tool_count": 1,
            "recommended_tools": ["agent_context"],
            "recommended_actions": [{"tool":"agent_context","group":"context","disclosure":"core"}]
        }
    });
    let before = tiny.clone();
    let routing = apply_agent_context_guidance(
        &mut tiny,
        &json!({
            "status":"active",
            "candidate_next_action":"semantic_navigation",
            "guidance":["jev:prefer_semantic_navigation"]
        }),
    );
    assert_eq!(routing["applied"], false);
    assert_eq!(routing["reason"], "context_budget");
    assert_eq!(tiny, before);
}

#[test]
fn questions_cover_probability_choice_and_score() {
    let questions = agent_context_questions();
    assert_eq!(questions["context_sufficient"]["type"], "noul");
    assert_eq!(questions["next_action"]["type"], "choice");
    assert_eq!(questions["capability_group"]["type"], "choice");
    assert!(questions["capability_group"]["criteria"]
        .get("repository_write")
        .is_none());
    for group in questions["capability_group"]["criteria"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|group| group.as_str() != "none")
    {
        assert!(
            !crate::harness::model_tools_for_group(group).is_empty(),
            "Jev group {group} is not backed by the Action Registry"
        );
    }
    assert!(questions["next_action"]["criteria"].get("edit").is_none());
    assert!(
        questions["next_action"]["criteria"]["semantic_navigation"]["use_when"]
            .as_str()
            .is_some_and(|value| value.contains("necessary evidence"))
    );
    assert_eq!(questions["semantic_navigation_required"]["type"], "noul");
    assert_eq!(questions["risk_surface"]["type"], "choice");
    assert_eq!(questions["evidence_density"]["type"], "score");
    assert_eq!(AGENT_CONTEXT_QUESTION_SET_ID, "wcode.agent_context");
    assert_eq!(AGENT_CONTEXT_QUESTION_SET_VERSION, 5);
}
