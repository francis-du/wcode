use super::*;
use crate::design::{
    AcceptancePolicy, AcceptanceRule, DocsOnlyPolicy, PolicyLevel, PolicyPath, PolicyRequirements,
    PolicySelector, ProjectDesign,
};
use crate::verification::policy::PolicySnapshot;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const ID: &str = "policy-fixture";

fn requirements(check: &str, level: PolicyLevel) -> PolicyRequirements {
    PolicyRequirements {
        minimum_level: level,
        checks: vec![check.into()],
        stages: Vec::new(),
        reviewers: Vec::new(),
        human_approval: false,
        human_approval_min_risk: None,
    }
}

fn policy() -> AcceptancePolicy {
    AcceptancePolicy {
        schema_version: 1,
        id: "native-baseline".into(),
        version: 1,
        requirements: requirements("node-test", PolicyLevel::Full),
        docs_only: Some(DocsOnlyPolicy {
            paths: vec![PolicyPath::File {
                path: "README.md".into(),
            }],
            require: requirements("node-lint", PolicyLevel::Quick),
        }),
        rules: vec![AcceptanceRule {
            id: "auth-boundary".into(),
            when: PolicySelector {
                paths: Vec::new(),
                components: vec!["component:auth".into()],
                requirements: vec!["REQ-AUTH".into()],
            },
            require: requirements("node-test", PolicyLevel::Full),
        }],
    }
}

fn write_policy(root: &Path, policy: AcceptancePolicy) {
    let project = ProjectDesign {
        schema_version: 1,
        name: "Native Policy fixture".into(),
        description: "Trusted native capture test; no executed check or approval proof.".into(),
        acceptance_policy: Some(policy),
    };
    fs::write(
        root.join(".wcode/project.yaml"),
        serde_yaml::to_string(&project).unwrap(),
    )
    .unwrap();
}

fn fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/auth.js"),
        "export function authorize() { return true; }\n",
    )
    .unwrap();
    fs::write(root.path().join("README.md"), "# Native Policy fixture\n").unwrap();
    fs::write(root.path().join("package.json"),
        r#"{"name":"policy-fixture","scripts":{"test":"node --test","lint":"node --check src/auth.js"}}"#).unwrap();
    fs::write(root.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:fixture\nname: Native Policy fixture\nvision: Capture a trusted local definition.\n").unwrap();
    fs::write(root.path().join(".wcode/design/components.yaml"),
        "- id: component:auth\n  name: Authorization\n  responsibilities: [Authorize requests]\n  implementation:\n    - kind: file\n      path: src/auth.js\n").unwrap();
    fs::write(root.path().join(".wcode/design/requirements.yaml"),
        "- id: REQ-AUTH\n  title: Authorization\n  intent: Enforce the access boundary.\n  implemented_by: [component:auth]\n").unwrap();
    write_policy(root.path(), policy());
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    (root, workspace, ToolHarness::new(2).unwrap())
}

fn preview(harness: &ToolHarness, workspace: &Workspace) -> PolicySnapshot {
    let value = harness.acceptance_policy_preview(ID, workspace).unwrap();
    value.validate().unwrap();
    value
}

fn source_hash(snapshot: &PolicySnapshot, path: &str) -> Option<String> {
    snapshot
        .sources
        .iter()
        .find(|source| source.path == path)
        .unwrap_or_else(|| panic!("native capture must account for {path}"))
        .sha256
        .clone()
}

fn captured_check<'a>(
    snapshot: &'a PolicySnapshot,
    id: &str,
) -> &'a crate::verification::policy::FrozenPolicyCheck {
    snapshot
        .checks
        .iter()
        .find(|check| check.binding.id == id)
        .unwrap()
}

#[test]
fn native_policy_preview_freezes_native_commands_sources_and_exact_file_mappings() {
    let (root, workspace, harness) = fixture();
    let value = preview(&harness, &workspace);
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    assert!(profile.discovery.complete);
    let load = crate::design::load_design(&workspace).unwrap();
    assert_eq!(load.error_count(), 0, "{:?}", load.diagnostics);
    assert_eq!(value.workspace, ID);
    assert_eq!(
        value.revision,
        harness.intelligence.current_revision(&workspace).unwrap()
    );
    assert!(value.revision.design.is_some());
    assert_eq!(value.policy, policy());
    assert!(value.mappings.complete);
    assert_eq!(
        value.mappings.components["component:auth"],
        vec!["src/auth.js"]
    );
    assert_eq!(value.mappings.requirements["REQ-AUTH"], vec!["src/auth.js"]);
    assert_eq!(
        value.checks.len(),
        2,
        "only explicit Policy check IDs are frozen"
    );
    for frozen in &value.checks {
        let native = profile
            .recommended_checks
            .iter()
            .find(|check| check.id == frozen.binding.id)
            .unwrap();
        assert_eq!(frozen.binding, verification_check_binding(native));
        assert_eq!(frozen.program, native.program);
        assert_eq!(frozen.args, native.args);
        assert_eq!(frozen.cwd.as_deref(), Some(native.cwd.as_str()));
        assert_eq!(frozen.island.as_deref(), Some(native.island.as_str()));
        assert_eq!(frozen.level, native.level);
        assert_eq!(frozen.phase, native.phase);
    }
    let package = fs::read(root.path().join("package.json")).unwrap();
    assert_eq!(
        source_hash(&value, "package.json"),
        Some(format!("{:x}", Sha256::digest(package)))
    );
    assert_eq!(
        source_hash(&value, "package-lock.json"),
        None,
        "missing definition inputs must be recorded explicitly"
    );
    assert!(!value
        .sources
        .iter()
        .any(|source| source.path == "README.md"));
}

#[test]
fn native_policy_recipe_change_invalidates_seal_even_when_command_signature_is_unchanged() {
    let (root, workspace, harness) = fixture();
    let before = preview(&harness, &workspace);
    fs::write(root.path().join("package.json"),
        r#"{"name":"policy-fixture","scripts":{"test":"node -e 'process.exit(0)'","lint":"node --check src/auth.js"}}"#).unwrap();
    let after = preview(&harness, &workspace);
    assert_eq!(
        captured_check(&before, "node-test").binding,
        captured_check(&after, "node-test").binding,
        "argv remains npm run test, so its signature alone cannot identify the recipe"
    );
    assert_ne!(
        source_hash(&before, "package.json"),
        source_hash(&after, "package.json")
    );
    assert_ne!(
        before.source_seal_digest().unwrap(),
        after.source_seal_digest().unwrap()
    );
    assert_ne!(before.digest().unwrap(), after.digest().unwrap());
    before.validate().unwrap();
}

#[test]
fn ordinary_readme_edit_preserves_definition_seal_without_reusing_current_revision() {
    let (root, workspace, harness) = fixture();
    let before = preview(&harness, &workspace);
    fs::write(
        root.path().join("README.md"),
        "# Native Policy fixture\nUpdated usage.\n",
    )
    .unwrap();
    let after = preview(&harness, &workspace);
    assert_eq!(
        before.source_seal_digest().unwrap(),
        after.source_seal_digest().unwrap()
    );
    assert_eq!(before.checks, after.checks);
    assert_eq!(before.policy, after.policy);
    assert_eq!(before.mappings, after.mappings);
    assert_ne!(before.revision.code, after.revision.code);
    assert_ne!(
        before.digest().unwrap(),
        after.digest().unwrap(),
        "the snapshot still binds its exact historical revision"
    );
}

#[test]
fn candidate_source_edit_does_not_corrupt_the_captured_snapshot_audit_anchor() {
    let (root, workspace, harness) = fixture();
    let historical = preview(&harness, &workspace);
    let digest = historical.digest().unwrap();
    fs::write(
        root.path().join("src/auth.js"),
        "export function authorize(request) { return request.authorized === true; }\n",
    )
    .unwrap();
    let current = preview(&harness, &workspace);
    assert_ne!(historical.revision.code, current.revision.code);
    assert_eq!(
        historical.source_seal_digest().unwrap(),
        current.source_seal_digest().unwrap()
    );
    historical.validate().unwrap();
    assert_eq!(
        historical.digest().unwrap(),
        digest,
        "model validation does not compare a historical snapshot with future source"
    );
}

#[test]
fn native_policy_binary_lock_capture_preserves_actual_bytes() {
    let (root, workspace, harness) = fixture();
    let bytes = [0_u8, 255, 128, 10];
    fs::write(root.path().join("bun.lockb"), bytes).unwrap();
    let value = preview(&harness, &workspace);
    assert_eq!(
        source_hash(&value, "bun.lockb"),
        Some(format!("{:x}", Sha256::digest(bytes)))
    );
    assert_eq!(captured_check(&value, "node-test").program, "bun");
}

#[test]
fn native_policy_unknown_required_check_in_any_branch_is_not_activatable() {
    for branch in ["default", "docs", "unmatched-rule"] {
        let (root, workspace, harness) = fixture();
        preview(&harness, &workspace);
        let mut draft = policy();
        let requirements = match branch {
            "default" => &mut draft.requirements,
            "docs" => &mut draft.docs_only.as_mut().unwrap().require,
            "unmatched-rule" => &mut draft.rules[0].require,
            _ => unreachable!(),
        };
        requirements.checks.push("unknown-required-check".into());
        write_policy(root.path(), draft);
        assert!(
            crate::design::load_design(&workspace)
                .unwrap()
                .error_count()
                == 0,
            "the draft parser preserves an unknown required ID for native resolution"
        );
        assert!(
            harness.acceptance_policy_preview(ID, &workspace).is_err(),
            "{branch}"
        );
    }
}

#[test]
fn native_policy_mapping_capture_rejects_missing_or_directory_file_refs() {
    for path in ["src", "src/missing.js"] {
        let (root, workspace, harness) = fixture();
        preview(&harness, &workspace);
        fs::write(root.path().join(".wcode/design/components.yaml"), format!(
            "- id: component:auth\n  name: Authorization\n  implementation:\n    - kind: file\n      path: {path}\n"
        )).unwrap();
        assert!(
            crate::design::load_design(&workspace)
                .unwrap()
                .error_count()
                == 0,
            "a lexical CodeRef is not proof of a regular-file mapping"
        );
        assert!(
            harness.acceptance_policy_preview(ID, &workspace).is_err(),
            "{path}"
        );
    }
}

#[cfg(unix)]
#[test]
fn native_policy_mapping_capture_rejects_a_symlink_to_external_source() {
    let (root, workspace, harness) = fixture();
    preview(&harness, &workspace);
    let external = tempfile::tempdir().unwrap();
    fs::write(
        external.path().join("external.js"),
        "export const outside = true;\n",
    )
    .unwrap();
    fs::remove_file(root.path().join("src/auth.js")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("external.js"),
        root.path().join("src/auth.js"),
    )
    .unwrap();
    assert!(harness.acceptance_policy_preview(ID, &workspace).is_err());
}

#[test]
fn native_policy_requires_complete_mapping_for_referenced_requirement() {
    let (root, workspace, harness) = fixture();
    preview(&harness, &workspace);
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        "- id: REQ-AUTH\n  title: Authorization\n  intent: Enforce the access boundary.\n",
    )
    .unwrap();
    assert!(
        crate::design::load_design(&workspace)
            .unwrap()
            .error_count()
            == 0
    );
    assert!(
        harness.acceptance_policy_preview(ID, &workspace).is_err(),
        "a known Requirement with no implementation scope must remain unknown"
    );
}

#[test]
fn native_policy_preview_rejects_partial_design_inventory() {
    let (root, workspace, harness) = fixture();
    preview(&harness, &workspace);
    let directory = root.path().join(".wcode/design/requirements");
    fs::create_dir(&directory).unwrap();
    for index in 0..=crate::design::MAX_DESIGN_FILES {
        fs::write(
            directory.join(format!("item-{index}.yaml")),
            format!(
                "id: REQ-EXTRA-{index}\ntitle: Extra {index}\nintent: Bounded Design capture.\n"
            ),
        )
        .unwrap();
    }
    let load = crate::design::load_design(&workspace).unwrap();
    assert!(load
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "design-file-limit"));
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    assert!(
        profile.discovery.complete,
        "this fixture isolates Design incompleteness"
    );
    assert!(harness.acceptance_policy_preview(ID, &workspace).is_err());
}

#[test]
fn native_policy_preview_rejects_partial_discovery_even_with_available_checks() {
    let (root, workspace, harness) = fixture();
    preview(&harness, &workspace);
    fs::write(root.path().join("codegen.yaml"), "schema: [\n").unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    assert!(!profile.discovery.complete);
    assert!(profile
        .discovery
        .reasons
        .iter()
        .any(|reason| reason == "profile_source_invalid"));
    assert!(profile
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-test"));
    assert!(
        crate::design::load_design(&workspace)
            .unwrap()
            .error_count()
            == 0
    );
    assert!(harness.acceptance_policy_preview(ID, &workspace).is_err());
}

#[test]
fn native_policy_preview_rejects_invalid_design_instead_of_using_cached_valid_state() {
    let (root, workspace, harness) = fixture();
    preview(&harness, &workspace);
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        "- id: [\n",
    )
    .unwrap();
    assert_ne!(
        crate::design::load_design(&workspace)
            .unwrap()
            .error_count(),
        0
    );
    assert!(harness.acceptance_policy_preview(ID, &workspace).is_err());
}

#[test]
fn native_policy_missing_lock_becoming_present_changes_seal_without_changing_argv() {
    let (root, workspace, harness) = fixture();
    let before = preview(&harness, &workspace);
    assert_eq!(source_hash(&before, "package-lock.json"), None);
    fs::write(root.path().join("package-lock.json"), "{}\n").unwrap();
    let after = preview(&harness, &workspace);
    assert_eq!(
        captured_check(&before, "node-test").binding,
        captured_check(&after, "node-test").binding
    );
    assert_eq!(
        source_hash(&after, "package-lock.json"),
        Some(format!("{:x}", Sha256::digest(b"{}\n")))
    );
    assert_ne!(
        before.source_seal_digest().unwrap(),
        after.source_seal_digest().unwrap()
    );
}

#[test]
fn native_policy_status_preserves_active_history_and_reports_stale_definition_then_revocation() {
    let (root, workspace, harness) = fixture();
    let inactive = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(inactive["status"], "inactive");
    assert_eq!(inactive["generation"], 0);
    assert_eq!(inactive["acceptance_record"], false);
    let value = preview(&harness, &workspace);
    let original_digest = value.digest().unwrap();
    // Synthetic native receipt fixture: this test is below the transport
    // authorization boundary and does not prove that a human approved anything.
    let receipt = crate::verification::policy_store::OperatorReceipt::new(
        "native-fixture-activate",
        policy_now_ms().unwrap(),
    )
    .unwrap();
    let record = harness
        .acceptance_policy_activate_authorized(ID, &workspace, 0, value, receipt, None)
        .unwrap();
    assert_eq!(record.generation(), 1);
    let active = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(active["status"], "active");
    assert_eq!(active["snapshot_digest"], original_digest);
    assert_eq!(active["acceptance_record"], false);

    fs::write(
        root.path().join("src/auth.js"),
        "export const changed = true;\n",
    )
    .unwrap();
    assert_eq!(
        harness.acceptance_policy_status(ID, &workspace).unwrap()["status"],
        "active",
        "ordinary source edits do not replace a historical approved definition"
    );
    fs::write(root.path().join("package.json"),
        r#"{"name":"policy-fixture","scripts":{"test":"node -e 'process.exit(0)'","lint":"node --check src/auth.js"}}"#).unwrap();
    let stale = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(stale["status"], "stale_definition");
    assert_eq!(stale["snapshot_digest"], original_digest);
    assert_eq!(stale["acceptance_record"], false);

    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let receipt = crate::verification::policy_store::OperatorReceipt::new(
        "native-fixture-revoke",
        policy_now_ms().unwrap(),
    )
    .unwrap();
    let revoked = harness
        .acceptance_policy_revoke_authorized(ID, &workspace, 1, &revision, receipt)
        .unwrap();
    assert_eq!(revoked.generation(), 2);
    let status = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(status["status"], "revoked");
    assert_eq!(status["generation"], 2);
    assert_eq!(status["acceptance_record"], false);
}

#[test]
fn native_policy_activation_rejects_inputs_changed_after_internal_receipt() {
    let (root, workspace, harness) = fixture();
    let captured = preview(&harness, &workspace);
    let receipt = crate::verification::policy_store::OperatorReceipt::new(
        "native-fixture-stale",
        policy_now_ms().unwrap(),
    )
    .unwrap();
    fs::write(root.path().join("README.md"), "# Changed after capture\n").unwrap();
    assert!(
        harness
            .acceptance_policy_activate_authorized(ID, &workspace, 0, captured, receipt, None,)
            .is_err(),
        "even a source-seal-preserving edit changes the approval-bound snapshot"
    );
    let status = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(status["status"], "inactive");
    assert_eq!(status["generation"], 0);
}

#[test]
fn native_policy_revocation_requires_the_operator_bound_current_revision() {
    let (root, workspace, harness) = fixture();
    let value = preview(&harness, &workspace);
    let receipt = crate::verification::policy_store::OperatorReceipt::new(
        "native-fixture-before-revoke",
        policy_now_ms().unwrap(),
    )
    .unwrap();
    harness
        .acceptance_policy_activate_authorized(ID, &workspace, 0, value, receipt, None)
        .unwrap();
    let approved_revision = harness.intelligence.current_revision(&workspace).unwrap();
    fs::write(
        root.path().join("README.md"),
        "# Changed after revocation decision\n",
    )
    .unwrap();
    let receipt = crate::verification::policy_store::OperatorReceipt::new(
        "native-fixture-stale-revoke",
        policy_now_ms().unwrap(),
    )
    .unwrap();
    assert!(harness
        .acceptance_policy_revoke_authorized(ID, &workspace, 1, &approved_revision, receipt,)
        .is_err());
    let status = harness.acceptance_policy_status(ID, &workspace).unwrap();
    assert_eq!(status["status"], "active");
    assert_eq!(status["generation"], 1);
}
#[test]
fn native_policy_git_scope_label_keeps_its_exact_binding_without_a_workspace_directory() {
    let (root, workspace, harness) = fixture();
    let output = std::process::Command::new("git")
        .args(["-c", "init.templateDir=", "init", "--quiet"])
        .current_dir(root.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("workspace").exists());
    let mut draft = policy();
    draft.requirements = requirements("git-diff-check", PolicyLevel::Full);
    draft.docs_only = None;
    draft.rules.clear();
    write_policy(root.path(), draft);
    let value = preview(&harness, &workspace);
    assert_eq!(value.checks.len(), 1);
    let frozen = captured_check(&value, "git-diff-check");
    assert_eq!(frozen.cwd.as_deref(), Some("."));
    assert_eq!(
        frozen.island.as_deref(),
        Some("workspace"),
        "the native scope label must not be reinterpreted or rewritten as a directory"
    );
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let native = profile
        .recommended_checks
        .iter()
        .find(|check| check.id == "git-diff-check")
        .unwrap();
    assert_eq!(frozen.binding, verification_check_binding(native));
    assert!(source_hash(&value, "package.json").is_some());
}
