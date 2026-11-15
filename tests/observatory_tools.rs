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
fn file_tree_keeps_its_first_page_bounded_and_expands_on_request() {
    run_observatory_contract("tests/unit/ui/file_tree.cjs", 3);
}

#[test]
fn convergence_cards_navigate_to_the_object_they_name() {
    run_observatory_contract("tests/unit/ui/convergence_navigation.cjs", 3);
}

#[test]
fn attention_signals_navigate_to_the_object_they_name() {
    run_observatory_contract("tests/unit/ui/attention_navigation.cjs", 7);
}

#[test]
fn tunnel_telemetry_stays_bounded_and_never_publishes_fake_health() {
    run_observatory_contract("tests/unit/ui/tunnel_telemetry.cjs", 9);
}

#[test]
fn project_navigator_declares_the_loaded_snapshot_scope() {
    run_observatory_contract("tests/unit/ui/navigator_scope.cjs", 7);
}

#[test]
fn oauth_session_administration_requires_confirmation_and_reports_persistence() {
    run_observatory_contract("tests/unit/ui/session_admin.cjs", 8);
}

#[test]
fn retained_job_output_search_preserves_scope_focus_and_scroll() {
    let output = Command::new("node")
        .args(["tests/unit/ui/job_output_reading.cjs", "."])
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
                        results.len() == 25 && results.iter().all(|row| row["passed"] == true)
                    })
            }),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Every UI suite must be reachable from a runner. A suite that no other file
/// names silently rots, so a newly added file that nothing invokes fails here.
#[test]
fn every_ui_suite_is_referenced_by_a_runner() {
    fn collect(root: &std::path::Path, sources: &mut Vec<(std::path::PathBuf, String)>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                collect(&path, sources);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "cjs" || ext == "yml" || ext == "yaml")
            {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    sources.push((path.clone(), text));
                }
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    collect(&root.join("tests"), &mut sources);
    collect(&root.join(".github"), &mut sources);

    let ui = root.join("tests/unit/ui");
    let mut suites = std::fs::read_dir(&ui)
        .expect("UI suites directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cjs"))
        .collect::<Vec<_>>();
    suites.sort();
    assert!(!suites.is_empty(), "no UI suites were found");
    for suite in suites {
        let name = suite.file_name().unwrap().to_str().unwrap();
        let referenced = sources
            .iter()
            .any(|(path, text)| path != &suite && text.contains(name));
        assert!(
            referenced,
            "{name} is not referenced by any runner; wire it into observatory_tools.rs, \
             release_audit.cjs or a workflow step",
        );
    }
}
