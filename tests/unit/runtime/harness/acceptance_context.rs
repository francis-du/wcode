//! Real Git/Graph/Risk inspection fixtures; no check execution Evidence is minted.
use super::*;
use crate::graph::GraphProvenance;
use crate::risk::{Risk, RiskCategory, RiskLevel};
use crate::verification::acceptance::{AcceptanceState, ChangeAcceptanceRecord};
use crate::verification::change::GitChangeTarget;
use serde_json::json;
use std::fs;
use std::path::Path;

const ID: &str = "inspection-fixture";

struct Fixture {
    root: tempfile::TempDir,
    workspace: Workspace,
    harness: ToolHarness,
    base: String,
    head: String,
}

fn git(root: &Path, args: &[&str]) -> String {
    let result = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}

fn commit(root: &Path, message: &str) {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            message,
        ],
    );
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("tests")).unwrap();
        fs::create_dir(root.path().join("assets")).unwrap();
        fs::write(root.path().join("Cargo.toml"),
            "[package]\nname = \"inspection-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lib]\npath = \"src/value.rs\"\n").unwrap();
        fs::write(
            root.path().join("src/value.rs"),
            "pub fn value() -> u8 { 1 }\npub fn untouched() -> u8 { 7 }\n",
        )
        .unwrap();
        fs::write(root.path().join("src/deleted.rs"), "pub fn deleted() {}\n").unwrap();
        fs::write(
            root.path().join("tests/value.rs"),
            "#[test]\nfn value_test() { assert_eq!(1, 1); }\n",
        )
        .unwrap();
        fs::write(
            root.path().join(".wcode/project.yaml"),
            "schema_version: 1\nname: Inspection fixture\n",
        )
        .unwrap();
        fs::write(root.path().join(".wcode/design/product.yaml"),
            "id: product:inspection\nname: Inspection fixture\nvision: Inspect native metadata honestly.\n").unwrap();
        fs::write(root.path().join(".wcode/design/components.yaml"),
            "- id: component:value\n  name: Value\n  implementation:\n    - kind: file\n      path: src/value.rs\n").unwrap();
        fs::write(root.path().join(".wcode/design/requirements.yaml"),
            "- id: REQ-VALUE\n  title: Value\n  intent: Return the candidate value.\n  priority: low\n  implemented_by: [component:value]\n  acceptance: [AC-VALUE]\n").unwrap();
        fs::write(root.path().join(".wcode/design/acceptance.yaml"),
            "- id: AC-VALUE\n  title: Value test\n  statement: The declared test verifies the value.\n  verification:\n    - kind: test\n      path: tests/value.rs\n      symbol: value_test\n    - kind: test\n      path: tests/missing.rs\n      symbol: missing_test\n    - kind: check\n      id: rust-test\n").unwrap();
        git(root.path(), &["init", "-q"]);
        git(root.path(), &["add", "."]);
        commit(root.path(), "baseline");
        let base = git(root.path(), &["rev-parse", "HEAD"]);
        fs::write(
            root.path().join("src/value.rs"),
            "pub fn value() -> u8 { 2 }\npub fn untouched() -> u8 { 7 }\n",
        )
        .unwrap();
        fs::remove_file(root.path().join("src/deleted.rs")).unwrap();
        fs::write(root.path().join("src/orphan.rs"), "pub fn orphan() {}\n").unwrap();
        fs::write(
            root.path().join("assets/data.fixture"),
            "unsupported graph input\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        commit(root.path(), "candidate");
        let head = git(root.path(), &["rev-parse", "HEAD"]);
        let workspace = Workspace::new(root.path(), true, true).unwrap();
        Self {
            root,
            workspace,
            harness: ToolHarness::new(2).unwrap(),
            base,
            head,
        }
    }

    fn target(&self) -> GitChangeTarget {
        GitChangeTarget::Commit {
            revision: self.head.clone(),
        }
    }

    async fn inputs(&self) -> (Revision, GitChangeSnapshot, RiskStatus) {
        let git = self
            .harness
            .git_change_snapshot(&self.workspace, &self.base, self.target())
            .await
            .unwrap();
        assert!(git.complete);
        let revision = self.harness.current_revision(&self.workspace).unwrap();
        let review = harness_policy_select::candidate_review(ID, &git);
        let risk = self
            .harness
            .risk_status(ID, &self.workspace, &review)
            .unwrap();
        (revision, git, risk)
    }

    async fn inspection(&self) -> AcceptanceChangeInspection {
        let (revision, git, risk) = self.inputs().await;
        self.harness
            .acceptance_change_inspection(ID, &self.workspace, &revision, &git, &risk)
            .unwrap()
    }

    async fn record(&self) -> ChangeAcceptanceRecord {
        self.harness
            .capture_acceptance(ID, &self.workspace, &self.base, self.target())
            .await
            .unwrap()
            .record()
            .clone()
    }

    async fn projection_inputs(
        &self,
    ) -> (
        Revision,
        GitChangeSnapshot,
        RiskStatus,
        DesignState,
        ImpactAnalysis,
        SoftwareGraphSnapshot,
    ) {
        let (revision, git, risk) = self.inputs().await;
        let state = self
            .harness
            .intelligence
            .design_load(&self.workspace)
            .unwrap()
            .state
            .clone();
        let review = harness_policy_select::candidate_review(ID, &git);
        let (impact, graph) = self
            .harness
            .intelligence
            .impact_analysis_with_graph_from_snapshot(
                ID,
                &self.workspace,
                &self.harness.code_index,
                &review,
                &state,
                risk.level,
            )
            .unwrap();
        (revision, git, risk, state, impact, graph)
    }
}

#[tokio::test]
async fn native_inspection_symbols_keep_file_membership_and_syntax_precision() {
    let fixture = Fixture::new();
    let inspection = fixture.inspection().await;
    assert!(inspection
        .symbols
        .iter()
        .any(|symbol| symbol.name == "value"));
    let untouched = inspection
        .symbols
        .iter()
        .find(|symbol| symbol.name == "untouched")
        .unwrap();
    assert_eq!(
        untouched.selection,
        AcceptanceSymbolSelection::FileMembership
    );
    assert_eq!(untouched.precision, GraphPrecision::Syntax);
    assert_eq!(untouched.freshness, AcceptanceFreshness::Current);
    assert!(untouched.start_line.is_some());
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "symbol_body_delta_not_captured"));
    assert!(
        !inspection.complete,
        "membership does not establish a symbol body delta"
    );
    assert!(inspection
        .impacted_components
        .contains(&"component:value".into()));
    assert!(inspection
        .impacted_requirements
        .contains(&"REQ-VALUE".into()));
}

#[tokio::test]
async fn native_inspection_preserves_unmapped_deleted_and_unindexed_paths() {
    let fixture = Fixture::new();
    let inspection = fixture.inspection().await;
    assert!(inspection
        .coverage
        .unmapped_paths
        .contains(&"src/orphan.rs".into()));
    assert!(inspection
        .coverage
        .uncovered_paths
        .contains(&"src/deleted.rs".into()));
    assert!(inspection
        .coverage
        .uncovered_paths
        .contains(&"assets/data.fixture".into()));
    assert_eq!(inspection.coverage.changed_paths_total, 4);
    assert_eq!(inspection.coverage.mapped_paths_total, 1);
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "changed_paths_unmapped"));
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "changed_paths_not_indexed"));
}

#[tokio::test]
async fn native_inspection_declared_tests_do_not_become_execution_or_pass() {
    let fixture = Fixture::new();
    let inspection = fixture.inspection().await;
    let test = inspection
        .mapped_verification
        .iter()
        .find(|item| item.target == "tests/value.rs::value_test")
        .unwrap();
    assert!(test.resolved);
    assert_eq!(test.kind, AcceptanceMappingKind::Test);
    assert_eq!(test.relation, "declared_verification");
    let missing = inspection
        .mapped_verification
        .iter()
        .find(|item| item.target == "tests/missing.rs::missing_test")
        .unwrap();
    assert!(!missing.resolved);
    let check = inspection
        .mapped_verification
        .iter()
        .find(|item| item.target == "rust-test")
        .unwrap();
    assert!(check.resolved);
    assert_eq!(check.kind, AcceptanceMappingKind::Check);
    let value = serde_json::to_value(test).unwrap();
    for key in ["execution", "outcome", "pass", "evidence_ids"] {
        assert!(
            value.get(key).is_none(),
            "mapped Test metadata cannot fabricate {key}"
        );
    }
    let record = fixture.record().await;
    assert_ne!(record.state, AcceptanceState::Ready);
    assert_eq!(record.summary.executed, 0);
}

#[tokio::test]
async fn native_inspection_rejects_workspace_revision_and_risk_identity_drift() {
    let fixture = Fixture::new();
    let (revision, git, mut risk) = fixture.inputs().await;
    risk.workspace = "another-workspace".into();
    assert!(fixture
        .harness
        .acceptance_change_inspection(ID, &fixture.workspace, &revision, &git, &risk)
        .is_err());
    risk.workspace = ID.into();
    risk.revision.code.push_str(":partial");
    assert!(fixture
        .harness
        .acceptance_change_inspection(ID, &fixture.workspace, &revision, &git, &risk)
        .is_err());
    risk.revision = revision.clone();
    fs::write(
        fixture.root.path().join("src/value.rs"),
        "pub fn value() -> u8 { 3 }\n",
    )
    .unwrap();
    assert!(fixture
        .harness
        .acceptance_change_inspection(ID, &fixture.workspace, &revision, &git, &risk)
        .is_err());
}

#[tokio::test]
async fn native_inspection_unknown_provider_binding_and_raw_sensitive_text_stay_advisory() {
    let fixture = Fixture::new();
    let (revision, git, mut risk, state, impact, mut graph) = fixture.projection_inputs().await;
    risk.risks.push(Risk {
        id: "RISK-fixture".into(),
        subject: "workspace:fixture".into(),
        category: RiskCategory::Security,
        level: RiskLevel::High,
        summary: "password=fixture-secret".into(),
        signals: vec!["password=fixture-secret".into()],
        guards: vec![],
    });
    graph
        .graph
        .add_node(GraphNode {
            id: "fixture:provider-symbol".into(),
            kind: NodeKind::Function,
            label: "provider_value".into(),
            attributes: BTreeMap::from([
                ("path".into(), json!("src/value.rs")),
                ("name".into(), json!("provider_value")),
                ("signature".into(), json!("password=fixture-secret")),
            ]),
            provenance: GraphProvenance {
                provider: "fixture:lsp".into(),
                precision: GraphPrecision::Semantic,
                revision: "unbound-fixture".into(),
            },
        })
        .unwrap();
    let inspection =
        project_inspection(ID, &revision, &git, &risk, &state, &impact, &graph).unwrap();
    let symbol = inspection
        .symbols
        .iter()
        .find(|item| item.id == "fixture:provider-symbol")
        .unwrap();
    assert_eq!(symbol.precision, GraphPrecision::Semantic);
    assert_eq!(symbol.freshness, AcceptanceFreshness::Unbound);
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "provider_source_binding_unknown"));
    assert!(!serde_json::to_string(&inspection)
        .unwrap()
        .contains("fixture-secret"));
}

#[tokio::test]
async fn native_inspection_output_bounds_keep_totals_and_partial_scan_unknown() {
    let fixture = Fixture::new();
    let (revision, git, risk, state, impact, mut graph) = fixture.projection_inputs().await;
    for index in 0..140 {
        graph
            .graph
            .add_node(GraphNode {
                id: format!("fixture:bounded-{index:03}"),
                kind: NodeKind::Function,
                label: format!("item_{index}"),
                attributes: BTreeMap::from([
                    ("path".into(), json!("src/value.rs")),
                    ("name".into(), json!(format!("item_{index}"))),
                ]),
                provenance: GraphProvenance {
                    provider: "fixture:lsp".into(),
                    precision: GraphPrecision::Semantic,
                    revision: "unbound-fixture".into(),
                },
            })
            .unwrap();
    }
    graph.scan_truncated = true;
    let inspection =
        project_inspection(ID, &revision, &git, &risk, &state, &impact, &graph).unwrap();
    assert_eq!(inspection.symbols.len(), 128);
    assert!(inspection.coverage.symbols_observed >= 142);
    assert_eq!(inspection.coverage.symbols_returned, 128);
    assert!(!inspection.coverage.totals_complete);
    assert!(!inspection.coverage.graph_complete);
    assert!(inspection.truncated && !inspection.complete);
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "graph_capture_incomplete"));
    assert!(serde_json::to_vec(&inspection).unwrap().len() <= BYTE_BUDGET);
}

#[tokio::test]
async fn native_inspection_supplement_is_atomic_and_repeat_digest_is_stable() {
    let fixture = Fixture::new();
    let inspection = fixture.inspection().await;
    let mut record = fixture.record().await;
    let original_state = record.state;
    record.supplement_inspection(inspection.clone()).unwrap();
    let first = record.digest().to_owned();
    let time = record.captured_at_ms;
    record.captured_at_ms = time.saturating_add(1);
    record.supplement_inspection(inspection.clone()).unwrap();
    assert_eq!(record.digest(), first);
    assert_eq!(record.state, original_state);
    let before = serde_json::to_vec(&record).unwrap();
    for drift in 0..5 {
        let mut changed = inspection.clone();
        match drift {
            0 => changed.workspace.push_str("-other"),
            1 => changed.revision.code.push_str(":partial"),
            2 => changed.base_sha = Some("a".repeat(40)),
            3 => changed.target_sha = Some("b".repeat(40)),
            _ => changed
                .git_binding
                .as_mut()
                .unwrap()
                .index_fingerprint
                .push('0'),
        }
        assert!(record.supplement_inspection(changed).is_err());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            before,
            "rejected metadata must not modify Record"
        );
    }
    let second = fixture.record().await;
    assert_eq!(
        second.digest(),
        first,
        "unchanged native recapture keeps stable content identity"
    );
}

#[tokio::test]
async fn native_inspection_partial_revision_and_missing_mappings_remain_unknown() {
    let fixture = Fixture::new();
    let (mut revision, git, mut risk, state, impact, graph) = fixture.projection_inputs().await;
    revision.code.push_str(":partial");
    risk.traceability.truncated = true;
    let inspection =
        project_inspection(ID, &revision, &git, &risk, &state, &impact, &graph).unwrap();
    assert!(!inspection.complete);
    assert!(!inspection.coverage.mappings_complete);
    assert!(!inspection.coverage.totals_complete);
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "revision_incomplete"));
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "traceability_incomplete"));
}

#[tokio::test]
async fn native_inspection_supplement_rejects_oversize_without_mutating_record() {
    let fixture = Fixture::new();
    let mut inspection = fixture.inspection().await;
    let mut record = fixture.record().await;
    let before = serde_json::to_vec(&record).unwrap();
    inspection.symbols = (0..129)
        .map(|index| AcceptanceInspectedSymbol {
            id: index.to_string(),
            path: "src/value.rs".into(),
            name: "value".into(),
            provider: "tree-sitter".into(),
            precision: GraphPrecision::Syntax,
            selection: AcceptanceSymbolSelection::FileMembership,
            freshness: AcceptanceFreshness::Current,
            start_line: Some(1),
            end_line: Some(1),
        })
        .collect();
    assert!(record.supplement_inspection(inspection.clone()).is_err());
    assert_eq!(serde_json::to_vec(&record).unwrap(), before);
    inspection.symbols.clear();
    inspection.unknown_reasons = vec!["x".repeat(BYTE_BUDGET)];
    assert!(record.supplement_inspection(inspection).is_err());
    assert_eq!(serde_json::to_vec(&record).unwrap(), before);
}
#[tokio::test]
async fn native_inspection_reference_budget_is_explicit_not_silent_coverage() {
    let fixture = Fixture::new();
    let (revision, git, risk, mut state, impact, graph) = fixture.projection_inputs().await;
    state.acceptance.get_mut("AC-VALUE").unwrap().verification = (0..4100)
        .map(|index| VerificationRef::Test {
            path: "tests/value.rs".into(),
            symbol: format!("case_{index}"),
        })
        .collect();
    let inspection =
        project_inspection(ID, &revision, &git, &risk, &state, &impact, &graph).unwrap();
    assert_eq!(inspection.coverage.verification_observed, 4096);
    assert_eq!(inspection.coverage.verification_returned, 128);
    assert!(!inspection.coverage.totals_complete);
    assert!(inspection.truncated && !inspection.complete);
    assert!(inspection
        .unknown_reasons
        .iter()
        .any(|reason| reason == "verification_inventory_truncated"));
    assert!(serde_json::to_vec(&inspection).unwrap().len() <= BYTE_BUDGET);
}
