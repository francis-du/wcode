use super::*;

#[test]
fn workspace_scope_is_order_independent_and_isolated() {
    let first =
        AuthStore::for_workspaces(&[PathBuf::from("/code/one"), PathBuf::from("/code/two")])
            .unwrap();
    let reordered =
        AuthStore::for_workspaces(&[PathBuf::from("/code/two"), PathBuf::from("/code/one")])
            .unwrap();
    let other = AuthStore::for_workspaces(&[PathBuf::from("/code/other")]).unwrap();

    assert_eq!(first.path, reordered.path);
    assert_ne!(first.path, other.path);
    assert_eq!(
        first.path.extension().and_then(|value| value.to_str()),
        Some("json")
    );
    assert!(!first.path.to_string_lossy().contains("/code/one"));
}

#[test]
fn persisted_owner_ids_are_backward_compatible_and_malformed_values_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let client_id = "wcode-782eef5c-7845-483c-b5de-7a1864d9ff65";
    let mut state = serde_json::json!({
        "version": 1,
        "clients": {
            (client_id): {
                "redirect_uris": ["https://chatgpt.com/connector_platform_oauth_redirect"]
            }
        },
        "access_tokens": {
            "access_legacy": {
                "issued_at_ms": 1,
                "client_id": client_id,
                "resource": "https://example.com/mcp"
            }
        },
        "refresh_tokens": {}
    });
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let store = AuthStore::at_path(path.clone());
    let loaded = store.load().unwrap();
    assert!(loaded.access_tokens["access_legacy"].owner_id.is_empty());

    state["access_tokens"]["access_legacy"]["owner_id"] = serde_json::json!("forged-owner");
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(store.load().is_err());
}
