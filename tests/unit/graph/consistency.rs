use super::*;
use std::sync::Barrier;

#[test]
fn syntax_search_limit_and_failures_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("a.go"),
        "package demo\nfunc first() {}\nfunc second() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let mut query = SyntaxSearchRequest {
        path: "a.go".into(),
        node_kinds: vec!["function_declaration".into()],
        text_regex: None,
        max_files: 100,
        max_results: 1,
    };
    let partial = index.search_ast_nodes("test", &workspace, &query).unwrap();
    assert_eq!(partial["count"], 1);
    assert_eq!(partial["truncated"], true);
    assert_eq!(partial["results_truncated"], true);
    assert_eq!(partial["coverage_complete"], false);
    assert_eq!(
        partial["matches"][0]["sha256"],
        workspace.read_file("a.go", 1, None).unwrap().sha256
    );
    query.max_results = 100;
    let complete = index.search_ast_nodes("test", &workspace, &query).unwrap();
    assert_eq!(complete["count"], 2);
    assert_eq!(complete["coverage_complete"], true);
    fs::write(root.path().join("bad.go"), [0xff, 0xfe]).unwrap();
    query.path = ".".into();
    let failed = index.search_ast_nodes("test", &workspace, &query).unwrap();
    assert_eq!(failed["files_failed"], 1);
    assert_eq!(failed["coverage_complete"], false);
}

#[test]
fn concurrent_cold_queries_share_one_parse_across_entrypoints() {
    let root = tempfile::tempdir().unwrap();
    let source = (0..4_000)
        .map(|number| format!("pub fn job_{number}() {{ helper(); }}\n"))
        .collect::<String>();
    fs::write(root.path().join("jobs.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let barrier = Barrier::new(32);
    let started = std::time::Instant::now();
    let parsed = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for number in 0..32 {
            let index = &index;
            let workspace = &workspace;
            let barrier = &barrier;
            workers.push(scope.spawn(move || {
                barrier.wait();
                match number % 3 {
                    0 => {
                        let result = index.ensure_indexed(workspace, "jobs.rs", false).unwrap();
                        assert_eq!(
                            result
                                .record
                                .symbols
                                .iter()
                                .filter(|s| s.is_definition)
                                .count(),
                            4_000
                        );
                        usize::from(!result.symbol_cache_hit)
                    }
                    1 => {
                        let result = index
                            .search_file(workspace, "jobs.rs", "job_42", None)
                            .unwrap();
                        assert!(result.matches.iter().any(|(_, s)| s.name == "job_42"));
                        usize::from(result.parsed)
                    }
                    _ => {
                        let result = index
                            .search_file_many(workspace, "jobs.rs", &["job_42".into()], None)
                            .unwrap();
                        assert!(result.matches.iter().any(|(_, _, s)| s.name == "job_42"));
                        usize::from(result.parsed)
                    }
                }
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum::<usize>()
    });
    let elapsed_us = started.elapsed().as_micros();
    assert_eq!(
        parsed, 1,
        "the same source revision should be parsed once, not once per caller"
    );
    let report = json!({"requests":32,"actual_builds":parsed,"elapsed_us":elapsed_us,
        "definitions_per_file":4_000,"scope":"concurrent cold source queries; not model/network latency"});
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wcode-index-sharing.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

fn replace_preserving_stamp(path: &Path, content: &str) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), content.len() as u64);
    fs::write(path, content).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[test]
fn observed_source_stamp_rejects_a_changed_read() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("job.rs");
    fs::write(&path, "pub fn job() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let stamp = workspace.source_stamp("job.rs").unwrap();
    fs::write(&path, "pub fn job() { changed(); }\n").unwrap();

    let error = workspace
        .load_source_at_stamp("job.rs", &stamp)
        .unwrap_err()
        .to_string();
    assert!(error.contains("changed while it was being read"), "{error}");

    let current = workspace.source_stamp("job.rs").unwrap();
    let source = workspace.load_source_at_stamp("job.rs", &current).unwrap();
    assert!(source.content.contains("changed()"));
}

#[test]
fn symbol_context_never_mixes_cached_relations_with_new_body() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("job.rs");
    fs::write(&path, "pub fn job() { old(); }\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let outline = index
        .file_outline("fixture", &workspace, "job.rs", 10)
        .unwrap();
    let id = outline["symbols"][0]["id"].as_str().unwrap();
    let stamp = workspace.source_stamp("job.rs").unwrap();
    replace_preserving_stamp(&path, "pub fn job() { new(); }\n");
    let replayed = workspace.source_stamp("job.rs").unwrap();
    #[cfg(unix)]
    assert_ne!(
        replayed, stamp,
        "Unix source stamps must include change metadata that mtime replay cannot hide"
    );
    #[cfg(not(unix))]
    assert_eq!(
        replayed, stamp,
        "non-Unix cache freshness relies on the content-SHA fallback when basic metadata is replayed"
    );
    let result = index.symbol_context("fixture", &workspace, id, 20).unwrap();
    assert!(result["body"]["content"]
        .as_str()
        .unwrap()
        .contains("new()"));
    let calls = result["syntax_calls"].as_array().unwrap();
    assert!(
        calls.iter().any(|call| call["name"] == "new"),
        "new body must not carry old call relations: {result}"
    );
    assert!(!calls.iter().any(|call| call["name"] == "old"));
    assert_ne!(result["sha256"], outline["sha256"]);
}

#[test]
fn cached_symbol_search_rejects_same_size_same_mtime_rewrite() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("job.rs");
    fs::write(&path, "pub fn alpha() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let first = index
        .find_symbol("fixture", &workspace, "alpha", ".", None, 10)
        .unwrap();
    assert_eq!(first["result_count"], 1);
    replace_preserving_stamp(&path, "pub fn bravo() {}\n");

    let stale = index
        .find_symbol("fixture", &workspace, "alpha", ".", None, 10)
        .unwrap();
    assert_eq!(
        stale["result_count"], 0,
        "old symbol must not survive cache replay"
    );
    let fresh = index
        .find_symbol("fixture", &workspace, "bravo", ".", None, 10)
        .unwrap();
    assert_eq!(
        fresh["result_count"], 1,
        "new symbol must be indexed immediately"
    );
}

#[test]
fn changed_symbol_identity_is_not_reused_for_a_different_definition() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("job.rs");
    fs::write(&path, "pub fn job() { old(); }\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let outline = index
        .file_outline("fixture", &workspace, "job.rs", 10)
        .unwrap();
    replace_preserving_stamp(&path, "pub fn run() { new(); }\n");
    let result = index.symbol_context(
        "fixture",
        &workspace,
        outline["symbols"][0]["id"].as_str().unwrap(),
        20,
    );
    assert!(
        result.is_err(),
        "an old symbol ID must not name the new definition"
    );
}
