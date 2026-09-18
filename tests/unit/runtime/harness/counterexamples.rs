use super::*;
use crate::harness::{ChangedFileReview, ReviewFinding};
use serde_json::json;
use std::fs;

fn review(paths: &[&str]) -> ChangeReviewReport {
    ChangeReviewReport {
        workspace: "candidate-fixture".into(),
        execution: "fixture".into(),
        clean: false,
        files_changed: paths.len(),
        staged_files: 0,
        unstaged_files: paths.len(),
        untracked_files: 0,
        additions: 1,
        deletions: 0,
        binary_files: 0,
        source_changed: true,
        tests_changed: false,
        docs_only: false,
        risk_level: "moderate".into(),
        recommended_verification: "full".into(),
        recommended_checks: vec![],
        summary: "fixture".into(),
        files: paths
            .iter()
            .map(|path| ChangedFileReview {
                path: (*path).into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                category: "source".into(),
                additions: Some(1),
                deletions: Some(0),
                binary: false,
                risk_reasons: vec![],
            })
            .collect(),
        findings: vec![],
        probes: vec![],
        truncated: false,
    }
}

fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

#[test]
fn syntax_candidates_ignore_comments_strings_and_keep_real_boundary() {
    let code = "// false and count < 7 are not candidates\nconst TEXT: &str = \"true and count < 7\";\npub fn accepts(count: i64) -> bool { count < 8 }\n";
    let (_root, workspace) = fixture(&[("src/lib.rs", code)]);
    let result = build(
        &ToolHarness::new(4).unwrap(),
        &workspace,
        &review(&["src/lib.rs"]),
    );
    assert_eq!(result.files_failed, 0, "{result:?}");
    assert_eq!(result.candidates.len(), 1, "{result:?}");
    let candidate = &result.candidates[0];
    assert_eq!(candidate.original, "count < 8");
    assert_eq!(candidate.replacement, "count <= 8");
    let boundary = candidate.boundary_probe.as_ref().unwrap();
    assert_eq!(boundary.binding, "count");
    assert_eq!(boundary.values, ["7", "8", "9"]);
    assert!(boundary.domain.contains("validation"));
    assert_eq!(result.precision, "syntax");
    assert!(!result.executed);
    assert!(result.oracle_required);
}

#[test]
fn syntax_candidates_support_four_languages_without_claiming_type_resolution() {
    for (path, code) in [
        (
            "src/lib.rs",
            "pub fn accepts(count: i64) -> bool { count < 8 }\n",
        ),
        (
            "src/main.go",
            "package main\nfunc accepts(count int) bool { return count < 8 }\n",
        ),
        (
            "src/main.ts",
            "export function accepts(count: number): boolean { return count < 8; }\n",
        ),
        ("src/main.py", "def accepts(count):\n    return count < 8\n"),
    ] {
        let (_root, workspace) = fixture(&[(path, code)]);
        let result = build(&ToolHarness::new(4).unwrap(), &workspace, &review(&[path]));
        assert!(!result.candidates.is_empty(), "{path}: {result:?}");
        let candidate = result
            .candidates
            .iter()
            .find(|c| c.original == "count < 8")
            .unwrap();
        assert_eq!(candidate.status, "proposed-not-typechecked");
        assert_eq!(
            &code[candidate.start_byte..candidate.end_byte],
            candidate.original
        );
        assert_eq!(
            candidate.sha256,
            workspace.read_file(path, 1, None).unwrap().sha256
        );
    }
}

#[test]
fn syntax_candidates_keep_unicode_crlf_bytes_and_reject_old_sha_edits() {
    let path = "源 码/lib.rs";
    let code = "// 你好🚀\r\npub fn accepts(count: i64) -> bool {\r\n    count < 8\r\n}\r\n";
    let (_root, workspace) = fixture(&[(path, code)]);
    let harness = ToolHarness::new(4).unwrap();
    let first = build(&harness, &workspace, &review(&[path]));
    let candidate = &first.candidates[0];
    assert_eq!(candidate.start_line, 3);
    assert_eq!(candidate.end_line, 3);
    assert_eq!(
        &code[candidate.start_byte..candidate.end_byte],
        candidate.original
    );
    workspace
        .replace_text(
            path,
            &candidate.original,
            &candidate.replacement,
            &candidate.sha256,
        )
        .unwrap();
    assert!(workspace
        .replace_text(
            path,
            &candidate.replacement,
            &candidate.original,
            &candidate.sha256
        )
        .is_err());
    let current = workspace.read_file(path, 1, None).unwrap();
    assert!(current.content.contains("\r\n"));
    let second = build(&harness, &workspace, &review(&[path]));
    assert_ne!(second.candidates[0].id, candidate.id);
    assert_eq!(second.candidates[0].sha256, current.sha256);
}

#[test]
fn syntax_candidates_rank_security_first_and_bound_deduplicated_work() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let names: Vec<_> = (0..10).map(|n| format!("src/file{n}.rs")).collect();
    for path in &names {
        fs::write(root.path().join(path), "pub fn f() -> bool { false }\n").unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut change = review(&names.iter().map(String::as_str).collect::<Vec<_>>());
    change.files.extend(review(&["src/file9.rs"]).files);
    change.findings.push(ReviewFinding {
        severity: "high".into(),
        code: "security-sensitive-change".into(),
        message: "fixture".into(),
        paths: vec!["src/file9.rs".into()],
    });
    let harness = ToolHarness::new(4).unwrap();
    let first = build(&harness, &workspace, &change);
    assert_eq!(first.files_considered, 10);
    assert_eq!(first.files_scanned, 6);
    assert_eq!(first.files_omitted, 4);
    assert_eq!(first.candidates.len(), 3);
    assert_eq!(first.candidates[0].path, "src/file9.rs");
    assert!(first.truncated);
    change.files.reverse();
    let second = build(&harness, &workspace, &change);
    assert_eq!(
        serde_json::to_value(first.candidates).unwrap(),
        serde_json::to_value(second.candidates).unwrap()
    );
}

#[test]
fn syntax_candidates_do_not_mutate_or_execute_readonly_sources() {
    let (root, _) = fixture(&[("src/lib.rs", "pub fn f() -> bool { false }\n")]);
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let before = fs::read(root.path().join("src/lib.rs")).unwrap();
    let result = build(
        &ToolHarness::new(4).unwrap(),
        &workspace,
        &review(&["src/lib.rs"]),
    );
    assert_eq!(result.candidates.len(), 1);
    assert!(!result.executed);
    assert_eq!(fs::read(root.path().join("src/lib.rs")).unwrap(), before);
    let c = &result.candidates[0];
    assert!(workspace
        .replace_text(&c.path, &c.original, &c.replacement, &c.sha256)
        .is_err());
}

#[test]
fn syntax_candidates_keep_workspace_boundaries_and_skip_deleted_binary_files() {
    let (_root, workspace) = fixture(&[("src/lib.rs", "pub fn f() -> bool { false }\n")]);
    let mut change = review(&[
        "../outside.rs",
        ".git/config",
        "src/deleted.rs",
        "src/binary.rs",
    ]);
    change.files[2].status = "deleted".into();
    change.files[3].binary = true;
    let result = build(&ToolHarness::new(4).unwrap(), &workspace, &change);
    assert_eq!(result.files_considered, 2);
    assert_eq!(result.files_failed, 2);
    assert!(result.candidates.is_empty());
    assert!(result.truncated);
    assert!(!result.executed);
}

#[test]
fn syntax_candidates_report_oversized_unsupported_and_invalid_source() {
    let too_large = " ".repeat(MAX_SOURCE_BYTES as usize + 1);
    let (_root, workspace) = fixture(&[
        ("src/large.rs", &too_large),
        ("notes.txt", "false"),
        ("src/broken.rs", "pub fn broken( { false"),
    ]);
    let result = build(
        &ToolHarness::new(4).unwrap(),
        &workspace,
        &review(&["src/large.rs", "notes.txt", "src/broken.rs"]),
    );
    assert_eq!(result.files_skipped, 2, "{result:?}");
    assert_eq!(result.files_failed, 1, "{result:?}");
    assert!(result.candidates.is_empty());
    assert_eq!(result.status, "partial");
}

#[test]
fn syntax_candidates_clean_tree_and_node_limits_are_honest() {
    let code = format!(
        "pub fn f() -> bool {{ {} false }}",
        "let _ = true; ".repeat(50)
    );
    let (_root, workspace) = fixture(&[("src/lib.rs", &code)]);
    let harness = ToolHarness::new(4).unwrap();
    let mut change = review(&["src/lib.rs"]);
    let bounded = build(&harness, &workspace, &change);
    assert!(bounded.truncated);
    assert!(bounded.candidates_considered <= MAX_NODES_PER_FILE);
    assert_eq!(bounded.candidates.len(), MAX_CANDIDATES);
    change.clean = true;
    let clean = build(&harness, &workspace, &change);
    assert_eq!(clean.files_considered, 0);
    assert!(clean.candidates.is_empty());
    assert_eq!(clean.status, "no_candidates");
}

#[test]
fn syntax_candidates_proposals_do_not_guess_unsupported_expressions_or_overflow() {
    for source in [
        "call() < 8",
        "a < b < c",
        "\"count < 8\"",
        "count /* == */ < 8",
        "count < 8usize",
    ] {
        assert!(proposal(source).is_none(), "{source}");
    }
    for (source, replacement, samples) in [
        ("x < 9223372036854775807", "x <= 9223372036854775807", 2),
        ("x > -9223372036854775808", "x >= -9223372036854775808", 2),
        ("0 < count", "0 <= count", 3),
        ("count === 8", "count !== 8", 3),
    ] {
        let (actual, boundary, _) = proposal(source).unwrap();
        assert_eq!(actual, replacement);
        assert_eq!(boundary.unwrap().values.len(), samples);
    }
    assert!(proposal("actual == expected").unwrap().1.is_none());
}

#[test]
fn syntax_candidates_generated_mutation_is_killed_by_isolated_fixture_oracle() {
    let code = "pub fn accepts(count: i64) -> bool { count < 8 }\n";
    let (root, workspace) = fixture(&[("src/lib.rs", code)]);
    let search = build(
        &ToolHarness::new(4).unwrap(),
        &workspace,
        &review(&["src/lib.rs"]),
    );
    let candidate = &search.candidates[0];
    let values = &candidate.boundary_probe.as_ref().unwrap().values;
    assert_eq!(values, &["7", "8", "9"]);
    // The oracle is independently supplied by this frozen test contract, not
    // inferred by the production generator from the implementation expression.
    let assertions: String = values
        .iter()
        .zip([true, false, false])
        .map(|(value, expected)| format!("assert_eq!(accepts({value}), {expected});"))
        .collect();
    let test = format!("#[test] fn independent_contract() {{ {assertions} }}\n");
    let run = |source: &str| {
        fs::write(root.path().join("trial.rs"), format!("{source}{test}")).unwrap();
        let executable = root
            .path()
            .join(if cfg!(windows) { "trial.exe" } else { "trial" });
        let compilation = std::process::Command::new("rustc")
            .args(["--edition=2021", "--test", "trial.rs", "-o"])
            .arg(&executable)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            compilation.status.success(),
            "{}",
            String::from_utf8_lossy(&compilation.stderr)
        );
        std::process::Command::new(&executable)
            .current_dir(root.path())
            .output()
            .unwrap()
    };
    let baseline = run(code);
    assert!(baseline.status.success());
    assert!(String::from_utf8_lossy(&baseline.stdout).contains("1 passed"));
    let changed = code.replacen(&candidate.original, &candidate.replacement, 1);
    let failed = run(&changed);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stdout).contains("1 failed"));
    assert_eq!(
        fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
        code
    );
    assert!(
        !search.executed,
        "fixture execution cannot relabel the original proposal"
    );
}

#[test]
fn syntax_candidates_reject_corrupted_redacted_or_truncated_node_evidence() {
    let sha = "a".repeat(64);
    let item = json!({"path":"src/lib.rs", "sha256":sha, "language":"rust", "node_kind":"boolean_literal",
        "text":"false", "redacted":false, "text_truncated":false, "parse_errors":false,
        "range":{"start_line":1,"start_column":1,"end_line":1,"end_column":6,"end_exclusive":true}});
    assert!(from_match("src/lib.rs", &sha, "false", &item, false).is_some());
    for (pointer, value) in [
        ("/redacted", json!(true)),
        ("/text_truncated", json!(true)),
        ("/parse_errors", json!(true)),
        ("/sha256", json!("old")),
        ("/path", json!("src/other.rs")),
        ("/node_kind", json!("string_literal")),
        ("/text", json!("true")),
        ("/range/end_column", json!(100)),
        ("/range/end_exclusive", json!(false)),
    ] {
        let mut invalid = item.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            from_match("src/lib.rs", &sha, "false", &invalid, false).is_none(),
            "{pointer}"
        );
    }
}

#[cfg(unix)]
#[test]
fn syntax_candidates_reject_symlink_sources_without_reading_external_bytes() {
    let (_root, workspace) = fixture(&[]);
    let external = tempfile::tempdir().unwrap();
    let path = external.path().join("outside.rs");
    fs::write(&path, "pub fn secret() -> bool { false }\n").unwrap();
    std::os::unix::fs::symlink(&path, workspace.root().join("linked.rs")).unwrap();
    let result = build(
        &ToolHarness::new(2).unwrap(),
        &workspace,
        &review(&["linked.rs"]),
    );
    assert_eq!(result.files_failed, 1);
    assert!(result.candidates.is_empty());
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("pub fn secret"));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "pub fn secret() -> bool { false }\n"
    );
}

#[test]
fn syntax_candidates_byte_mapping_rejects_non_utf8_boundaries() {
    assert_eq!(byte_at("你好\r\nfalse\r\n", 2, 1), Some(8));
    assert_eq!(byte_at("你好\r\nfalse\r\n", 1, 2), None);
    assert_eq!(
        byte_at("x\nfalse", 1, 3),
        None,
        "a column cannot cross a line terminator"
    );
    assert_eq!(byte_at("x\nfalse", 2, usize::MAX), None);
    assert_eq!(byte_at("x", 0, 1), None);
    assert_eq!(byte_at("x", 1, 0), None);
    assert_eq!(byte_at("x", usize::MAX, 1), None);
    // Explicitly assert the non-Evidence protocol, not just field existence.
    let empty = build(&ToolHarness::new(1).unwrap(), &fixture(&[]).1, &review(&[]));
    let json = serde_json::to_value(empty).unwrap();
    assert_eq!(json["executed"], json!(false));
    assert!(json.get("verdict").is_none());
    assert!(json.get("evidence_id").is_none());
}
