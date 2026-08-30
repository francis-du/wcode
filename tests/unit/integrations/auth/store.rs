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
