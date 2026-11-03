use std::process::Command;

#[test]
fn retained_job_controls_bind_scope_selection_and_poll_generation() {
    for (script, count) in [
        ("tests/unit/ui/job_controls.cjs", 22),
        ("tests/unit/ui/job_lifecycle.cjs", 8),
        ("tests/unit/ui/job_list_state.cjs", 9),
    ] {
        let output = Command::new("node")
            .args([
                "--test",
                "--test-isolation=none",
                "--test-reporter=tap",
                "--test-timeout=10000",
                script,
            ])
            .env_remove("NODE_TEST_CONTEXT")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Node must execute the Jobs control regression suite");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{script}: {stdout}\n{stderr}");
        for summary in [
            format!("# tests {count}"),
            format!("# pass {count}"),
            "# fail 0".into(),
            "# cancelled 0".into(),
            "# skipped 0".into(),
            "# todo 0".into(),
        ] {
            assert!(
                stdout.lines().any(|line| line.trim() == summary),
                "Missing complete test summary {summary} in {script}: {stdout}"
            );
        }
    }
}
