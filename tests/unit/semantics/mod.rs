use super::*;

#[test]
fn candidates_do_not_become_authoritative_without_confirmation() {
    let candidate = SemanticFact::candidate(
        "SEM-1".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Metric,
            canonical: "gross merchandise value".into(),
            aliases: vec!["GMV".into()],
            description: "Total transaction value before deductions.".into(),
            scopes: vec!["analytics".into()],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::Conversation,
            provider: Some("model-a".into()),
            confidence: SemanticConfidence::Medium,
            source: Some("conversation:fixture".into()),
        },
    )
    .unwrap();
    assert!(query([candidate.clone()], "GMV", false, 10).is_empty());
    assert_eq!(query([candidate.clone()], "GMV", true, 10).len(), 1);
    let confirmed = candidate.confirm("human:fixture".into()).unwrap();
    assert_eq!(query([confirmed], "GMV", false, 10).len(), 1);
}

#[test]
fn scoped_queries_filter_scoped_facts_but_keep_global_facts() {
    let graph = SemanticFact::candidate(
        "SEM-GRAPH".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Concept,
            canonical: "revision".into(),
            aliases: vec![],
            description: "Graph revision semantics.".into(),
            scopes: vec!["software graph".into()],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::User,
            provider: None,
            confidence: SemanticConfidence::High,
            source: None,
        },
    )
    .unwrap()
    .confirm("human:fixture".into())
    .unwrap();
    let verification = SemanticFact::candidate(
        "SEM-VERIFY".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Concept,
            canonical: "revision".into(),
            aliases: vec![],
            description: "Verification revision semantics.".into(),
            scopes: vec!["verification".into()],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::User,
            provider: None,
            confidence: SemanticConfidence::High,
            source: None,
        },
    )
    .unwrap()
    .confirm("human:fixture".into())
    .unwrap();
    let global = SemanticFact::candidate(
        "SEM-GLOBAL".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Concept,
            canonical: "revision".into(),
            aliases: vec![],
            description: "Global revision semantics.".into(),
            scopes: vec![],
            subject: None,
            predicate: None,
            object: None,
            origin: SemanticOrigin::User,
            provider: None,
            confidence: SemanticConfidence::High,
            source: None,
        },
    )
    .unwrap()
    .confirm("human:fixture".into())
    .unwrap();

    assert_eq!(graph.scopes, vec!["graph"]);
    let matches = query_scoped(
        [graph, verification, global],
        "revision",
        &["software graph".into()],
        false,
        10,
    );
    assert_eq!(matches.len(), 2);
    assert!(matches
        .iter()
        .all(|matched| { matched.fact.scopes.is_empty() || matched.fact.scopes == vec!["graph"] }));
}

#[test]
fn relationship_requires_a_complete_triple() {
    let result = SemanticFact::candidate(
        "SEM-REL".into(),
        SemanticCandidateInput {
            kind: SemanticKind::Relationship,
            canonical: "orders belong to users".into(),
            aliases: vec![],
            description: "Ownership relation.".into(),
            scopes: vec![],
            subject: Some("order".into()),
            predicate: Some("belongs_to".into()),
            object: None,
            origin: SemanticOrigin::User,
            provider: None,
            confidence: SemanticConfidence::High,
            source: None,
        },
    );
    assert_eq!(result.unwrap_err(), SemanticError::InvalidRelationship);
}
