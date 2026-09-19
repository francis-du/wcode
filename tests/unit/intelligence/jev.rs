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
        "readiness": {"next_actions":["apply_edits"],"advisories":[]}
    });
    let before = tiny["readiness"].clone();
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
    assert_eq!(tiny["readiness"], before);
}

#[test]
fn questions_cover_probability_choice_and_score() {
    let questions = agent_context_questions();
    assert_eq!(questions["context_sufficient"]["type"], "noul");
    assert_eq!(questions["next_action"]["type"], "choice");
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
    assert_eq!(AGENT_CONTEXT_QUESTION_SET_VERSION, 4);
}
