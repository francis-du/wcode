use std::process::Command;

fn run_observatory_contract(script: &str, cases: usize) {
    let output = Command::new("node")
        .args([
            "--test",
            "--test-isolation=none",
            "--test-reporter=tap",
            "--test-timeout=20000",
            script,
        ])
        .env_remove("NODE_TEST_CONTEXT")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Node must run the production observability contract");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    for summary in [
        format!("# tests {cases}"),
        format!("# pass {cases}"),
        "# fail 0".into(),
        "# cancelled 0".into(),
        "# skipped 0".into(),
        "# todo 0".into(),
    ] {
        assert!(
            stdout.lines().any(|line| line.trim() == summary),
            "Missing complete test summary {summary}: {stdout}"
        );
    }
}

#[test]
fn activity_job_navigation_preserves_exact_observation_identity() {
    run_observatory_contract("tests/unit/ui/activity_jobs.cjs", 16);
}

#[test]
fn design_source_navigation_preserves_observed_paths_and_revision() {
    run_observatory_contract("tests/unit/ui/design_sources.cjs", 14);
}

#[test]
fn retained_job_output_search_preserves_scope_focus_and_scroll() {
    let output = Command::new("node")
        .args(["tests/unit/ui/job_output.cjs", "."])
        .env_remove("NODE_TEST_CONTEXT")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Node must run the retained-output production UI contract");
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
    assert!(
        output.status.success()
            && report.as_ref().ok().is_some_and(|report| {
                report["suite"] == "retained-job-output"
                    && report["results"].as_array().is_some_and(|results| {
                        results.len() == 22 && results.iter().all(|row| row["passed"] == true)
                    })
            }),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
