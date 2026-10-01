#[test]
fn observatory_change_view_is_readonly_scoped_and_snapshot_bound() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/change_view.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for read-only change inspection regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 32
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_fitness_preserves_unknown_revision_scope_and_visible_denominators() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/fitness.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for Fitness Observatory regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 15
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn release_062_webui_keeps_semantic_and_authorization_responses_scoped() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/release.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for release WebUI regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 10
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_refresh_distinguishes_transport_response_and_render_failures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/refresh.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for observatory refresh regressions");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_adversarial_failures_are_part_of_the_full_suite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/adversarial.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for adversarial Observatory regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|report| report["results"].as_array())
                .is_some_and(|results| results.len() == 6
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_layout_regressions_cover_long_content_and_nested_grids() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/layout.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for Observatory layout regressions");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_audit_covers_refresh_access_and_truthful_telemetry() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/audit.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for observatory audit tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_behavior_keeps_refresh_state_and_operator_summary_truthful() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let report_path = root.join("target/wcode-observatory-behavior.json");
    let _ = std::fs::remove_file(&report_path);
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/observatory.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for the observatory behavior contract");
    let report = std::fs::read(&report_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    assert!(
        output.status.success()
            && report
                .as_ref()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 75
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_code_graph_opens_loads_and_renders_real_graph_data() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/code_graph.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for the Code Graph behavior contract");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 43
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

use crate::native_acceptance_fixture;

#[tokio::test]
async fn acceptance_real_native_serializer_is_consumed_by_the_webui() {
    let fixture = native_acceptance_fixture::NativeFixture::new();
    assert_eq!(
        fixture.root.path().canonicalize().unwrap(),
        fixture.workspace.root()
    );
    let incomplete = fixture.capture().await;
    assert_eq!(
        incomplete.record().state,
        crate::verification::acceptance::AcceptanceState::Incomplete
    );
    fixture.activate();
    let ready = fixture.verify_and_review().await;
    let snapshots = [incomplete, ready].into_iter().map(|native| {
        let record = native.record();
        serde_json::json!({
            "workspace": record.workspace, "repository_revision": record.revision,
            "proof": {"revision_code": record.revision.code, "revision_design": record.revision.design},
            "acceptance": record,
        })
    }).collect::<Vec<_>>();
    let input = fixture.root.path().join("native-acceptance-protocol.json");
    std::fs::write(&input, serde_json::to_vec(&snapshots).unwrap()).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/acceptance.cjs"))
        .arg(root)
        .arg(&input)
        .output()
        .expect("Node is required for native Acceptance serializer interoperability");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["snapshots"], 2);
    assert_eq!(report["passed"], true);
}

#[test]
fn acceptance_ui_keeps_canonical_state_required_axes_and_stale_scope_explicit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/acceptance.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for Acceptance UI protocol regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 33
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verification_ui_runs_observes_cancels_and_refreshes_canonical_acceptance() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/verification.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for native verification UI protocol regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 16
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn jobs_ui_observation_and_verification_recovery_preserve_native_authority() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/jobs.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for native Jobs UI protocol regressions");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 18
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn observatory_web_i18n_honors_locale_and_covers_static_copy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/web_i18n.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for the WebUI i18n contract");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 8
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
