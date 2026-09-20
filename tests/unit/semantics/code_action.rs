use super::*;

#[test]
fn organize_imports_workspace_edit_becomes_one_sha_guarded_target_edit() {
    let root = tempfile::tempdir().unwrap();
    let original = "use z::Z;\nuse a::A;\nfn main() {}\n";
    std::fs::write(root.path().join("main.rs"), original).unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let source = workspace.load_source("main.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let workspace_edit = json!({
        "changes": {
            uri: [{
                "range": {
                    "start": {"line":0,"character":0},
                    "end": {"line":2,"character":0}
                },
                "newText": "use a::A;\nuse z::Z;\n"
            }]
        }
    });

    let files = workspace_edit_to_guarded_files(
        &workspace,
        &workspace_edit,
        "utf-8",
        "main.rs",
        &source.sha256,
    )
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "main.rs");
    assert_eq!(files[0]["expected_sha256"], source.sha256);
    assert_eq!(files[0]["edits"].as_array().unwrap().len(), 1);

    let requests =
        serde_json::from_value::<Vec<crate::workspace::FileEditRequest>>(Value::Array(files))
            .unwrap();
    let applied = workspace.apply_file_edits(&requests).unwrap();
    assert!(applied.iter().all(|item| item.ok));
    assert_eq!(
        std::fs::read_to_string(root.path().join("main.rs")).unwrap(),
        "use a::A;\nuse z::Z;\nfn main() {}\n"
    );
}

#[test]
fn organize_imports_rejects_multi_file_and_sensitive_edits() {
    let root = tempfile::tempdir().unwrap();
    let main = "use z::Z;\nfn main() {}\n";
    std::fs::write(root.path().join("main.rs"), main).unwrap();
    std::fs::write(root.path().join("other.rs"), "use z::Z;\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let source = workspace.load_source("main.rs").unwrap();
    let main_uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let other_uri = Url::from_file_path(root.path().join("other.rs"))
        .unwrap()
        .to_string();
    let multi = json!({
        "changes": {
            main_uri: [{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"use"}],
            other_uri: [{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"use"}]
        }
    });
    let error =
        workspace_edit_to_guarded_files(&workspace, &multi, "utf-8", "main.rs", &source.sha256)
            .unwrap_err();
    assert!(error.to_string().contains("exactly one target file"));

    let sentinel = "synthetic-private-fixture";
    let sensitive = format!("let password = \"{sentinel}\"; use z::Z;\n");
    std::fs::write(root.path().join("main.rs"), &sensitive).unwrap();
    let source = workspace.load_source("main.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let start = sensitive.find("use z::Z").unwrap() as u64;
    let sensitive_edit = json!({
        "changes": {
            uri: [{
                "range":{"start":{"line":0,"character":start},"end":{"line":0,"character":start + 8}},
                "newText":"use a::A"
            }]
        }
    });
    let error = workspace_edit_to_guarded_files(
        &workspace,
        &sensitive_edit,
        "utf-8",
        "main.rs",
        &source.sha256,
    )
    .unwrap_err();
    assert!(error.to_string().contains("sensitive source"));
    assert!(!error.to_string().contains(sentinel));
}

#[test]
fn organize_imports_rejects_ambiguous_same_position_insertions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let source = workspace.load_source("main.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();
    let workspace_edit = json!({
        "changes": {
            uri: [
                {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"use a::A;\n"},
                {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"use b::B;\n"}
            ]
        }
    });
    let error = workspace_edit_to_guarded_files(
        &workspace,
        &workspace_edit,
        "utf-8",
        "main.rs",
        &source.sha256,
    )
    .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
}

#[test]
fn quick_fix_diagnostic_matching_respects_lsp_half_open_ranges() {
    let diagnostic = json!({
        "range":{
            "start":{"line":2,"character":3},
            "end":{"line":2,"character":7}
        },
        "message":"broken"
    });
    assert!(diagnostic_contains_position(&diagnostic, 2, 3));
    assert!(diagnostic_contains_position(&diagnostic, 2, 6));
    assert!(!diagnostic_contains_position(&diagnostic, 2, 7));
    assert!(!diagnostic_contains_position(&diagnostic, 1, 6));

    let zero_width = json!({
        "range":{
            "start":{"line":4,"character":5},
            "end":{"line":4,"character":5}
        },
        "message":"point diagnostic"
    });
    assert!(diagnostic_contains_position(&zero_width, 4, 5));
    assert!(!diagnostic_contains_position(&zero_width, 4, 6));
}

#[test]
fn quick_fix_selection_requires_one_unambiguous_preferred_action() {
    assert_eq!(select_quick_fix_index(&[false], "mock").unwrap(), 0);
    assert_eq!(
        select_quick_fix_index(&[false, true, false], "mock").unwrap(),
        1
    );
    assert!(select_quick_fix_index(&[false, false], "mock")
        .unwrap_err()
        .to_string()
        .contains("refuses to guess"));
    assert!(select_quick_fix_index(&[true, true], "mock")
        .unwrap_err()
        .to_string()
        .contains("refuses to guess"));
}

#[test]
fn organize_imports_rejects_provider_commands() {
    let command_action = json!({
        "title":"Organize Imports",
        "kind":"source.organizeImports",
        "command":{"title":"run","command":"provider.organize"}
    });
    assert!(has_provider_command(&command_action));
    assert!(!has_provider_command(&json!({
        "title":"Organize Imports",
        "kind":"source.organizeImports",
        "edit":{"changes":{}}
    })));
}

#[test]
fn organize_imports_rejects_mixed_edit_shapes_and_stale_target_revision() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "use z::Z;\nfn main() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let source = workspace.load_source("main.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("main.rs"))
        .unwrap()
        .to_string();

    let mixed = json!({
        "changes": {
            uri.clone(): [{
                "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":9}},
                "newText":"use a::A;"
            }]
        },
        "documentChanges": [{
            "textDocument":{"uri":uri.clone(),"version":1},
            "edits":[{
                "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":9}},
                "newText":"use a::A;"
            }]
        }]
    });
    let error =
        workspace_edit_to_guarded_files(&workspace, &mixed, "utf-8", "main.rs", &source.sha256)
            .unwrap_err();
    assert!(error
        .to_string()
        .contains("both changes and documentChanges"));

    std::fs::write(root.path().join("main.rs"), "use b::B;\nfn main() {}\n").unwrap();
    let stale = json!({
        "changes": {
            uri: [{
                "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":9}},
                "newText":"use a::A;"
            }]
        }
    });
    let error =
        workspace_edit_to_guarded_files(&workspace, &stale, "utf-8", "main.rs", &source.sha256)
            .unwrap_err();
    assert!(error.to_string().contains("target changed"));
}
