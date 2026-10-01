use super::*;

#[test]
fn git_provider_contract_rejects_short_injected_or_mixed_commit_identities() {
    let repository = ProviderRepository::new("francis-du", "wcode").unwrap();
    let mut target = ProviderTarget {
        repository,
        change: 7,
        base_sha: "a".repeat(40),
        head_sha: "b".repeat(40),
    };
    target.validate().unwrap();
    target.head_sha = "HEAD".into();
    assert!(target.validate().is_err());
    target.head_sha = "--upload-pack=command".into();
    assert!(target.validate().is_err());
    target.head_sha = "b".repeat(64);
    assert!(target.validate().is_err());
    target.base_sha = "a".repeat(64);
    target.validate().unwrap();
    target.change = 0;
    assert!(target.validate().is_err());
}

#[test]
fn git_provider_contract_missing_native_authority_can_only_deny() {
    let target = ProviderTarget {
        repository: ProviderRepository::new("francis-du", "wcode").unwrap(),
        change: 7,
        base_sha: "a".repeat(40),
        head_sha: "b".repeat(40),
    };
    let publication = NativePublication::unavailable(target).unwrap();
    assert_eq!(publication.verdict(), GateVerdict::Blocked);
    assert!(publication.record_digest().is_none());
}

#[test]
fn git_provider_contract_repository_identity_has_no_remote_credential_fields() {
    for namespace in [
        "",
        ".",
        "..",
        "owner/repo",
        "token@host",
        "https://github.com",
        "owner\n",
    ] {
        assert!(ProviderRepository::new(namespace, "repo").is_err());
    }
    let repository = ProviderRepository::new("FRANCIS-DU", "Wcode").unwrap();
    assert_eq!(repository.key(), "francis-du/wcode");
    let value = serde_json::to_value(repository).unwrap();
    assert!(value.get("remote_url").is_none());
    assert!(value.get("credential").is_none());
}
