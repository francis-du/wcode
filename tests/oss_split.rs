use std::fs;
use wcode::verification::change::ExecutionGitBinding as LegacyPathBinding;
use wcode_core_types::ExecutionGitBinding;

#[test]
fn extracted_git_binding_preserves_the_public_wire_shape_and_validation() {
    let direct = ExecutionGitBinding {
        repository: format!("sha256:{}", "a".repeat(64)),
        head_sha: "b".repeat(40),
        tree_sha: "c".repeat(40),
        dirty: true,
        index_fingerprint: format!("sha256:{}", "d".repeat(64)),
    };
    let encoded = serde_json::to_value(&direct).unwrap();
    let legacy: LegacyPathBinding = serde_json::from_value(encoded.clone()).unwrap();
    assert!(direct.valid());
    assert!(legacy.valid());
    assert_eq!(encoded, serde_json::to_value(&legacy).unwrap());

    let round_trip: ExecutionGitBinding =
        serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
    assert_eq!(round_trip, direct);
}

#[test]
fn evidence_no_longer_depends_on_the_verification_module_for_git_binding() {
    let evidence = fs::read_to_string("src/evidence/mod.rs").unwrap();
    assert!(evidence.contains("use crate::core_types::ExecutionGitBinding;"));
    assert!(!evidence.contains("crate::verification::change::ExecutionGitBinding"));

    let verification = fs::read_to_string("src/verification/change.rs").unwrap();
    assert!(verification.contains("pub use crate::core_types::ExecutionGitBinding;"));
    assert!(!verification.contains("pub struct ExecutionGitBinding"));

    let root = fs::read_to_string("src/lib.rs").unwrap();
    assert!(root.contains("#[path = \"../crates/core-types/src/lib.rs\"]"));
    assert!(root.contains("pub mod core_types;"));
}

#[test]
fn authority_state_root_contract_no_longer_depends_on_store_modules() {
    let types = fs::read_to_string("crates/core-types/src/lib.rs").unwrap();
    assert!(types.contains("pub fn authority_state_root()"));
    assert!(types.contains("pub fn intelligence_state_root()"));
    for forbidden in ["evidence_store", "auth::", "workspace::", "commercial"] {
        assert!(!types.contains(forbidden), "{forbidden}");
    }

    let safety = fs::read_to_string("src/workspace/fs_safety.rs").unwrap();
    assert!(safety.contains("crate::core_types::authority_state_root()?"));
    assert!(safety.contains("crate::core_types::intelligence_state_root()?"));
    assert!(!safety.contains("crate::evidence_store::state_root"));
    assert!(!safety.contains("crate::auth::state_root"));

    let evidence_store = fs::read_to_string("src/evidence/store.rs").unwrap();
    assert!(evidence_store.contains("crate::core_types::intelligence_state_root()?"));
    let auth_store = fs::read_to_string("src/integrations/auth/store.rs").unwrap();
    assert!(auth_store.contains("crate::core_types::authority_state_root()?"));
}

#[test]
fn intelligence_no_longer_depends_on_harness_report_or_capability_owners() {
    fn rust_files(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    rust_files(std::path::Path::new("src/intelligence"), &mut files);
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("crate::harness::"),
            "{} still imports Harness",
            path.display()
        );
    }

    let reports = fs::read_to_string("crates/core-types/src/reports.rs").unwrap();
    for contract in [
        "pub struct ChangeReviewReport",
        "pub struct ChangedFileReview",
        "pub struct VerificationReport",
        "pub struct VerificationCheck",
    ] {
        assert!(reports.contains(contract), "{contract}");
    }
    let report_facade = fs::read_to_string("src/runtime/contracts/report_types.rs").unwrap();
    assert!(report_facade.contains("pub use crate::core_types::{"));
    let harness = fs::read_to_string("src/runtime/harness/mod.rs").unwrap();
    assert!(harness.contains("pub use crate::report_types::{"));
    let jev = fs::read_to_string("src/intelligence/jev.rs").unwrap();
    assert!(jev.contains("crate::model_tools::model_tools_for_group"));
    assert!(jev.contains("crate::model_tools::promote_model_tools"));
    let root = fs::read_to_string("src/lib.rs").unwrap();
    assert!(root.contains("#[path = \"runtime/contracts/report_types.rs\"]"));
    assert!(root.contains("#[path = \"runtime/contracts/model_tools.rs\"]"));
}

#[test]
fn runtime_core_uses_neutral_telemetry_ports_instead_of_ui_monitor_types() {
    fn assert_tree_has_no_ui_monitor(root: &std::path::Path) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_tree_has_no_ui_monitor(&path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path).unwrap();
                assert!(
                    !source.contains("TaskMonitor") && !source.contains("crate::monitor::"),
                    "{} still depends on UI monitor ownership",
                    path.display()
                );
            }
        }
    }

    assert_tree_has_no_ui_monitor(std::path::Path::new("src/runtime/harness"));
    assert_tree_has_no_ui_monitor(std::path::Path::new("src/runtime/tunnel"));
    for path in [
        "src/runtime/resource.rs",
        "src/runtime/semantic.rs",
        "src/integrations/auth/mod.rs",
    ] {
        let source = fs::read_to_string(path).unwrap();
        assert!(!source.contains("TaskMonitor"), "{path}");
        assert!(!source.contains("crate::monitor::"), "{path}");
    }

    let contract = fs::read_to_string("src/runtime/contracts/telemetry.rs").unwrap();
    for port in ["AuthTelemetry", "TaskTelemetry", "RuntimeTelemetry"] {
        assert!(contract.contains(&format!("trait {port}")), "{port}");
    }
    let adapter = fs::read_to_string("src/ui/monitor/telemetry.rs").unwrap();
    for implementation in [
        "impl AuthTelemetry for TaskMonitor",
        "impl TaskTelemetry for TaskMonitor",
        "impl RuntimeTelemetry for TaskMonitor",
    ] {
        assert!(adapter.contains(implementation), "{implementation}");
    }

    let jobs = fs::read_to_string("src/runtime/contracts/job_observation.rs").unwrap();
    for contract in [
        "trait MonitorJobAccess",
        "struct MonitorJobSnapshot",
        "struct MonitorJobStream",
    ] {
        assert!(jobs.contains(contract), "{contract}");
    }
    let mcp_tasks = fs::read_to_string("src/integrations/mcp/tasks.rs").unwrap();
    assert!(mcp_tasks.contains("impl crate::monitor_jobs::MonitorJobAccess"));
    assert!(!mcp_tasks.contains("crate::monitor::MonitorJob"));
}

#[test]
fn extracted_core_types_crate_stays_dependency_light_and_oss_only() {
    let manifest = fs::read_to_string("crates/core-types/Cargo.toml").unwrap();
    assert!(manifest.contains("name = \"wcode-core-types\""));
    assert!(manifest.contains("license = \"Apache-2.0\""));
    assert!(manifest.contains("serde ="));
    for forbidden in [
        "wcode =",
        "commercial",
        "team",
        "enterprise",
        "reqwest",
        "tokio",
    ] {
        assert!(
            !manifest.to_ascii_lowercase().contains(forbidden),
            "{forbidden}"
        );
    }

    let root_manifest = fs::read_to_string("Cargo.toml").unwrap();
    assert!(root_manifest.contains("members = [\".\", \"crates/core-types\"]"));
    assert!(root_manifest.contains("\"/crates/core-types/src/**\""));

    let core = fs::read_to_string("crates/core-types/src/lib.rs").unwrap();
    assert!(core.contains("pub struct RequiredVerificationCheck"));
    assert!(core.contains("pub enum VerificationCheckExecution"));
    let evidence = fs::read_to_string("src/evidence/mod.rs").unwrap();
    assert!(evidence.contains(
        "pub use crate::core_types::{RequiredVerificationCheck, VerificationCheckExecution};"
    ));
    assert!(!evidence.contains("pub struct RequiredVerificationCheck"));
    assert!(!evidence.contains("pub enum VerificationCheckExecution"));
}
