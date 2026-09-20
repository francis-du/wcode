use super::*;

#[test]
fn missing_lsp_is_installed_before_refresh_and_semantic_navigation() {
    let mut value = json!({
        "query": "find callers and references for target",
        "targets": [{"id":"target","kind":"function","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"target",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"fn target() {}","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"syntax","truncated":false},
        "semantic_provider_hints": [{
            "language":"rust",
            "provider":null,
            "action":"install_lsp",
            "install":{
                "provider":"rust-analyzer",
                "model_can_install":true
            }
        }],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    let index = |name: &str| {
        actions
            .iter()
            .position(|action| action == name)
            .unwrap_or_else(|| panic!("missing next action {name}: {actions:?}"))
    };
    assert!(index("semantic_provider_install") < index("semantic_provider_refresh"));
    assert!(index("semantic_provider_refresh") < index("semantic_navigation"));
    assert!(index("semantic_navigation") < index("apply_edits"));
    assert!(value["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "lsp_install_required"));
}

#[test]
fn rename_task_routes_through_semantic_navigation_before_guarded_edit() {
    let mut value = json!({
        "query": "rename target to execute across the project",
        "targets": [{"id":"target","kind":"function","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"target",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"fn target() {}","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"syntax","truncated":false},
        "semantic_provider_hints": [],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    let semantic = actions
        .iter()
        .position(|action| action == "semantic_navigation")
        .expect("rename must request semantic navigation");
    let edit = actions
        .iter()
        .position(|action| action == "apply_file_edits")
        .expect("rename must use the SHA-pinned multi-file edit surface");
    assert!(semantic < edit, "{actions:?}");
    assert_eq!(
        value["readiness"]["semantic_navigation_intent"],
        "rename_plan"
    );
    assert_eq!(
        value["readiness"]["recommended_edit_tool"],
        "apply_file_edits"
    );
    assert!(value["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "semantic_rename_plan_required"));
}

#[test]
fn rename_with_missing_lsp_routes_install_refresh_plan_and_guarded_apply_in_order() {
    let mut value = json!({
        "query": "rename target to execute across the project",
        "targets": [{"id":"target","kind":"function","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"target",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"fn target() {}","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"syntax","truncated":false},
        "semantic_provider_hints": [{
            "language":"rust",
            "provider":null,
            "action":"install_lsp",
            "install":{"provider":"rust-analyzer","model_can_install":true}
        }],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    let index = |name: &str| {
        actions
            .iter()
            .position(|action| action == name)
            .unwrap_or_else(|| panic!("missing next action {name}: {actions:?}"))
    };
    assert!(index("semantic_provider_install") < index("semantic_provider_refresh"));
    assert!(index("semantic_provider_refresh") < index("semantic_navigation"));
    assert!(index("semantic_navigation") < index("apply_file_edits"));
    assert!(index("apply_file_edits") < index("review_changes"));
    assert!(index("review_changes") < index("verify_project"));
    assert_eq!(
        value["readiness"]["semantic_navigation_intent"],
        "rename_plan"
    );
    assert_eq!(
        value["readiness"]["recommended_edit_tool"],
        "apply_file_edits"
    );
}

#[test]
fn organize_imports_requires_live_plan_even_with_semantic_graph_precision() {
    let mut value = json!({
        "query": "organize imports in the target file",
        "targets": [{"id":"src/a.rs","kind":"file","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"src/a.rs",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"use z::Z;\nuse a::A;\n","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"semantic","truncated":false},
        "semantic_provider_hints": [{
            "language":"rust",
            "provider":null,
            "action":"install_lsp",
            "install":{"provider":"rust-analyzer","model_can_install":true}
        }],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    let index = |name: &str| {
        actions
            .iter()
            .position(|action| action == name)
            .unwrap_or_else(|| panic!("missing next action {name}: {actions:?}"))
    };
    assert!(index("semantic_provider_install") < index("semantic_provider_refresh"));
    assert!(index("semantic_provider_refresh") < index("semantic_navigation"));
    assert!(index("semantic_navigation") < index("apply_file_edits"));
    assert_eq!(
        value["readiness"]["semantic_navigation_intent"],
        "organize_imports_plan"
    );
    assert_eq!(
        value["readiness"]["recommended_edit_tool"],
        "apply_file_edits"
    );
    assert!(value["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "semantic_organize_imports_plan_required"));
}

#[test]
fn quick_fix_task_routes_current_diagnostic_through_live_lsp_before_guarded_edit() {
    let mut value = json!({
        "query": "apply the quick fix for src/a.rs:1:1 diagnostic",
        "targets": [{"id":"src/a.rs","kind":"file","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"src/a.rs",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"let value = broken();","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"semantic","truncated":false},
        "semantic_provider_hints": [{
            "language":"rust",
            "provider":null,
            "action":"install_lsp",
            "install":{"provider":"rust-analyzer","model_can_install":true}
        }],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    let index = |name: &str| {
        actions
            .iter()
            .position(|action| action == name)
            .unwrap_or_else(|| panic!("missing next action {name}: {actions:?}"))
    };
    assert!(index("semantic_provider_install") < index("semantic_provider_refresh"));
    assert!(index("semantic_provider_refresh") < index("semantic_navigation"));
    assert!(index("semantic_navigation") < index("apply_file_edits"));
    assert_eq!(
        value["readiness"]["semantic_navigation_intent"],
        "quick_fix_plan"
    );
    assert_eq!(
        value["readiness"]["recommended_edit_tool"],
        "apply_file_edits"
    );
    assert!(value["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "semantic_quick_fix_plan_required"));
}

#[test]
fn initialized_lsp_does_not_recommend_installation() {
    let mut value = json!({
        "query": "find callers and references for target",
        "targets": [{"id":"target","kind":"function","path":"src/a.rs"}],
        "files": [{"path":"src/a.rs","sha256":"sha256:a","readonly":false}],
        "hot_source": [{
            "id":"target",
            "path":"src/a.rs",
            "sha256":"sha256:a",
            "body":{"content":"fn target() {}","redacted":false}
        }],
        "project": {"write_enabled":true},
        "tests": [{"resolved":true}],
        "conventions": {"errors":0,"truncated":false},
        "repo_map": {"precision":"syntax","truncated":false},
        "semantic_provider_hints": [],
        "readiness": {"parallelism":{"max_parallel":1}},
        "scopes": []
    });
    update_agent_readiness(&mut value);
    let actions = value["readiness"]["next_actions"].as_array().unwrap();
    assert!(!actions
        .iter()
        .any(|action| action == "semantic_provider_install"));
    assert!(actions.iter().any(|action| action == "semantic_navigation"));
}
