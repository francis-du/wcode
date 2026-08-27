use super::*;
use crate::graph::GraphPrecision;
use crate::stage_executor::{execute as execute_stage, StageExecutorSpec};
use crate::workspace::{Workspace, WorkspaceSecurity};
use std::fs;

#[test]
fn design_status_distinguishes_uninitialized_from_invalid() {
    let empty = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(empty.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let status = runtime.design_status("demo", &workspace).unwrap();
    assert!(!status.initialized);
    assert!(!status.valid);
    assert_eq!(status.errors, 0);

    fs::create_dir_all(empty.path().join(".wcode/design/requirements")).unwrap();
    fs::write(
        empty.path().join(".wcode/design/requirements/bad.yaml"),
        "id: REQ-1\ntitle: Missing intent\n",
    )
    .unwrap();
    let status = runtime.design_status("demo", &workspace).unwrap();
    assert!(status.initialized);
    assert!(!status.valid);
    assert!(status.errors > 0);
}

#[test]
fn traces_requirement_to_real_implementation_and_test_symbols() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/requirements")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/components")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/acceptance")).unwrap();
    fs::write(
            dir.path().join("src/lib.rs"),
            "fn secure(path: &str) -> bool { !path.contains(\"..\") }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn blocks_escape() { assert!(!super::secure(\"../secret\")); }\n}\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements/REQ-SEC-001.yaml"),
            "id: REQ-SEC-001\ntitle: Workspace isolation\nintent: Paths must stay inside the workspace.\npriority: critical\nimplemented_by:\n  - component:workspace-security\nacceptance:\n  - AC-SEC-001\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/components/workspace-security.yaml"),
            "id: component:workspace-security\nname: Workspace Security\nimplementation:\n  - kind: symbol\n    path: src/lib.rs\n    symbol: secure\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/acceptance/AC-SEC-001.yaml"),
            "id: AC-SEC-001\ntitle: Escape is blocked\nstatement: Parent traversal is rejected.\nverification:\n  - kind: test\n    path: src/lib.rs\n    symbol: blocks_escape\n",
        )
        .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let status = runtime
        .traceability_status("demo", &workspace, &index, &HashSet::new())
        .unwrap();

    assert!(status.valid_design, "{:?}", status.diagnostics);
    assert_eq!(status.requirement_to_component.percent, 100);
    assert_eq!(status.design_to_implementation.percent, 100);
    assert_eq!(status.acceptance_to_verification.percent, 100);
    assert_eq!(status.complete_requirements, 1);
    assert_eq!(
        status.requirements[0].status,
        RequirementTraceStatus::Complete
    );
    assert!(status.requirements[0]
        .implementation
        .iter()
        .all(|reference| reference.resolved && reference.precision == "syntax"));
    assert!(status.requirements[0]
        .verification
        .iter()
        .all(|reference| reference.resolved && reference.kind == TraceReferenceKind::Test));
}

#[test]
fn software_context_uses_token_scoring_and_real_budget_caps() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design")).unwrap();
    fs::write(
        dir.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements.yaml"),
            "- schema_version: 1\n  id: REQ-AAA-1\n  title: Unrelated analytics\n  intent: Render an unrelated metrics dashboard.\n  implemented_by:\n    - component:noise\n- schema_version: 1\n  id: REQ-SEC-1\n  title: Workspace isolation\n  intent: Keep command execution inside the workspace security boundary.\n  implemented_by:\n    - component:security\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/components.yaml"),
            "- schema_version: 1\n  id: component:noise\n  name: Analytics Noise\n  responsibilities:\n    - render unrelated metrics\n- schema_version: 1\n  id: component:security\n  name: Command Security\n  responsibilities:\n    - enforce workspace command boundaries\n",
        )
        .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let index = CodeIndex::new().unwrap();
    graph_provider_store::persist(
        &workspace,
        &crate::graph::GraphProviderImport {
            provider: "fixture-lsp".into(),
            precision: GraphPrecision::Semantic,
            revision: "sha256:fixture-graph-context".into(),
            nodes: vec![
                crate::graph::GraphImportNode {
                    id: "semantic:workspace-command-guard".into(),
                    kind: NodeKind::Function,
                    label: "workspace_command_guard".into(),
                    attributes: BTreeMap::from([(
                        "path".into(),
                        serde_json::json!("src/security.rs"),
                    )]),
                },
                crate::graph::GraphImportNode {
                    id: "semantic:audit-command".into(),
                    kind: NodeKind::Function,
                    label: "audit_command".into(),
                    attributes: BTreeMap::from([(
                        "path".into(),
                        serde_json::json!("src/audit.rs"),
                    )]),
                },
            ],
            edges: vec![crate::graph::GraphImportEdge {
                from: "semantic:workspace-command-guard".into(),
                to: "semantic:audit-command".into(),
                kind: EdgeKind::Calls,
            }],
        },
    )
    .unwrap();
    let context = runtime
        .software_context(
            "demo",
            &workspace,
            &index,
            &HashSet::new(),
            &SoftwareContextRequest {
                query: "workspace command security".into(),
                intent: "inspect".into(),
                budget: 1_000,
                scopes: vec![],
            },
        )
        .unwrap();
    assert_eq!(context.budget, 1_000);
    assert_eq!(context.requirements, vec!["REQ-SEC-1"]);
    assert_eq!(context.components, vec!["component:security"]);
    assert!(context.requirements.len() <= 4);
    assert_eq!(context.coverage.requirements_returned, 1);
    assert_eq!(context.coverage.requirements[0].id, "REQ-SEC-1");
    assert!(context.coverage.truncated);
    assert!(context
        .graph_context
        .nodes
        .iter()
        .any(|node| node.id == "semantic:workspace-command-guard"));
    assert!(context.graph_context.edges.iter().any(|edge| {
        edge.from == "semantic:workspace-command-guard"
            && edge.to == "semantic:audit-command"
            && edge.kind == EdgeKind::Calls
    }));
}

#[test]
fn software_context_scopes_narrow_source_navigation() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/graph")).unwrap();
    fs::create_dir_all(dir.path().join("src/verification")).unwrap();
    fs::write(
        dir.path().join("src/graph/scoped.rs"),
        "pub fn scope_marker_graph() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/verification/scoped.rs"),
        "pub fn scope_marker_verification() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let index = CodeIndex::new().unwrap();
    let context = runtime
        .software_context(
            "demo",
            &workspace,
            &index,
            &HashSet::new(),
            &SoftwareContextRequest {
                query: "scope marker".into(),
                intent: "inspect".into(),
                budget: 4_000,
                scopes: vec!["software graph".into()],
            },
        )
        .unwrap();

    assert_eq!(context.scopes, vec!["graph"]);
    assert!(!context.symbols.is_empty());
    assert!(context.symbols.iter().all(|symbol| {
        symbol
            .get("path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| path.starts_with("src/graph/"))
    }));
    assert!(context.symbols.iter().any(|symbol| {
        symbol
            .get("qualified_name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name.contains("scope_marker_graph"))
    }));
}

#[test]
fn transitive_call_impact_walks_reverse_callers() {
    use crate::graph::{GraphEdge, GraphNode, GraphPrecision, GraphProvenance, SoftwareGraph};
    let provenance = GraphProvenance {
        provider: "fixture".into(),
        precision: GraphPrecision::Syntax,
        revision: "sha256:fixture".into(),
    };
    let mut graph = SoftwareGraph::default();
    for (id, path, name) in [
        ("symbol:callee", "src/callee.rs", "callee"),
        ("symbol:caller", "src/caller.rs", "caller"),
    ] {
        graph
            .add_node(GraphNode {
                id: id.into(),
                kind: NodeKind::Function,
                label: name.into(),
                attributes: BTreeMap::from([
                    ("path".into(), serde_json::json!(path)),
                    ("qualified_name".into(), serde_json::json!(name)),
                ]),
                provenance: provenance.clone(),
            })
            .unwrap();
    }
    graph
        .add_edge(GraphEdge {
            from: "symbol:caller".into(),
            to: "symbol:callee".into(),
            kind: EdgeKind::Calls,
            provenance,
        })
        .unwrap();
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "tree-sitter".into(),
        precision: crate::graph::GraphPrecision::Syntax,
        files_considered: 2,
        files_indexed: 2,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 2,
        edge_count: 1,
        failures: vec![],
        graph,
    };
    let changed = HashSet::from(["src/callee.rs".to_owned()]);
    let (paths, symbols, callers) = transitive_call_impact(&snapshot, &changed);
    assert!(paths.contains("src/callee.rs"));
    assert!(paths.contains("src/caller.rs"));
    assert!(symbols.contains("src/caller.rs::caller"));
    assert_eq!(callers, 1);
}

#[test]
fn low_risk_verification_becomes_ready_after_deterministic_and_blind_review_pass() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    assert!(plan.automation_gaps.is_empty());
    let job = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-a",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &job.id,
            "reviewer-a",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "Correctness review passed.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model".into()),
            },
        )
        .unwrap();
    runtime
        .record_verification_report(
            "demo",
            &workspace,
            &VerificationReport {
                workspace: "demo".into(),
                level: "quick".into(),
                execution: "fixture".into(),
                phases_run: 1,
                passed: true,
                checks_run: 1,
                checks_failed: 0,
                elapsed_ms: 1,
                summary: "fixture passed".into(),
                checks: vec![crate::harness::VerificationCheck {
                    id: "rust-check".into(),
                    phase: 0,
                    command: "cargo check --locked".into(),
                    reason: "fixture".into(),
                    success: true,
                    exit_code: Some(0),
                    elapsed_ms: 1,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    output_truncated: false,
                }],
            },
        )
        .unwrap();
    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.deterministic_result, Some(EvidenceResult::Pass));
    assert!(status.ready, "{:?}", status.blockers);
    assert!(status.blockers.is_empty());
}

#[test]
fn verification_jobs_resume_in_a_fresh_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let first = SoftwareIntelligenceRuntime::default();
    let plan = first
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    drop(first);

    let second = SoftwareIntelligenceRuntime::default();
    let job = second
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-after-restart",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    assert_eq!(job.plan_id, plan.id);
    let status = second
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.claimed, 1);
}

#[test]
fn required_stage_evidence_replaces_automation_gap_blockers() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    let before = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(before
        .blockers
        .iter()
        .any(|blocker| blocker == "property-evidence-missing"));
    assert!(before
        .blockers
        .iter()
        .any(|blocker| blocker == "mutation-evidence-missing"));

    for stage in [VerificationStage::Property, VerificationStage::Mutation] {
        runtime
            .verification_stage_submit(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage,
                    producer: "external-test-runner".into(),
                    verdict: crate::verification::ReviewVerdict::Pass,
                    summary: format!("{stage:?} stage passed."),
                    artifact_digest: format!("sha256:{stage:?}"),
                    model: None,
                },
            )
            .unwrap();
    }
    let after = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        after.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    assert_eq!(
        after.stage_results.get("mutation"),
        Some(&EvidenceResult::Pass)
    );
    assert!(!after
        .blockers
        .iter()
        .any(|blocker| blocker.contains("property-evidence")));
    assert!(!after
        .blockers
        .iter()
        .any(|blocker| blocker.contains("mutation-evidence")));
    assert!(
        !after.ready,
        "review and deterministic verification are still required"
    );
}

#[test]
fn stage_readiness_aggregates_latest_result_per_producer_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();

    for (producer, verdict) in [
        (
            "property-runner-a",
            crate::verification::ReviewVerdict::Fail,
        ),
        (
            "property-runner-b",
            crate::verification::ReviewVerdict::Pass,
        ),
    ] {
        runtime
            .verification_stage_submit(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage: VerificationStage::Property,
                    producer: producer.into(),
                    verdict,
                    summary: format!("{producer} returned {verdict:?}"),
                    artifact_digest: format!("sha256:{producer}-{verdict:?}"),
                    model: None,
                },
            )
            .unwrap();
    }

    let failed = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        failed.stage_results.get("property"),
        Some(&EvidenceResult::Fail),
        "one producer failure must not be hidden by another producer pass"
    );
    assert_eq!(
        failed.stage_producer_results["property"]["property-runner-a"],
        EvidenceResult::Fail
    );
    assert_eq!(
        failed.stage_producer_results["property"]["property-runner-b"],
        EvidenceResult::Pass
    );
    assert!(failed
        .blockers
        .contains(&"property-evidence-failed".to_owned()));

    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: VerificationStage::Property,
                producer: "property-runner-a".into(),
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "property-runner-a passed after remediation".into(),
                artifact_digest: "sha256:property-runner-a-remediated".into(),
                model: None,
            },
        )
        .unwrap();
    let remediated = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        remediated.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    assert!(!remediated
        .blockers
        .iter()
        .any(|blocker| blocker.starts_with("property-evidence-")));
}

#[tokio::test]
async fn configured_stage_executor_produces_real_persistent_stage_evidence() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    let workspace = Workspace::new_with_security(
        dir.path(),
        false,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    let execution = execute_stage(
        &workspace,
        &StageExecutorSpec {
            id: "fixture-property".into(),
            stage: VerificationStage::Property,
            languages: vec![crate::semantic_provider::SemanticLanguage::Rust],
            program: "rustc".into(),
            args: vec!["--version".into()],
            cwd: ".".into(),
            timeout_seconds: 10,
            builtin: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(execution.verdict, crate::verification::ReviewVerdict::Pass);
    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: execution.stage,
                producer: format!("executor:{}", execution.executor_id),
                verdict: execution.verdict,
                summary: execution.summary,
                artifact_digest: execution.artifact_digest,
                model: None,
            },
        )
        .unwrap();
    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        status.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    let fresh = SoftwareIntelligenceRuntime::default();
    let status = fresh
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        status.stage_results.get("property"),
        Some(&EvidenceResult::Pass),
        "stage evidence must survive runtime restart"
    );
}

#[test]
fn explicit_human_approval_clears_only_the_human_blocker() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    let before = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(before
        .blockers
        .contains(&"human-approval-required".to_owned()));
    runtime
        .verification_approve(
            "demo",
            &workspace,
            &plan.id,
            "operator-a",
            "I reviewed the critical-risk plan and approve proceeding.",
        )
        .unwrap();
    let after = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(after.human_approval);
    assert!(!after
        .blockers
        .contains(&"human-approval-required".to_owned()));
    assert!(after
        .blockers
        .iter()
        .any(|blocker| blocker.ends_with("-evidence-missing")));
    assert!(!after.ready);
}

#[test]
fn reviewer_disagreement_is_persisted_as_evidence_once() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    assert_eq!(plan.job_ids.len(), 2);

    let correctness = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-a",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &correctness.id,
            "reviewer-a",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "No correctness issue found.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model-a".into()),
            },
        )
        .unwrap();

    let maintainability = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-b",
            &["maintainability_review".to_owned()],
            Some(ReviewerRole::Maintainability),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &maintainability.id,
            "reviewer-b",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Fail,
                summary:
                    "The change preserves behavior but adds an avoidable structural regression."
                        .into(),
                claims: vec![],
                risks: vec!["canonical boundary and duplicated branching".into()],
                model: Some("provider/model-b".into()),
            },
        )
        .unwrap();

    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.disagreements, 1);
    let evidence = runtime
        .evidence_status("demo", &workspace, Some(&plan.subject), 100)
        .unwrap();
    assert_eq!(evidence.disagreed, 1);
    assert_eq!(
        evidence
            .evidence
            .iter()
            .filter(|record| record.result == EvidenceResult::Disagree)
            .count(),
        1
    );
}
