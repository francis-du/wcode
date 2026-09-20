use super::*;

fn locations(query: &str) -> Vec<(String, Option<usize>)> {
    query_anchors(query)
        .into_iter()
        .map(|anchor| (anchor.path, anchor.line))
        .collect()
}

fn detailed_locations(query: &str) -> Vec<(String, Option<usize>, Option<usize>)> {
    query_anchors(query)
        .into_iter()
        .map(|anchor| (anchor.path, anchor.line, anchor.column))
        .collect()
}

#[test]
fn trace_format_wrappers_preserve_quoted_locations() {
    for quote in ['\"', '\'', '`'] {
        for (open, close) in [("(", ")"), ("[", "]"), ("【", "】")] {
            let query = format!("at worker {open}{quote}src/a b.ts:23:7{quote}{close}");
            assert_eq!(
                locations(&query),
                vec![("src/a b.ts".to_owned(), Some(23))],
                "{query}"
            );
        }
    }
}

#[test]
fn trace_format_matrix_keeps_duplicate_frames_bounded_and_line_scoped() {
    for path in ["src/a.py", "源 码/worker file.py", r"src\worker.py"] {
        for number in [1, 2, 37, 2048] {
            for quote in ['\"', '\'', '`'] {
                for newline in ["\n", "\r\n"] {
                    let frame = format!("  File {quote}{path}{quote}, line {number}, in run");
                    let query = format!("Traceback:{newline}{frame}{newline}{frame}");
                    assert_eq!(
                        locations(&query),
                        vec![(path.replace('\\', "/"), Some(number))]
                    );
                }
            }
        }
    }
    let query = (1..=8)
        .map(|line| format!("File \"src/a.py\", line {line}, in run\n"))
        .collect::<String>();
    assert_eq!(
        locations(&query),
        (1..=MAX_ANCHORS)
            .map(|line| ("src/a.py".to_owned(), Some(line)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        locations("src/a.py\nline 90\nsrc/b.py:4"),
        vec![
            ("src/a.py".to_owned(), None),
            ("src/b.py".to_owned(), Some(4))
        ]
    );
}

#[test]
fn trace_format_invalid_locations_never_read_existing_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("worker.py"), "never_read_line_one = 1\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for number in ["0", "-1", "+2", "bad", "99999999999999999999999999999999"] {
        let mut targets = Vec::new();
        let query = format!("File \"worker.py\", line {number}, in run");
        let records = retrieve(&harness, "demo", &workspace, &query, &mut targets).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["status"], "invalid_location");
        assert!(records[0].get("source").is_none());
        assert!(targets.is_empty());
    }
}

#[test]
fn trace_format_quoted_paths_keep_workspace_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let external = outside.path().join("outside file.py");
    std::fs::write(&external, "do_not_read_external = 1\n").unwrap();
    std::fs::write(root.path().join(".env"), "synthetic_test_only = 1\n").unwrap();
    for path in [
        external.to_string_lossy().into_owned(),
        "../outside file.py".to_owned(),
        ".env".to_owned(),
    ] {
        let mut targets = Vec::new();
        let query = format!("File \"{path}\", line 1, in run");
        let records = retrieve(&harness, "demo", &workspace, &query, &mut targets).unwrap();
        assert_eq!(records.len(), 1);
        assert_ne!(records[0]["status"], "resolved");
        assert!(records[0].get("source").is_none());
        assert!(targets.is_empty());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&external, root.path().join("linked file.py")).unwrap();
        let records = retrieve(
            &harness,
            "demo",
            &workspace,
            "File \"linked file.py\", line 1, in run",
            &mut Vec::new(),
        )
        .unwrap();
        assert_ne!(records[0]["status"], "resolved");
        assert!(records[0].get("source").is_none());
    }
}

#[test]
fn trace_format_malformed_or_oversized_quotes_do_not_emit_local_suffixes() {
    for quote in ['\"', '\'', '`'] {
        assert!(locations(&format!(
            "File {quote}https://example.org/dir with spaces/a.py, line 8"
        ))
        .is_empty());
        assert!(locations(&format!(
            "File {quote}{} trailing.py{quote}, line 8",
            "a".repeat(600)
        ))
        .is_empty());
    }
}

#[test]
fn trace_format_python_frames_keep_line_and_quoted_spaces() {
    let query = "Traceback (most recent call last):\n  File \"src/worker.py\", line 23, in worker\n  File \"源 码/worker file.py\", line 41, in run\nValueError: bad input";
    assert_eq!(
        locations(query),
        vec![
            ("src/worker.py".to_owned(), Some(23)),
            ("源 码/worker file.py".to_owned(), Some(41)),
        ]
    );
}

#[test]
fn trace_format_compilers_keep_parenthesized_line_and_column() {
    for path in ["src/worker.ts", "src/worker.tsx", "src/service.cs"] {
        let query = format!("{path}(23,7): error: invalid argument");
        assert_eq!(locations(&query), vec![(path.to_owned(), Some(23))]);
        assert_eq!(
            detailed_locations(&query),
            vec![(path.to_owned(), Some(23), Some(7))]
        );
    }
}

#[test]
fn trace_format_php_keeps_on_line_location() {
    assert_eq!(
        locations("PHP Warning: Undefined variable in src/handler.php on line 29"),
        vec![("src/handler.php".to_owned(), Some(29))]
    );
}

#[test]
fn trace_format_quoted_locations_preserve_whole_path() {
    for quote in ['\"', '\'', '`'] {
        let query = format!("fix {quote}src/a b.ts{quote}:23:7");
        assert_eq!(locations(&query), vec![("src/a b.ts".to_owned(), Some(23))]);
    }
}

#[test]
fn trace_format_invalid_frame_lines_do_not_fall_back_to_file_start() {
    for number in ["0", "-1", "banana", "999999999999999999999999999999999"] {
        let query = format!("File \"src/worker.py\", line {number}, in run");
        assert_eq!(
            locations(&query),
            vec![("src/worker.py".to_owned(), Some(0))]
        );
    }
}

#[test]
fn trace_format_url_frames_are_not_reinterpreted_as_local_suffixes() {
    for path in [
        "https://example.org/src/a.py",
        "file:///outside/a.py",
        "https://example.org/dir with spaces/a.py",
    ] {
        assert!(locations(&format!("File \"{path}\", line 8, in run")).is_empty());
    }
}

#[test]
fn trace_format_single_context_call_supplies_exact_line_for_guarded_edit() {
    for budget in [1_000, 1_400, 4_000] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        let path = "src/worker file.py";
        let original = format!(
            "{}def worker():\n    return 1\n",
            "# unrelated header\n".repeat(35)
        );
        std::fs::write(root.path().join(path), &original).unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let harness = ToolHarness::new(4).unwrap();
        let query = format!("Traceback (most recent call last):\n  File \"{path}\", line 37, in worker\nAssertionError: expected 2");
        let pack = harness
            .agent_context("demo", &workspace, &query, budget, &[])
            .unwrap();
        let source = &pack["hot_source"][0];
        assert_eq!(source["path"], path);
        assert_eq!(source["body"]["start_line"], 37);
        assert_eq!(source["body"]["end_line"], 37);
        assert_eq!(source["body"]["redacted"], false);
        let text = source["body"]["content"].as_str().unwrap();
        assert!(text.contains("return 1"));
        assert!(original.contains(text));
        let sha = source["sha256"].as_str().unwrap();
        let changed = text.replace("return 1", "return 2");
        workspace.replace_text(path, text, &changed, sha).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join(path)).unwrap(),
            original.replace("return 1", "return 2")
        );
        assert!(workspace.replace_text(path, &changed, text, sha).is_err());
        assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
    }
}
