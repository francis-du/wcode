use super::*;
use std::fs;

const NAMES: [&str; 4] = [
    "feature_entry",
    "batch_worker",
    "parse_request",
    "finish_job",
];

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    for (index, name) in NAMES.iter().enumerate() {
        let mut source = format!("pub fn {name}() -> usize {{ helper_{index}() }}\n");
        source.push_str(&format!("fn helper_{index}() -> usize {{ {index} }}\n"));
        for caller in 0..4 {
            source.push_str(&format!(
                "fn caller_{index}_{caller}() -> usize {{ {name}() }}\n"
            ));
        }
        fs::write(root.path().join(format!("src/unit_{index}.rs")), source).unwrap();
    }
    for index in 0..32 {
        fs::write(
            root.path().join(format!("src/noise_{index}.rs")),
            format!("fn noise_{index}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

#[test]
fn fast_context_routes_launch_requests_through_bounded_profiles() {
    let (root, _) = fixture();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in ["run app", "start server", "启动项目"] {
        let pack = harness
            .agent_context("demo", &workspace, query, 0, &[])
            .unwrap();
        assert_eq!(pack["intent"], "launch");
        assert_eq!(
            pack["readiness"]["next_actions"],
            serde_json::json!(["workspace_info", "run_command"])
        );
        let tools = pack["capabilities"]["recommended_tools"]
            .as_array()
            .unwrap();
        assert_eq!(tools[0], "workspace_info");
        assert_eq!(tools[1], "run_command");
        assert!(tools.len() <= 14);
        assert!(pack["targets"].as_array().unwrap().is_empty());
        assert!(pack["files"].as_array().unwrap().is_empty());
        assert!(pack["hot_source"].as_array().unwrap().is_empty());
        let workflow = serde_json::to_string(&pack["workflow"]).unwrap();
        assert!(workflow.contains("launch_profiles"));
        assert!(workflow.contains("program_available=false"));
        assert!(workflow.contains("task_mode=true"));
        assert!(workflow.contains("status_probe"));
        assert!(workflow.contains("unknown rather than healthy"));
        assert!(!workflow.contains("Stage only reviewed files"));
    }
}

fn assert_explicit_targets_at_budget(budget: usize) {
    let (_root, workspace) = fixture();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, &NAMES.join(" "), budget, &[])
        .unwrap();
    let sources = pack["hot_source"].as_array().unwrap();
    for name in NAMES {
        let source = sources
            .iter()
            .find(|source| source["qualified_name"] == name)
            .unwrap_or_else(|| panic!("missing explicit body {name} at budget {budget}: {pack}"));
        assert!(source["body"]["content"]
            .as_str()
            .unwrap()
            .contains(&format!("fn {name}")));
        assert_eq!(source["body"]["truncated"], false);
        let file = pack["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == source["path"])
            .expect("body must keep its file precondition");
        assert_eq!(source["sha256"], file["sha256"]);
    }
    assert!(
        serde_json::to_vec(&pack).unwrap().len().div_ceil(4)
            <= pack["budget"].as_u64().unwrap() as usize
    );
}

#[test]
fn fast_context_covers_every_explicit_target_without_followup_reads() {
    assert_explicit_targets_at_budget(0);
}

#[test]
fn fast_context_explicit_targets_survive_2k_budget() {
    assert_explicit_targets_at_budget(2_000);
}

#[test]
fn fast_context_explicit_targets_survive_3k_budget() {
    assert_explicit_targets_at_budget(3_000);
}

#[test]
fn fast_context_explicit_targets_survive_6k_budget() {
    assert_explicit_targets_at_budget(6_000);
}

#[test]
fn fast_context_exposes_advisory_decision_plane_when_budget_allows() {
    let (_root, workspace) = fixture();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "feature_entry callers and references",
            12_000,
            &[],
        )
        .unwrap();

    let decisions = &pack["decision_plane"];
    assert_eq!(decisions["schema_version"], "0.1.0");
    assert_eq!(decisions["policy"]["authority"], "advisory_only");
    assert_eq!(decisions["policy"]["can_reduce_safety"], false);
    assert_eq!(
        decisions["policy"]["deterministic_verification_floor"],
        true
    );
    assert!(decisions["signals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|signal| { signal["id"] == "next_action" && signal["mode"] == "assist" }));
    assert!(decisions["signals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|signal| {
            signal["id"] == "semantic_navigation_required"
                && signal["recommendation"] == "semantic_navigation_required_before_edit"
        }));
}

#[test]
fn fast_context_does_not_replace_explicit_body_with_related_helper() {
    let (_root, workspace) = fixture();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "feature_entry batch_worker callers",
            6_000,
            &[],
        )
        .unwrap();
    let sources = pack["hot_source"].as_array().unwrap();
    for name in &NAMES[..2] {
        assert!(
            sources
                .iter()
                .any(|source| source["qualified_name"] == *name),
            "lost direct {name}: {pack}"
        );
    }
}

#[test]
fn fast_context_compaction_keeps_original_prefix_and_actual_line_bounds() {
    let content = "fn feature_entry() {\n    let message = \"你好🚀\";\n    finish_job();\n}\n";
    let source = json!({
        "symbol": {"id": "s:entry", "path": "src/entry.rs", "qualified_name": "feature_entry"},
        "sha256": "a".repeat(64),
        "body": {"start_line": 10, "end_line": 13, "content": content, "redacted": false, "truncated": false}
    });
    let compact = compact_hot_source(&source, 45);
    let excerpt = compact["body"]["content"].as_str().unwrap();
    assert!(
        content.starts_with(excerpt),
        "source excerpt must not insert an ellipsis into code: {excerpt}"
    );
    assert_eq!(
        compact["body"]["end_line"].as_u64().unwrap(),
        9 + excerpt.lines().count() as u64
    );
    assert_eq!(compact["body"]["truncated"], true);
}

#[test]
fn fast_context_unicode_prefix_matrix_preserves_bounds_and_truncation() {
    for content in [
        "",
        "single_line_without_newline",
        "fn entry() {\r\n    let x = \"你好🚀λ\";\r\n}\r\n",
        "first\n\nlast\n",
    ] {
        for limit in 0..80 {
            for already_truncated in [false, true] {
                let source = json!({
                    "body": {"start_line": 7, "end_line": 6 + content.lines().count(),
                        "content": content, "truncated": already_truncated, "redacted": false}
                });
                let compact = compact_hot_source(&source, limit);
                let excerpt = compact["body"]["content"].as_str().unwrap();
                assert!(content.starts_with(excerpt));
                assert!(excerpt.chars().count() <= limit);
                assert_eq!(compact["body"]["end_line"], 6 + excerpt.lines().count());
                assert_eq!(
                    compact["body"]["truncated"],
                    already_truncated || excerpt.len() < content.len()
                );
            }
        }
    }
}

#[test]
fn fast_context_tiny_budgets_keep_original_source_prefixes() {
    let (root, workspace) = fixture();
    for (index, name) in NAMES.iter().enumerate() {
        let source = format!(
            "pub fn {name}() {{\n{}\n}}\n",
            "    let value = \"你好🚀\";\n".repeat(90)
        );
        fs::write(root.path().join(format!("src/unit_{index}.rs")), source).unwrap();
    }
    for cap in [1, 4] {
        let harness = ToolHarness::new(cap).unwrap();
        for budget in [1_000, 1_200, 1_700, 2_000, 3_000, 6_000] {
            let pack = harness
                .agent_context("demo", &workspace, &NAMES.join(" "), budget, &[])
                .unwrap();
            assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
            assert!(!pack["hot_source"].as_array().unwrap().is_empty());
            for source in pack["hot_source"].as_array().unwrap() {
                let index = NAMES
                    .iter()
                    .position(|name| source["qualified_name"] == *name)
                    .unwrap();
                let body = &source["body"];
                let start = body["start_line"].as_u64().unwrap();
                let original = workspace
                    .read_file(&format!("src/unit_{index}.rs"), start as usize, None)
                    .unwrap();
                let excerpt = body["content"].as_str().unwrap();
                assert!(
                    original.content.starts_with(excerpt),
                    "budget {budget} must keep source bytes"
                );
                assert_eq!(source["sha256"], original.sha256);
                assert_eq!(body["truncated"], true);
                if let Some(end) = body["end_line"].as_u64() {
                    assert_eq!(end, start + excerpt.lines().count() as u64 - 1);
                }
            }
        }
    }
}

// Measures actual local Harness invocations, not paid model accuracy, network
// latency, or a Kimi/Claude/Codex end-to-end comparison. Timings are reported,
// never used as brittle CI thresholds; coverage and budgets are deterministic.
#[test]
#[ignore = "manual local before/after benchmark; run with --ignored --nocapture"]
fn fast_context_benchmark() {
    let (_root, workspace) = fixture();
    for phase in ["cold", "warm"] {
        let warm = ToolHarness::new(4).unwrap();
        if phase == "warm" {
            warm.agent_context("demo", &workspace, &NAMES.join(" "), 3_000, &[])
                .unwrap();
        }
        let mut elapsed = Vec::new();
        let mut call_counts = Vec::new();
        let mut payloads = Vec::new();
        let mut initial_coverage = Vec::new();
        for _ in 0..12 {
            let cold = ToolHarness::new(4).unwrap();
            let harness = if phase == "cold" { &cold } else { &warm };
            let started = Instant::now();
            let pack = harness
                .agent_context("demo", &workspace, &NAMES.join(" "), 3_000, &[])
                .unwrap();
            let mut calls = 1;
            let mut bytes = serde_json::to_vec(&pack).unwrap().len();
            let mut covered = 0;
            for name in NAMES {
                if pack["hot_source"].as_array().unwrap().iter().any(|source| {
                    source["qualified_name"] == name && source["body"]["truncated"] == false
                }) {
                    covered += 1;
                    continue;
                }
                let target = pack["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|target| target["qualified_name"] == name)
                    .unwrap();
                let source = harness
                    .symbol_context("demo", &workspace, target["id"].as_str().unwrap(), 80)
                    .unwrap();
                assert!(source["body"]["content"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("fn {name}")));
                bytes += serde_json::to_vec(&source).unwrap().len();
                calls += 1;
            }
            elapsed.push(started.elapsed().as_micros());
            call_counts.push(calls);
            payloads.push(bytes);
            initial_coverage.push(covered);
        }
        elapsed.sort_unstable();
        println!(
            "FAST_CONTEXT_BENCH {}",
            json!({"phase": phase, "samples": elapsed.len(), "p50_us": elapsed[elapsed.len()/2], "p95_us": elapsed[(elapsed.len()*95).div_ceil(100)-1], "local_invocations": call_counts, "response_bytes": payloads, "initial_bodies": initial_coverage, "required_bodies": NAMES.len(), "fixture_files": 36})
        );
    }
}
