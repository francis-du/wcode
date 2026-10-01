//! Shared real Git/Node native-capture fixtures. Operator/model inputs below the
//! transport boundary are explicit fixtures; no execution Evidence is invented.
use crate::design::{AcceptancePolicy, PolicyLevel, PolicyRequirements, ProjectDesign};
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::verification::acceptance::{AcceptanceState, ChangeAcceptanceRecord};
use crate::verification::acceptance_native::NativeAcceptanceRecord;
use crate::verification::change::GitChangeTarget;
use crate::verification::{ReviewSubmission, ReviewVerdict, VerificationPlan};
use crate::workspace::Workspace;
use std::fs;
use std::path::Path;

pub const ID: &str = "native-acceptance-fixture";
pub struct NativeFixture {
    pub root: tempfile::TempDir,
    pub workspace: Workspace,
    pub harness: ToolHarness,
    pub base: String,
    pub head: String,
}
pub fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub fn commit(root: &Path, message: &str) {
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
            "--allow-empty",
            "-qm",
            message,
        ],
    );
}
pub fn operator_receipt() -> crate::verification::policy_store::OperatorReceipt {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    // Test-only native operator input; this is not a transport approval claim.
    crate::verification::policy_store::OperatorReceipt::new(
        &format!("fixture:{}", uuid::Uuid::new_v4()),
        u64::try_from(now.as_millis()).unwrap(),
    )
    .unwrap()
}
impl NativeFixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("tests")).unwrap();
        fs::write(
            root.path().join("src/value.js"),
            "exports.value = function value() { return 1; };\n",
        )
        .unwrap();
        fs::write(root.path().join("tests/value.test.js"),
            "const test = require('node:test');\nconst assert = require('node:assert/strict');\nconst { value } = require('../src/value.js');\ntest('actual native fixture value', () => assert.equal(value(), 1));\n").unwrap();
        fs::write(root.path().join("package.json"),
            r#"{"name":"native-acceptance-fixture","version":"1.0.0","scripts":{"lint":"node --check src/value.js","test":"node --test tests/value.test.js"}}"#).unwrap();
        let policy = AcceptancePolicy {
            schema_version: 1,
            id: "native-node-baseline".into(),
            version: 1,
            requirements: PolicyRequirements {
                minimum_level: PolicyLevel::Full,
                checks: vec!["node-lint".into(), "node-test".into()],
                stages: Vec::new(),
                reviewers: Vec::new(),
                human_approval: true,
                human_approval_min_risk: None,
            },
            docs_only: None,
            rules: Vec::new(),
        };
        let project = ProjectDesign {
            schema_version: 1,
            name: "Native acceptance fixture".into(),
            description: "Real Node execution plus explicit low-level model/operator fixtures."
                .into(),
            acceptance_policy: Some(policy),
        };
        fs::write(
            root.path().join(".wcode/project.yaml"),
            serde_yaml::to_string(&project).unwrap(),
        )
        .unwrap();
        fs::write(root.path().join(".wcode/design/product.yaml"),
            "schema_version: 1\nid: product:native-fixture\nname: Native acceptance fixture\nvision: Check an exact candidate with native execution.\n").unwrap();
        fs::write(root.path().join(".wcode/design/components.yaml"),
            "- schema_version: 1\n  id: component:value\n  name: Value\n  responsibilities: [Return the declared value]\n  implementation:\n    - kind: file\n      path: src/value.js\n    - kind: file\n      path: README.md\n").unwrap();
        fs::write(root.path().join(".wcode/design/requirements.yaml"),
            "- schema_version: 1\n  id: REQ-VALUE\n  title: Value\n  intent: Return one and describe the behavior.\n  priority: low\n  implemented_by: [component:value]\n  acceptance: [AC-VALUE]\n").unwrap();
        fs::write(root.path().join(".wcode/design/acceptance.yaml"),
            "- schema_version: 1\n  id: AC-VALUE\n  title: Native value checks\n  statement: Syntax validation and the actual value test pass.\n  verification:\n    - kind: check\n      id: node-lint\n    - kind: check\n      id: node-test\n").unwrap();
        fs::write(root.path().join("README.md"), "# Before\n").unwrap();
        git(root.path(), &["init", "-q"]);
        git(root.path(), &["add", "."]);
        commit(root.path(), "base fixture");
        let base = git(root.path(), &["rev-parse", "HEAD"]);
        fs::write(root.path().join("README.md"), "# Candidate\n").unwrap();
        git(root.path(), &["add", "README.md"]);
        commit(root.path(), "candidate fixture");
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
    pub fn target(&self) -> GitChangeTarget {
        GitChangeTarget::Commit {
            revision: self.head.clone(),
        }
    }
    pub fn activate(&self) {
        let snapshot = self
            .harness
            .acceptance_policy_preview(ID, &self.workspace)
            .unwrap();
        assert!(snapshot.mappings.complete);
        self.harness
            .acceptance_policy_activate_authorized(
                ID,
                &self.workspace,
                0,
                snapshot,
                operator_receipt(),
                None,
            )
            .unwrap();
        assert_eq!(
            self.harness
                .acceptance_policy_status(ID, &self.workspace)
                .unwrap()["status"],
            "active"
        );
    }
    pub async fn capture(&self) -> NativeAcceptanceRecord {
        self.harness
            .capture_acceptance(ID, &self.workspace, &self.base, self.target())
            .await
            .unwrap()
    }
    pub fn submit_fixture_reviews(&self, plan: &VerificationPlan) {
        let status = self
            .harness
            .verification_status(ID, &self.workspace, &plan.id)
            .unwrap();
        for queued in status.jobs {
            let reviewer = format!("fixture-reviewer:{:?}", queued.role);
            let claimed = self
                .harness
                .verification_claim(
                    ID,
                    &self.workspace,
                    &reviewer,
                    &queued.required_capabilities,
                    Some(queued.role),
                )
                .unwrap();
            assert_eq!(claimed.plan_id, plan.id);
            // A self-reported model-review fixture, never NativeVerification.
            self.harness.verification_submit(ID, &self.workspace, &claimed.id, &reviewer,
                ReviewSubmission { verdict: ReviewVerdict::Pass,
                    summary: "Fixture review of the docs-only candidate; native checks remain independently required.".into(),
                    claims: Vec::new(), risks: Vec::new(), model: Some("fixture/model".into()) },
            ).unwrap();
        }
    }
    pub async fn verify_and_review(&self) -> NativeAcceptanceRecord {
        let monitor = TaskMonitor::new([ID.to_owned()]);
        let executed = self
            .harness
            .acceptance_verify(ID, &self.workspace, &self.base, self.target(), 60, &monitor)
            .await
            .unwrap();
        assert_ne!(executed.record().state, AcceptanceState::Ready);
        let plan_id = executed
            .record()
            .plan
            .as_ref()
            .expect("native plan must exist")
            .id
            .clone();
        let plan = self
            .harness
            .verification_status(ID, &self.workspace, &plan_id)
            .unwrap()
            .plan;
        assert!(plan.require_human_approval);
        self.submit_fixture_reviews(&plan);
        let binding = self
            .harness
            .execution_git_binding(&self.workspace)
            .await
            .unwrap();
        assert!(binding
            .as_ref()
            .is_some_and(|binding| !binding.dirty && binding.head_sha == self.head));
        self.harness
            .verification_approve_authorized_bound(
                ID,
                &self.workspace,
                &plan.id,
                "fixture-local-operator",
                "Approve this exact native plan after actual lint and test execution.",
                binding,
            )
            .unwrap();
        let ready = self.capture().await;
        assert_ready(ready.record());
        let evidence = crate::evidence_store::load(&self.workspace).unwrap();
        for id in ["node-lint", "node-test"] {
            assert!(
                evidence
                    .iter()
                    .any(|record| record.subject == format!("verification:{id}")
                        && record.authority
                            == crate::evidence::EvidenceAuthority::NativeVerification
                        && record.result == crate::evidence::EvidenceResult::Pass
                        && record.execution_receipt.is_some()),
                "real execution receipt required: {id}"
            );
        }
        ready
    }
}
pub fn assert_ready(record: &ChangeAcceptanceRecord) {
    assert_eq!(
        record.state,
        AcceptanceState::Ready,
        "{}",
        serde_json::to_string_pretty(record).unwrap()
    );
    assert!(!record.partial);
    assert!(record.git.complete);
    assert!(record.policy.is_some());
}
