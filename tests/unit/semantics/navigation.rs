use super::*;

#[test]
fn position_conversion_preserves_utf8_and_utf16_boundaries() {
    let source = "fn café() {}\n";
    let byte_column = 9;
    assert_eq!(
        byte_column_to_lsp(source, 1, byte_column, "utf-8").unwrap(),
        8
    );
    assert_eq!(
        byte_column_to_lsp(source, 1, byte_column, "utf-16").unwrap(),
        7
    );
    assert_eq!(
        lsp_to_byte_column(source, 1, 7, "utf-16").unwrap(),
        byte_column
    );
}

#[test]
fn position_conversion_accepts_the_final_empty_line() {
    let source = "fn demo() {}\n";
    assert_eq!(byte_column_to_lsp(source, 2, 1, "utf-16").unwrap(), 0);
    assert_eq!(lsp_to_byte_column(source, 2, 0, "utf-16").unwrap(), 1);
}

#[test]
fn hover_text_accepts_markdown_and_marked_string_shapes() {
    let value = json!({"contents":[{"language":"rust","value":"fn demo()"},"docs"]});
    assert_eq!(hover_text(&value).as_deref(), Some("fn demo()\ndocs"));
}

#[test]
fn navigation_intent_wire_names_are_stable() {
    assert_eq!(
        serde_json::to_value(SemanticNavigationIntent::References).unwrap(),
        "references"
    );
    assert_eq!(
        serde_json::to_value(SemanticNavigationIntent::Implementations).unwrap(),
        "implementations"
    );
    assert_eq!(
        serde_json::to_value(SemanticNavigationIntent::IncomingCalls).unwrap(),
        "incoming_calls"
    );
    assert_eq!(
        serde_json::to_value(SemanticNavigationIntent::OutgoingCalls).unwrap(),
        "outgoing_calls"
    );
}

#[test]
fn navigation_distinguishes_unsupported_from_failed_queries() {
    let mut unsupported = Vec::new();
    let mut failures = Vec::new();
    record_query_status(
        &mut unsupported,
        &mut failures,
        "references",
        NavigationQueryStatus::Unsupported,
    );
    record_query_status(
        &mut unsupported,
        &mut failures,
        "implementations",
        NavigationQueryStatus::Failed,
    );
    assert_eq!(unsupported, ["references"]);
    assert_eq!(failures, ["implementations"]);
}
