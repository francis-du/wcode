use super::*;
use serde_json::{json, Value};
use std::fs;

#[path = "io_bench.rs"]
mod io_bench;

fn request(queries: &[&str], mode: SearchMode, limit: usize) -> SearchRequest {
    SearchRequest {
        queries: queries.iter().map(|s| (*s).to_owned()).collect(),
        path: ".".into(),
        mode,
        context_lines: 0,
        max_results: limit,
        offset: 0,
        output_mode: "content".into(),
    }
}

fn report(workspace: &Workspace, request: &SearchRequest, grouped: bool) -> Value {
    workspace
        .search_report(request)
        .unwrap()
        .into_value("test", request, grouped)
}

#[test]
fn search_auto_uses_one_traversal_and_one_read_per_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = "package demo\nfmt.Sprintf(\"SELECT job\")\n";
    for i in 0..17 {
        fs::write(dir.path().join(format!("{i:02}.go")), source).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let query = request(&["Sprintf SELECT"], SearchMode::Auto, 100);
    let found = report(&workspace, &query, false);
    assert_eq!(found["mode"], "tokens_all");
    assert_eq!(found["traversals"], 1);
    assert_eq!(found["files_scanned"], 17);
    assert_eq!(found["bytes_read"], 17 * source.len());
    assert_eq!(found["total_matches"], 17);
    assert_eq!(found["coverage_complete"], true);
    fs::write(dir.path().join("zz.go"), "Sprintf SELECT\n").unwrap();
    let exact = report(&workspace, &query, false);
    assert_eq!(exact["mode"], "exact");
    assert_eq!(exact["total_matches"], 1);
    assert_eq!(exact["matches"][0]["path"], "zz.go");
}

#[test]
fn search_batch_keeps_rare_patterns_and_reports_real_totals() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "common\n".repeat(100)).unwrap();
    fs::write(dir.path().join("z.go"), "rare\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let query = request(&["common", "rare", "common"], SearchMode::Exact, 2);
    let found = report(&workspace, &query, false);
    assert_eq!(found["pattern_count"], 2);
    assert_eq!(found["count"], 2);
    assert_eq!(found["total_matches"], 101);
    assert_eq!(found["query_counts"][0]["matched_lines"], 100);
    assert_eq!(found["query_counts"][1]["matched_lines"], 1);
    assert_eq!(found["matches"][1]["query"], "rare");
    assert_eq!(found["coverage_complete"], true);
    assert_eq!(found["results_truncated"], true);
    assert_eq!(found["next_offset"], 2);
}

#[test]
fn search_pagination_is_deterministic_without_duplicate_or_missing_rows() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "common\n".repeat(19)).unwrap();
    fs::write(dir.path().join("z.go"), "rare\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut query = request(&["common", "rare"], SearchMode::Exact, 3);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..10 {
        let page = report(&workspace, &query, false);
        assert_eq!(page, report(&workspace, &query, false));
        for row in page["matches"].as_array().unwrap() {
            assert!(seen.insert((
                row["path"].as_str().unwrap().to_owned(),
                row["line"].as_u64().unwrap()
            )));
        }
        match page["next_offset"].as_u64() {
            Some(offset) => query.offset = offset as usize,
            None => break,
        }
    }
    assert_eq!(seen.len(), 20);
}

#[test]
fn search_grouped_context_merges_overlapping_windows_and_query_hits() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.go"),
        "before\nalpha beta\nalpha\nafter\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut query = request(&["alpha", "beta"], SearchMode::Exact, 100);
    query.context_lines = 1;
    let found = report(&workspace, &query, true);
    assert_eq!(found["count"], 2);
    assert_eq!(
        found["files"][0]["matches"][0]["queries"],
        json!(["alpha", "beta"])
    );
    assert_eq!(
        found["files"][0]["context_lines"].as_array().unwrap().len(),
        4
    );
    assert!(found["files"][0]["matches"][0].get("context").is_none());
}

#[test]
fn search_sha_allows_precise_edit_and_rejects_external_change() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "package demo\n// old\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let found = report(
        &workspace,
        &request(&["// old"], SearchMode::Exact, 10),
        false,
    );
    let row = &found["matches"][0];
    let sha = row["sha256"].as_str().unwrap();
    workspace
        .replace_text("a.go", "// old", "// new", sha)
        .unwrap();
    assert!(workspace
        .replace_text("a.go", "// new", "// wrong", sha)
        .is_err());
    assert!(fs::read_to_string(dir.path().join("a.go"))
        .unwrap()
        .contains("// new"));
}

#[test]
fn search_file_and_count_modes_do_not_return_source_bodies() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "needle\nneedle\n").unwrap();
    fs::write(dir.path().join("b.go"), "needle\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut query = request(&["needle"], SearchMode::Exact, 1);
    for mode in ["files_with_matches", "count_matches"] {
        query.output_mode = mode.into();
        let found = report(&workspace, &query, false);
        assert!(found.get("matches").is_none());
        assert_eq!(found["total_matches"], 3);
        assert_eq!(found["file_count"], 2);
        assert_eq!(found["files"][0]["count"], 2);
        assert!(found["files"][0].get("text").is_none());
        assert_eq!(found["next_offset"], 1);
    }
}

#[test]
fn search_partial_io_and_oversized_files_never_claim_complete_coverage() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "needle\n").unwrap();
    fs::write(dir.path().join("large.go"), "x".repeat(1024 * 1024 + 1)).unwrap();
    fs::write(dir.path().join("invalid.go"), [0xff, 0xfe]).unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let found = report(
        &workspace,
        &request(&["missing"], SearchMode::Exact, 100),
        false,
    );
    assert_eq!(found["count"], 0);
    assert_eq!(found["skipped_files"], 1);
    assert_eq!(found["failed_files"], 1);
    assert_eq!(found["coverage_complete"], false);
    assert_eq!(found["truncated"], true);
}

#[test]
fn search_regex_and_original_lines_preserve_unicode_crlf_and_eof() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.go"), "首行\r\nneedle中文\r\n最后needle").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let found = report(
        &workspace,
        &request(&[r"\Aneedle中文\z", "needle$"], SearchMode::Regex, 100),
        false,
    );
    assert_eq!(found["count"], 2);
    assert_eq!(found["matches"][0]["line"], 2);
    assert_eq!(found["matches"][1]["line"], 3);
    assert_eq!(
        found["matches"][0]["sha256"],
        workspace.read_file("a.go", 1, None).unwrap().sha256
    );
}

#[test]
fn search_invalid_and_excessive_regex_is_rejected_before_path_access() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut query = request(&["("], SearchMode::Regex, 100);
    query.path = "missing".into();
    assert!(workspace
        .search_report(&query)
        .err()
        .unwrap()
        .to_string()
        .contains("invalid search regex"));
    query.queries = vec!["x".repeat(16 * 1024 + 1)];
    assert!(workspace
        .search_report(&query)
        .err()
        .unwrap()
        .to_string()
        .contains("16384"));
}

#[cfg(unix)]
#[test]
fn search_never_follows_symlinks_or_searches_protected_files() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("outside.go"), "needle outside").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.go"),
        dir.path().join("alias.go"),
    )
    .unwrap();
    fs::write(dir.path().join(".env"), "needle hidden").unwrap();
    fs::write(dir.path().join("safe.go"), "needle safe").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let found = report(
        &workspace,
        &request(&["needle"], SearchMode::Exact, 100),
        false,
    );
    assert_eq!(found["count"], 1);
    assert_eq!(found["matches"][0]["path"], "safe.go");
    let mut query = request(&["needle"], SearchMode::Exact, 100);
    query.path = "alias.go".into();
    assert!(workspace.search_report(&query).is_err());
}

#[test]
fn batch_edit_stitching_preserves_original_anchors_unicode_and_crlf() {
    for seed in 0..128 {
        let mut original = String::new();
        let mut expected = String::new();
        let mut edits = Vec::new();
        for i in 0..32 {
            let old = format!("原始_{seed}_{i}");
            let new = format!("新_{i}_{}", "值".repeat((seed + i) % 13));
            let ending = if i == 31 { "" } else { "\r\n" };
            original.push_str(&format!("{old}{ending}"));
            expected.push_str(&format!("{new}{ending}"));
            edits.push(TextEdit {
                old_text: old,
                new_text: new,
                start_line: Some(i + 1),
                end_line: Some(i + 1),
            });
        }
        edits.reverse();
        assert_eq!(apply_text_edits(&original, &edits).unwrap(), expected);
    }
}

#[test]
fn batch_edit_stitching_rejects_growth_overlap_and_missing_anchor() {
    let edit = |old: &str, new: &str| TextEdit {
        old_text: old.into(),
        new_text: new.into(),
        start_line: None,
        end_line: None,
    };
    assert!(apply_text_edits("abc", &[edit("ab", "X"), edit("bc", "Y")]).is_err());
    assert!(apply_text_edits("abc", &[edit("missing", "X")]).is_err());
    assert!(apply_text_edits("abc", &[edit("a", &"x".repeat(4 * 1024 * 1024))]).is_err());
}

#[test]
fn search_regex_anchors_match_interior_lines() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.go"),
        "package demo\nneedle\nfunc run() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    for pattern in ["^needle$", "^needle", "needle$"] {
        let (hits, _) = workspace
            .search_with_options(pattern, ".", 10, SearchMode::Regex, 0)
            .unwrap();
        assert_eq!(
            hits.len(),
            1,
            "anchored regex lost an interior line: {pattern}"
        );
        assert_eq!(hits[0]["line"], 2);
    }
}

#[test]
fn search_duplicate_patterns_do_not_consume_result_budget() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.go"), "alpha\nbeta\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let hits = workspace
        .search_many_with_options(
            &["alpha".into(), "alpha".into(), "beta".into()],
            ".",
            2,
            SearchMode::Exact,
            0,
        )
        .unwrap();
    assert!(
        hits.iter().any(|hit| hit["query"] == "beta"),
        "duplicate pattern exhausted the budget: {hits:?}"
    );
}

#[test]
fn advanced_search_supports_auto_token_fallback_regex_and_context() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.go"),
        "package demo\nfunc run() {\nquery := fmt.Sprintf(\"SELECT * FROM jobs\")\nflag := *svc.DriverBizInfo().IsAutoGrab\n_ = query\n_ = flag\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();

    let (token_matches, resolved) = workspace
        .search_with_options("Sprintf SELECT", ".", 100, SearchMode::Auto, 1)
        .unwrap();
    assert_eq!(resolved, SearchMode::TokensAll);
    assert_eq!(token_matches.len(), 1);
    assert_eq!(token_matches[0]["line"], 3);
    assert_eq!(token_matches[0]["context"]["start_line"], 2);
    assert_eq!(token_matches[0]["context"]["end_line"], 4);

    let patterns = vec![r"\*\w+\.\w+\(\)\.\w+".to_owned()];
    let regex_matches = workspace
        .search_many_with_options(&patterns, ".", 100, SearchMode::Regex, 1)
        .unwrap();
    assert_eq!(regex_matches.len(), 1);
    assert_eq!(regex_matches[0]["line"], 4);
    assert_eq!(regex_matches[0]["query"], patterns[0]);
    assert_eq!(
        regex_matches[0]["context"]["lines"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn regex_search_rejects_invalid_patterns_without_scanning() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.go"), "package demo\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let patterns = vec!["(".to_owned()];

    let error = workspace
        .search_many_with_options(&patterns, ".", 100, SearchMode::Regex, 0)
        .unwrap_err();
    assert!(error.to_string().contains("invalid search regex"));
}
