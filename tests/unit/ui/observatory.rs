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
                .is_some_and(|results| results.len() == 7
                    && results.iter().all(|item| item["passed"] == true)),
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
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/observatory.cjs"))
        .arg(root)
        .output()
        .expect("Node is required for the observatory behavior contract");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report
                .as_ref()
                .ok()
                .and_then(|value| value["results"].as_array())
                .is_some_and(|results| results.len() == 38
                    && results.iter().all(|item| item["passed"] == true)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
