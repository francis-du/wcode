use super::*;

const BOUNDED_LIMIT_TOOLS: &[&str] = &[
    "graph_history",
    "semantic_status",
    "semantic_query",
    "evidence_status",
    "verification_history",
    "reconciliation_history",
];

fn limit_arguments(name: &str, limit: Value) -> Value {
    let mut arguments = json!({"limit": limit});
    if name == "semantic_query" {
        arguments["query"] = json!("baseline");
    }
    arguments
}

async fn call_with_limit(state: &AppState, name: &str, arguments: Value) -> Result<Value, String> {
    call_tool(state, json!({"name": name, "arguments": arguments})).await
}

#[tokio::test]
async fn bounded_limit_tools_reject_values_below_the_declared_minimum() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for name in BOUNDED_LIMIT_TOOLS {
        for limit in [json!(0), json!(-1), json!(1.5), json!("10")] {
            let arguments = limit_arguments(name, limit);
            let error = call_with_limit(&state, name, arguments.clone())
                .await
                .expect_err("limits below the schema minimum must fail closed");
            assert!(
                error.contains("limit must be"),
                "unexpected error for {name} {arguments}: {error}"
            );
        }
    }
}

#[tokio::test]
async fn bounded_limit_tools_accept_schema_permitted_values() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for name in BOUNDED_LIMIT_TOOLS {
        for arguments in [
            limit_arguments(name, json!(1)),
            limit_arguments(name, json!(20)),
        ] {
            let response = call_with_limit(&state, name, arguments.clone())
                .await
                .unwrap_or_else(|error| panic!("{name} {arguments} must be accepted: {error}"));
            assert_eq!(response["isError"], false, "{name}: {response}");
        }
        let mut omitted = json!({});
        if *name == "semantic_query" {
            omitted["query"] = json!("baseline");
        }
        let response = call_with_limit(&state, name, omitted.clone())
            .await
            .unwrap_or_else(|error| panic!("{name} {omitted} must be accepted: {error}"));
        assert_eq!(response["isError"], false, "{name}: {response}");
    }
}

#[tokio::test]
async fn bounded_limit_tools_keep_existing_maximum_and_clamp_behavior() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for name in BOUNDED_LIMIT_TOOLS {
        let arguments = limit_arguments(name, json!(1_000));
        let response = call_with_limit(&state, name, arguments.clone())
            .await
            .unwrap_or_else(|error| panic!("{name} {arguments} must stay accepted: {error}"));
        assert_eq!(response["isError"], false, "{name}: {response}");
    }
}
