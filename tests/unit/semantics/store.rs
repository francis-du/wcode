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
