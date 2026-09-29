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
fn lsp_locations_reject_out_of_bounds_or_split_codepoint_columns() {
    let source = "a🙂b\n";
    assert!(lsp_to_byte_column(source, 1, 2, "utf-8").is_err());
    assert!(lsp_to_byte_column(source, 1, 7, "utf-8").is_err());
    assert!(lsp_to_byte_column(source, 1, 2, "utf-16").is_err());
    assert!(lsp_to_byte_column(source, 1, 5, "utf-16").is_err());
    assert!(lsp_to_byte_column(source, 1, 4, "utf-32").is_err());

    let crlf_source = "abc\r\ndef\r\n";
    assert_eq!(lsp_to_byte_column(crlf_source, 1, 3, "utf-8").unwrap(), 4);
    assert!(lsp_to_byte_column(crlf_source, 1, 4, "utf-8").is_err());
    assert!(lsp_to_byte_column(crlf_source, 1, 4, "utf-16").is_err());
    assert!(lsp_to_byte_column(crlf_source, 1, 4, "utf-32").is_err());
    assert!(byte_column_to_lsp(crlf_source, 1, 5, "utf-8").is_err());
}

#[test]
fn malformed_lsp_locations_mark_semantic_coverage_incomplete() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "a🙂b\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let malformed = json!([{
        "uri": uri,
        "range": {
            "start": {"line": 0, "character": 2},
            "end": {"line": 0, "character": 3}
        }
    }]);
    let mut cache = NavigationLocationCache::default();
    let mut locations = Vec::new();
    let mut incomplete = false;
    append_locations(
        &workspace,
        &malformed,
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);

    locations.clear();
    incomplete = false;
    append_locations(
        &workspace,
        &json!([{
            "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
            "range": {
                "start": {"line": 0, "character": 1},
                "end": {"line": 0, "character": 0}
            }
        }]),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);

    locations.clear();
    incomplete = false;
    append_locations(
        &workspace,
        &json!([{
            "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 2}
            }
        }]),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);

    let mut incoming = Vec::new();
    let mut call_incomplete = false;
    {
        let mut context = CallLocationContext {
            workspace: &workspace,
            encoding: "utf-8",
            max_results: 24,
            cache: &mut cache,
            truncated: &mut call_incomplete,
        };
        append_call_locations(
            &mut context,
            &json!([{
                "from": {
                    "name": "caller",
                    "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
                    "range": {
                        "start": {"line": 0, "character": 2},
                        "end": {"line": 0, "character": 3}
                    }
                }
            }]),
            "from",
            None,
            &mut incoming,
        );
    }
    assert!(incoming.is_empty());
    assert!(call_incomplete);

    incoming.clear();
    call_incomplete = false;
    {
        let mut context = CallLocationContext {
            workspace: &workspace,
            encoding: "utf-8",
            max_results: 24,
            cache: &mut cache,
            truncated: &mut call_incomplete,
        };
        append_call_locations(
            &mut context,
            &json!([{
                "from": {
                    "name": "caller",
                    "kind": 12,
                    "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    },
                    "selectionRange": {
                        "start": {"line": 0, "character": 5},
                        "end": {"line": 0, "character": 6}
                    }
                }
            }]),
            "from",
            None,
            &mut incoming,
        );
    }
    assert!(incoming.is_empty());
    assert!(call_incomplete);

    incoming.clear();
    call_incomplete = false;
    let caller_uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    {
        let mut context = CallLocationContext {
            workspace: &workspace,
            encoding: "utf-8",
            max_results: 24,
            cache: &mut cache,
            truncated: &mut call_incomplete,
        };
        append_call_locations(
            &mut context,
            &json!([{
                "from": {
                    "name": "caller",
                    "kind": 12,
                    "uri": caller_uri,
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 6}
                    },
                    "selectionRange": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    }
                }
            }]),
            "from",
            None,
            &mut incoming,
        );
    }
    assert!(incoming.is_empty());
    assert!(call_incomplete);

    incoming.clear();
    call_incomplete = false;
    {
        let mut context = CallLocationContext {
            workspace: &workspace,
            encoding: "utf-8",
            max_results: 24,
            cache: &mut cache,
            truncated: &mut call_incomplete,
        };
        append_call_locations(
            &mut context,
            &json!([{
                "from": {
                    "name": "caller",
                    "kind": 12,
                    "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 6}
                    },
                    "selectionRange": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    }
                },
                "fromRanges": []
            }]),
            "from",
            None,
            &mut incoming,
        );
    }
    assert!(incoming.is_empty());
    assert!(call_incomplete);
}

#[test]
fn collection_location_responses_reject_singleton_objects() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "fn demo() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let mut cache = NavigationLocationCache::default();
    let mut locations = Vec::new();
    let mut incomplete = false;

    append_locations(
        &workspace,
        &json!({
            "uri": uri,
            "range": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);

    locations.clear();
    incomplete = false;
    append_locations(
        &workspace,
        &json!([{
            "uri": Url::from_file_path(root.path().join("main.rs")).unwrap().to_string(),
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 12}
            },
            "selectionRange": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }]),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);
}

#[test]
fn lsp_location_links_reject_mixed_or_contradictory_target_identity() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "fn demo() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let mut cache = NavigationLocationCache::default();
    let mut locations = Vec::new();
    let mut incomplete = false;

    append_locations(
        &workspace,
        &json!([{
            "targetUri": uri.clone(),
            "targetRange": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            },
            "targetSelectionRange": {
                "start": {"line": 0, "character": 8},
                "end": {"line": 0, "character": 10}
            }
        }]),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);

    locations.clear();
    incomplete = false;
    append_locations(
        &workspace,
        &json!([{
            "uri": uri.clone(),
            "targetUri": uri,
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 7}
            },
            "targetRange": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 7}
            },
            "targetSelectionRange": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }]),
        "utf-8",
        24,
        &mut cache,
        &mut locations,
        &mut incomplete,
    );
    assert!(locations.is_empty());
    assert!(incomplete);
}

#[test]
fn prepared_call_hierarchy_item_must_match_the_requested_source_and_position() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "fn demo() {}\n").unwrap();
    std::fs::write(root.path().join("other.rs"), "fn other() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let other_uri = Url::from_file_path(root.path().join("other.rs"))
        .unwrap()
        .to_string();
    let position = json!({"line": 0, "character": 4});
    let prepared = |item_uri: &str, selection_end: u64| {
        json!([{
            "name":"demo",
            "kind":12,
            "uri":item_uri,
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":selection_end}},
            "selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":selection_end}}
        }])
    };

    let mut failures = Vec::new();
    let mut cache = NavigationLocationCache::default();
    assert!(prepared_call_hierarchy_item_for_request(
        &workspace,
        &prepared(&uri, 7),
        &uri,
        &position,
        "utf-8",
        &mut cache,
        &mut failures,
    )
    .is_some());
    assert!(failures.is_empty());

    failures.clear();
    assert!(prepared_call_hierarchy_item_for_request(
        &workspace,
        &prepared(&uri, 7),
        &uri,
        &json!({"line": 0, "character": 7}),
        "utf-8",
        &mut cache,
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item_for_request(
        &workspace,
        &prepared(&other_uri, 8),
        &uri,
        &position,
        "utf-8",
        &mut cache,
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item_for_request(
        &workspace,
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":uri.clone(),
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":99}},
            "selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":99}}
        }]),
        &uri,
        &position,
        "utf-8",
        &mut cache,
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item_for_request(
        &workspace,
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":uri.clone(),
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":7}},
            "selectionRange":{"start":{"line":0,"character":5},"end":{"line":0,"character":7}}
        }]),
        &uri,
        &position,
        "utf-8",
        &mut cache,
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);
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

#[test]
fn empty_call_hierarchy_prepare_is_recorded_as_failed_coverage() {
    let mut failures = Vec::new();
    assert!(prepared_call_hierarchy_item(&json!([]), &mut failures).is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item(&json!([{"name":"demo"}]), &mut failures).is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item(
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":"file:///tmp/demo.rs",
            "range":{},
            "selectionRange":{}
        }]),
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item(
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":"file:///tmp/demo.rs",
            "range":{"start":{"line":2,"character":4},"end":{"line":1,"character":9}},
            "selectionRange":{"start":{"line":2,"character":4},"end":{"line":2,"character":8}}
        }]),
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item(
        &json!([
            {"name":"demo","kind":12,"uri":"file:///tmp/demo.rs","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":7}},"selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":7}}},
            {"name":"other","kind":12,"uri":"file:///tmp/demo.rs","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":8}},"selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":8}}}
        ]),
        &mut failures,
    ).is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    assert!(prepared_call_hierarchy_item(
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":"file:///tmp/demo.rs",
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},
            "selectionRange":{"start":{"line":0,"character":4},"end":{"line":0,"character":7}}
        }]),
        &mut failures,
    )
    .is_none());
    assert_eq!(failures, ["calls"]);

    failures.clear();
    let item = prepared_call_hierarchy_item(
        &json!([{
            "name":"demo",
            "kind":12,
            "uri":"file:///tmp/demo.rs",
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":7}},
            "selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":7}}
        }]),
        &mut failures,
    )
    .unwrap();
    assert_eq!(item["name"], "demo");
    assert!(failures.is_empty());
}
