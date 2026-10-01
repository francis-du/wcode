use wcode::verification::change::ExecutionGitBinding as WcodeExecutionGitBinding;
use wcode_core_types::ExecutionGitBinding;

#[test]
fn execution_git_binding_requires_exact_repository_and_git_identities() {
    let valid = ExecutionGitBinding {
        repository: format!("sha256:{}", "a".repeat(64)),
        head_sha: "b".repeat(40),
        tree_sha: "c".repeat(40),
        dirty: false,
        index_fingerprint: format!("sha256:{}", "d".repeat(64)),
    };
    assert!(valid.valid());
    let public = WcodeExecutionGitBinding {
        repository: valid.repository.clone(),
        head_sha: valid.head_sha.clone(),
        tree_sha: valid.tree_sha.clone(),
        dirty: valid.dirty,
        index_fingerprint: valid.index_fingerprint.clone(),
    };
    assert!(public.valid());
    assert_eq!(
        serde_json::to_value(&valid).unwrap(),
        serde_json::to_value(&public).unwrap()
    );

    for invalid in [
        ExecutionGitBinding {
            repository: "owner/repo".into(),
            ..valid.clone()
        },
        ExecutionGitBinding {
            head_sha: "short".into(),
            ..valid.clone()
        },
        ExecutionGitBinding {
            tree_sha: "e".repeat(64),
            ..valid.clone()
        },
        ExecutionGitBinding {
            index_fingerprint: "sha256:xyz".into(),
            ..valid.clone()
        },
    ] {
        assert!(!invalid.valid(), "{invalid:?}");
    }
}
