use super::*;
use serde_json::json;
use std::fs;

#[test]
fn fitness_benchmark_discovery_never_recurses_and_counts_non_files_toward_bounds() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    fs::create_dir_all(root.path().join(DIRECTORY).join("nested/deeper")).unwrap();
    for index in 0..64 {
        fs::write(
            root.path()
                .join(DIRECTORY)
                .join(format!("nested/deeper/{index}-1.json")),
            "{}",
        )
        .unwrap();
    }
    let entries = workspace.bounded_directory_entries(DIRECTORY, 33).unwrap();
    assert_eq!(entries, vec!["target/engineering-fitness/nested"]);
    let view = load_benchmarks(&workspace, &revision('a'));
    assert!(view.available);
    assert_eq!(view.invalid_reports, 1);
    assert!(view.latest.is_none());
    assert!(workspace.bounded_directory_entries("../", 33).is_err());
}

fn revision(seed: char) -> Revision {
    Revision {
        code: format!("sha256:{}", seed.to_string().repeat(64)),
        design: Some(format!("sha256:{}", "d".repeat(64))),
    }
}
fn snapshot() -> FitnessBenchmarkSnapshot {
    FitnessBenchmarkSnapshot {
        schema_version: 1,
        contract_version: 3,
        captured_at_ms: 1,
        wcode_version: "0.8.4".into(),
        profile: "debug".into(),
        os: "macos".into(),
        arch: "aarch64".into(),
        case_count: 60,
        samples_per_case: 1,
        model_calls: 0,
        harness_slots: 4,
        corpus_sha256: "c".repeat(64),
        evaluator_sha256: "e".repeat(64),
        test_binary_sha256: "b".repeat(64),
        revision_before: revision('a'),
        revision_after: revision('a'),
        source_snapshot_before: "f".repeat(64),
        source_snapshot_after: "f".repeat(64),
        source_stable_during_run: true,
        controls_passed: 11,
        controls_total: 11,
        rows: [1000, 2000, 4000]
            .into_iter()
            .flat_map(|budget| {
                ["cold", "warm"]
                    .into_iter()
                    .map(move |phase| FitnessBenchmarkRow {
                        budget,
                        phase: phase.into(),
                        attempts: 60,
                        query_errors: 1,
                        warmup_errors: 0,
                        over_budget: 0,
                        required_count: 98,
                        required_hits: 90,
                        complete_body_hits: 80,
                        fresh_sha_hits: 88,
                        edit_input_eligible: 58,
                        edit_input_ready: 50,
                        ranking_attempts: 59,
                        mean_ndcg_at_10: Some(0.85),
                        noise_samples: 58,
                        mean_non_gold_fraction: Some(0.05),
                        complete_gold_bytes: 100,
                        density_response_bytes: 1000,
                        latency_samples: 60,
                        p50_us: Some(10.0),
                        p95_us: Some(30),
                    })
            })
            .collect(),
    }
}
fn store(root: &std::path::Path, name: &str, report: &FitnessBenchmarkSnapshot) {
    fs::create_dir_all(root.join(DIRECTORY)).unwrap();
    fs::write(
        root.join(DIRECTORY).join(name),
        serde_json::to_vec(report).unwrap(),
    )
    .unwrap();
}
#[test]
fn fitness_benchmark_requires_complete_matrix_bounded_counts_and_defined_ratios() {
    let source = snapshot();
    source.validate(2).unwrap();
    let mut bad = source.clone();
    bad.rows[0].required_hits = 99;
    assert!(bad.validate(2).is_err());
    let mut bad = source.clone();
    bad.rows[0].mean_ndcg_at_10 = None;
    assert!(bad.validate(2).is_err());
    let mut bad = source.clone();
    bad.rows[0].noise_samples = 0;
    assert!(bad.validate(2).is_err());
    let mut bad = source.clone();
    bad.rows.pop();
    assert!(bad.validate(2).is_err());
    let mut bad = source.clone();
    bad.rows[1] = bad.rows[0].clone();
    assert!(bad.validate(2).is_err());
    let mut bad = source.clone();
    bad.captured_at_ms = 3;
    assert!(bad.validate(2).is_err());
    let mut bad = source;
    bad.revision_after.code = "untrusted text".into();
    assert!(bad.validate(2).is_err());
}
#[test]
fn fitness_benchmark_missing_is_unmeasured_not_green_or_source_mutation() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let view = load_benchmarks(&workspace, &revision('a'));
    assert!(view.available);
    assert_eq!(view.status, "not_measured");
    assert!(view.latest.is_none());
    assert!(!root.path().join(DIRECTORY).exists());
}
#[test]
fn fitness_benchmark_stale_and_unstable_reports_never_prove_current_revision() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut report = snapshot();
    store(root.path(), "1-1.json", &report);
    assert_eq!(
        load_benchmarks(&workspace, &revision('a')).status,
        "current"
    );
    assert_eq!(
        load_benchmarks(&workspace, &revision('b')).status,
        "historical"
    );
    report.revision_before = revision('b');
    store(root.path(), "1-1.json", &report);
    assert_eq!(
        load_benchmarks(&workspace, &revision('a')).status,
        "unstable"
    );
    report.source_snapshot_after = "a".repeat(64);
    store(root.path(), "1-1.json", &report);
    assert_eq!(
        load_benchmarks(&workspace, &revision('a')).status,
        "invalid_reports"
    );
}
#[test]
fn fitness_benchmark_invalid_newer_record_and_duplicates_cannot_inflate_history() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    store(root.path(), "1-1.json", &snapshot());
    store(root.path(), "2-1.json", &snapshot());
    let view = load_benchmarks(&workspace, &revision('a'));
    assert_eq!(view.duplicate_reports, 1);
    assert_eq!(view.report_count, 1);
    fs::write(root.path().join(DIRECTORY).join("3-1.json"), "{broken").unwrap();
    let view = load_benchmarks(&workspace, &revision('a'));
    assert!(view.partial);
    assert_eq!(view.status, "partial");
    assert_eq!(view.invalid_reports, 1);
    assert!(!view.history[0].current);
}
#[test]
fn fitness_benchmark_only_exposes_typed_summaries_and_bounds_report_population() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    store(root.path(), "1-1.json", &snapshot());
    let mut report = serde_json::to_value(snapshot()).unwrap();
    report["source"] = json!("not telemetry");
    fs::write(
        root.path().join(DIRECTORY).join("2-1.json"),
        report.to_string(),
    )
    .unwrap();
    let view = load_benchmarks(&workspace, &revision('a'));
    assert_eq!(view.invalid_reports, 1);
    assert!(!serde_json::to_string(&view)
        .unwrap()
        .contains("not telemetry"));
    for index in 3..=33 {
        store(root.path(), &format!("{index}-1.json"), &snapshot());
    }
    let view = load_benchmarks(&workspace, &revision('a'));
    assert!(!view.available);
    assert!(view.partial);
    assert!(view.latest.is_none());
}
#[test]
fn fitness_benchmark_signals_track_report_bytes_and_comparability_keeps_environment() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let before = benchmark_signal(&workspace);
    let mut report = snapshot();
    store(root.path(), "1-1.json", &report);
    let after = benchmark_signal(&workspace);
    assert_ne!(before, after);
    report.rows[0].required_hits -= 1;
    store(root.path(), "1-1.json", &report);
    assert_ne!(after, benchmark_signal(&workspace));
    assert!(report.comparable(&snapshot()));
    report.profile = "release".into();
    assert!(!report.comparable(&snapshot()));
}
#[test]
fn fitness_benchmark_workspace_reader_rejects_symbolic_link_summaries() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    fs::create_dir_all(root.path().join(DIRECTORY)).unwrap();
    fs::write(
        outside.path().join("outside.json"),
        serde_json::to_vec(&snapshot()).unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            outside.path().join("outside.json"),
            root.path().join(DIRECTORY).join("1-1.json"),
        )
        .unwrap();
        assert!(read_summary(&workspace, "target/engineering-fitness/1-1.json").is_err());
        assert!(load_benchmarks(&workspace, &revision('a')).latest.is_none());
    }
    #[cfg(not(unix))]
    assert!(read_summary(&workspace, "../outside.json").is_err());
}
