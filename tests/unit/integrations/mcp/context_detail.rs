use super::*;

async fn project_context(state: &AppState, arguments: Value) -> Result<Value, String> {
    call_tool(
        state,
        json!({"name": "project_context", "arguments": arguments}),
    )
    .await
}

#[tokio::test]
async fn project_context_defaults_to_summary_and_restores_full_projection() {
    let root = tempfile::tempdir().unwrap();
    // Twenty maintained modules above the 600-line review threshold produce
    // more findings than the summary keeps, so truncation is exercised for
    // real rather than only asserted on a small fixture.
    std::fs::create_dir(root.path().join("src")).unwrap();
    for index in 0..20 {
        let body = format!("pub mod m{index};\n{}\n", "pub fn f() {}\n".repeat(600));
        std::fs::write(root.path().join(format!("src/mod_{index}.rs")), body).unwrap();
    }
    std::fs::write(
        root.path().join("AGENTS.md"),
        format!(
            "header\n{}\n",
            "guidance line of reasonable length\n".repeat(160)
        ),
    )
    .unwrap();
    let state = batch_test_state(root.path());

    let response = project_context(&state, json!({})).await.unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let summary = &response["structuredContent"];
    assert_eq!(summary["detail"], "summary");

    let conventions = &summary["conventions"];
    let total = conventions["findings_total"].as_u64().unwrap();
    assert!(total > 16, "fixture must exceed the summary sample");
    let sample = conventions["findings"].as_array().unwrap();
    assert_eq!(sample.len(), 16);
    assert_eq!(conventions["findings_truncated"], true);
    let aggregated: u64 = conventions["findings_by_code"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["count"].as_u64().unwrap())
        .sum();
    assert_eq!(
        aggregated, total,
        "the aggregate must account for every finding"
    );
    assert!(
        conventions["errors"].as_u64().unwrap() + conventions["warnings"].as_u64().unwrap()
            >= total,
        "the aggregate counts must still describe every finding"
    );

    for language in summary["language_quality"]["languages"].as_array().unwrap() {
        for provider in language["providers"].as_array().unwrap() {
            for key in ["command", "reason", "root", "execution_lane", "source"] {
                assert!(
                    provider.get(key).is_none(),
                    "summary provider must not carry {key}"
                );
            }
        }
    }

    let excerpt = summary["guidance"][0]["excerpt"].as_str().unwrap();
    assert_eq!(excerpt.chars().count(), 4_000);
    assert_eq!(summary["guidance"][0]["excerpt_truncated"], true);

    let response = project_context(&state, json!({"detail": "full"}))
        .await
        .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let full = &response["structuredContent"];
    assert!(full.get("detail").is_none());
    assert!(full["conventions"].get("findings_by_code").is_none());
    assert!(full["conventions"].get("findings_total").is_none());
    assert!(full["conventions"].get("findings_truncated").is_none());
    let full_findings = full["conventions"]["findings"].as_array().unwrap();
    assert!(full_findings.len() > sample.len());
    for language in full["language_quality"]["languages"].as_array().unwrap() {
        for provider in language["providers"].as_array().unwrap() {
            assert!(provider.get("command").is_some());
        }
    }
    assert!(
        full["guidance"][0]["excerpt"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            > 4_000
    );
    assert!(full["guidance"][0].get("excerpt_truncated").is_none());

    let rejected = project_context(&state, json!({"detail": "everything"})).await;
    assert!(rejected.is_err(), "unknown detail must fail closed");
}
