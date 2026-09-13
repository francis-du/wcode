use super::*;
use crate::semantic::{
    SemanticCandidateInput, SemanticConfidence, SemanticFact, SemanticKind, SemanticOrigin,
    SemanticStatus,
};

#[test]
fn semantic_store_keeps_latest_revision_per_fact() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let candidate = SemanticFact::candidate(
        "SEM-GMV".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Metric,
            canonical: "gross merchandise value".into(),
            aliases: vec!["GMV".into()],
            description: "Transaction value before deductions.".into(),
            scopes: vec!["analytics".into()],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::Conversation,
            provider: Some("model-a".into()),
            confidence: SemanticConfidence::Medium,
            source: None,
        },
    )
    .unwrap();
    persist(&workspace, &candidate).unwrap();
    let confirmed = candidate.confirm("human:fixture".into()).unwrap();
    assert!(confirmed.timestamp_ms > candidate.timestamp_ms);
    persist(&workspace, &confirmed).unwrap();
    let loaded = load(&workspace).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].status, SemanticStatus::Confirmed);
}

#[test]
fn semantic_load_one_stops_at_the_latest_matching_record() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    for index in 0..16 {
        let mut fact = SemanticFact::candidate(
            format!("SEM-{index}"),
            SemanticCandidateInput {
                kind: SemanticKind::Metric,
                canonical: format!("metric {index}"),
                aliases: vec![],
                description: "fixture".into(),
                scopes: vec![],
                subject: None,
                predicate: None,
                object: None,
                origin: SemanticOrigin::Conversation,
                provider: None,
                confidence: SemanticConfidence::Medium,
                source: None,
            },
        )
        .unwrap();
        fact.timestamp_ms = 1_000 + index;
        persist(&workspace, &fact).unwrap();
    }
    let mut target = SemanticFact::candidate(
        "SEM-TARGET".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Metric,
            canonical: "target metric".into(),
            aliases: vec![],
            description: "fixture".into(),
            scopes: vec![],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::Conversation,
            provider: None,
            confidence: SemanticConfidence::Medium,
            source: None,
        },
    )
    .unwrap();
    target.timestamp_ms = 2_000;
    persist(&workspace, &target).unwrap();

    READ_FACT_CALLS.with(|count| count.set(0));
    assert_eq!(load_one(&workspace, "SEM-TARGET").unwrap(), Some(target));
    assert_eq!(
        READ_FACT_CALLS.with(|count| count.get()),
        1,
        "single-fact lookup should stop after the newest matching record"
    );
}

#[cfg(unix)]
#[test]
fn semantic_load_one_rejects_a_symlinked_store_directory() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let target = tempfile::tempdir().unwrap();
    let store = semantic_directory(&workspace).unwrap();
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    symlink(target.path(), &store).unwrap();

    assert!(load_one(&workspace, "SEM-TARGET").is_err());
}
