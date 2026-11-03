use crate::engineering_journal::{persist, EngineeringMilestone};
use crate::telemetry::{self, TelemetryKind};
use crate::workspace::Workspace;

#[test]
fn telemetry_export_maps_journal_without_paths_or_secrets() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 7, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let exported = telemetry::export(&workspace, 8).unwrap();
    assert_eq!(exported.len(), 1);
    let mapped = &exported[0];
    assert_eq!(mapped.version, telemetry::TELEMETRY_SCHEMA_VERSION);
    assert_eq!(mapped.kind, TelemetryKind::Verification);
    assert_eq!(mapped.tool, "verify_project");
    assert_eq!(mapped.outcome, "succeeded");
    assert_eq!(mapped.event_id, event.event_id.unwrap());
    let encoded = telemetry::encode(&exported).unwrap();
    assert!(encoded.contains("\"workspace_key\""));
    assert!(!encoded.contains(root.path().to_str().unwrap()));
}

#[test]
fn telemetry_export_is_bounded_and_json_lines() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..3 {
        let event =
            EngineeringMilestone::new("apply_edits", "change", "succeeded", index + 1, Vec::new())
                .unwrap();
        persist(&workspace, &event).unwrap();
    }
    let exported = telemetry::export(&workspace, 2).unwrap();
    assert_eq!(exported.len(), 2);
    assert!(exported
        .iter()
        .all(|event| event.kind == TelemetryKind::Command));
    let encoded = telemetry::encode(&exported).unwrap();
    assert_eq!(encoded.lines().count(), 2);
}

#[test]
fn telemetry_export_writes_only_an_explicit_file() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("apply_edits", "change", "succeeded", 1, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let target = root.path().join("telemetry.jsonl");
    telemetry::write_to(&target, &telemetry::export(&workspace, 8).unwrap()).unwrap();
    let bytes = std::fs::read_to_string(&target).unwrap();
    assert_eq!(bytes.lines().count(), 1);
    assert!(!root.path().join("telemetry.jsonl.bak").exists());
}
