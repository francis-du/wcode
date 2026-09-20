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
                .is_some_and(|results| results.len() == 53
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
                .is_some_and(|results| results.len() == 25
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
