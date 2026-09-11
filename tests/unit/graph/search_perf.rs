use super::*;

#[test]
fn perf_exact_match_survives_an_earlier_broad_query() {
    let root = tempfile::tempdir().unwrap();
    let helpers = (0..240)
        .map(|number| format!("pub fn worker_helper_{number}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("helpers.rs"), helpers).unwrap();
    fs::write(root.path().join("target.rs"), "pub fn finish_job() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let queries = vec!["worker".to_owned(), "finish_job".to_owned()];
    for _ in 0..2 {
        let result = index
            .find_symbols_many("fixture", &workspace, &queries, ".", None, 1)
            .unwrap();
        assert_eq!(result["results"][0]["name"], "finish_job");
        assert_eq!(result["total_matches"], 241);
        assert_eq!(result["truncated"], true);
    }
}

#[test]
fn perf_multi_query_clones_only_the_best_match_per_symbol() {
    let root = tempfile::tempdir().unwrap();
    let source = (0..128)
        .map(|number| format!("pub fn worker_item_{number}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("worker.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let ensured = index
        .ensure_indexed(&workspace, "worker.rs", false)
        .unwrap();
    let queries = ["worker", "worker_item", "worker_item_0"].map(str::to_owned);
    let matches = crate::code_index::symbols::matching_symbols_many(
        &ensured.record,
        &queries,
        Some("function"),
    );
    assert_eq!(
        matches.len(),
        128,
        "one candidate per symbol, not per keyword"
    );
    let exact = matches
        .iter()
        .find(|(_, _, symbol)| symbol.name == "worker_item_0")
        .unwrap();
    assert_eq!((exact.0, exact.1), (2, 0));
    assert!(crate::code_index::symbols::matching_symbols_many(
        &ensured.record,
        &queries,
        Some("class"),
    )
    .is_empty());
}
