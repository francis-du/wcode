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
