use std::process::Command;

#[test]
fn retained_job_controls_bind_scope_selection_and_poll_generation() {
    let output = Command::new("node")
        .args([
            "--test",
            "--test-isolation=none",
            "--test-reporter=tap",
            "--test-timeout=10000",
            "tests/unit/ui/job_controls.cjs",
        ])
        .env_remove("NODE_TEST_CONTEXT")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Node must execute the Jobs control regression suite");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    for summary in [
        "# tests 17",
        "# pass 17",
        "# fail 0",
        "# cancelled 0",
        "# skipped 0",
        "# todo 0",
    ] {
        assert!(
            stdout.lines().any(|line| line.trim() == summary),
            "Missing complete test summary {summary}: {stdout}"
        );
    }
}
