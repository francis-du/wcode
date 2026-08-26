use crate::scopes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ALIASES: usize = 32;
const MAX_SCOPES: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    Concept,
    Alias,
    Entity,
    Metric,
    Dimension,
    Relationship,
    Rule,
    DomainTerm,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOrigin {
    User,
    Conversation,
    Design,
    Provider,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStatus {
    Candidate,
    Confirmed,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticFact {
    pub schema_version: u32,
    pub id: String,
    pub kind: SemanticKind,
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    pub origin: SemanticOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub confidence: SemanticConfidence,
    pub status: SemanticStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attested_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub timestamp_ms: u64,
}

impl SemanticFact {
    pub fn candidate(id: String, input: SemanticCandidateInput) -> Result<Self, SemanticError> {
        let mut fact = Self {
            schema_version: 1,
            id,
            kind: input.kind,
            canonical: input.canonical,
            aliases: input.aliases,
            description: input.description,
            scopes: input.scopes,
            subject: input.subject,
            predicate: input.predicate,
            object: input.object,
            origin: input.origin,
            provider: input.provider,
            confidence: input.confidence,
            status: SemanticStatus::Candidate,
            attested_by: None,
            source: input.source,
            timestamp_ms: now_ms(),
        };
        fact.normalize();
        fact.validate()?;
        Ok(fact)
    }

    pub fn confirm(&self, attested_by: String) -> Result<Self, SemanticError> {
        let mut fact = self.clone();
        fact.status = SemanticStatus::Confirmed;
        fact.attested_by = Some(attested_by);
        fact.timestamp_ms = next_revision_timestamp(self.timestamp_ms);
        fact.normalize();
        fact.validate()?;
        Ok(fact)
    }

    pub fn retire(&self, attested_by: String) -> Result<Self, SemanticError> {
        let mut fact = self.clone();
        fact.status = SemanticStatus::Retired;
        fact.attested_by = Some(attested_by);
        fact.timestamp_ms = next_revision_timestamp(self.timestamp_ms);
        fact.normalize();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), SemanticError> {
        if self.schema_version != 1
            || !bounded_text(&self.id, 160, false)
            || !bounded_text(&self.canonical, 300, false)
            || !bounded_text(&self.description, 2_000, false)
            || self.aliases.len() > MAX_ALIASES
            || self.scopes.len() > MAX_SCOPES
            || self
                .aliases
                .iter()
                .any(|value| !bounded_text(value, 300, false))
            || self
                .scopes
                .iter()
                .any(|value| !bounded_text(value, 300, false))
            || self
                .subject
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 512, false))
            || self
                .predicate
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 256, false))
            || self
                .object
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 512, false))
            || self
                .provider
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 256, false))
            || self
                .attested_by
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 256, false))
            || self
                .source
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 1_000, true))
        {
            return Err(SemanticError::InvalidFact);
        }
        if self.kind == SemanticKind::Relationship
            && (self.subject.is_none() || self.predicate.is_none() || self.object.is_none())
        {
            return Err(SemanticError::InvalidRelationship);
        }
        if self.status == SemanticStatus::Confirmed && self.attested_by.is_none() {
            return Err(SemanticError::MissingAttestation);
        }
        Ok(())
    }

    pub fn searchable_text(&self) -> String {
        let mut values = vec![self.canonical.clone(), self.description.clone()];
        values.extend(self.aliases.clone());
        values.extend(self.scopes.clone());
        values.extend(self.subject.clone());
        values.extend(self.predicate.clone());
        values.extend(self.object.clone());
        values.join(" ")
    }

    pub fn expansion_terms(&self) -> Vec<String> {
        let mut terms = BTreeSet::new();
        terms.insert(self.canonical.clone());
        terms.extend(self.aliases.iter().cloned());
        terms.into_iter().collect()
    }

    fn normalize(&mut self) {
        self.id = self.id.trim().to_owned();
        self.canonical = self.canonical.trim().to_owned();
        self.description = self.description.trim().to_owned();
        self.aliases = normalize_list(std::mem::take(&mut self.aliases));
        self.scopes = scopes::canonicalize(&std::mem::take(&mut self.scopes));
        normalize_option(&mut self.subject);
        normalize_option(&mut self.predicate);
        normalize_option(&mut self.object);
        normalize_option(&mut self.provider);
        normalize_option(&mut self.attested_by);
        normalize_option(&mut self.source);
    }
}

fn next_revision_timestamp(previous: u64) -> u64 {
    now_ms().max(previous.saturating_add(1))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCandidateInput {
    pub kind: SemanticKind,
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub object: Option<String>,
    pub origin: SemanticOrigin,
    #[serde(default)]
    pub provider: Option<String>,
    pub confidence: SemanticConfidence,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticMatch {
    pub score: usize,
    pub fact: SemanticFact,
}

pub fn query(
    facts: impl IntoIterator<Item = SemanticFact>,
    query: &str,
    include_candidates: bool,
    limit: usize,
) -> Vec<SemanticMatch> {
    query_scoped(facts, query, &[], include_candidates, limit)
}

pub fn query_scoped(
    facts: impl IntoIterator<Item = SemanticFact>,
    query: &str,
    requested_scopes: &[String],
    include_candidates: bool,
    limit: usize,
) -> Vec<SemanticMatch> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let tokens = tokens(&needle);
    let requested_scopes = scopes::canonicalize(requested_scopes);
    let mut matches = facts
        .into_iter()
        .filter(|fact| {
            fact.status == SemanticStatus::Confirmed
                || (include_candidates && fact.status == SemanticStatus::Candidate)
        })
        .filter(|fact| {
            requested_scopes.is_empty()
                || fact.scopes.is_empty()
                || fact
                    .scopes
                    .iter()
                    .any(|scope| requested_scopes.binary_search(scope).is_ok())
        })
        .filter_map(|fact| {
            let haystack = fact.searchable_text().to_ascii_lowercase();
            let exact = usize::from(haystack.contains(&needle));
            let canonical = usize::from(fact.canonical.to_ascii_lowercase() == needle);
            let alias = usize::from(
                fact.aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(&needle)),
            );
            let token_hits = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            let confirmed = usize::from(fact.status == SemanticStatus::Confirmed);
            let scope_hits = fact
                .scopes
                .iter()
                .filter(|scope| requested_scopes.binary_search(scope).is_ok())
                .count();
            let score = canonical * 200
                + alias * 160
                + exact * 100
                + token_hits * 12
                + scope_hits * 30
                + confirmed * 5;
            (score > 0).then_some(SemanticMatch { score, fact })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.fact.id.cmp(&right.fact.id))
    });
    matches.truncate(limit.clamp(1, 100));
    matches
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticError {
    InvalidFact,
    InvalidRelationship,
    MissingAttestation,
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFact => "semantic fact metadata is invalid",
            Self::InvalidRelationship => {
                "relationship semantic facts require subject, predicate, and object"
            }
            Self::MissingAttestation => "confirmed semantic facts require an attestation identity",
        })
    }
}

impl std::error::Error for SemanticError {}

fn bounded_text(value: &str, maximum: usize, allow_newline: bool) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= maximum
        && !value
            .chars()
            .any(|character| character.is_control() && !(allow_newline && character == '\n'))
}

fn normalize_option(value: &mut Option<String>) {
    if let Some(current) = value {
        *current = current.trim().to_owned();
        if current.is_empty() {
            *value = None;
        }
    }
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn tokens(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
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
        assert!(matches.iter().all(|matched| {
            matched.fact.scopes.is_empty() || matched.fact.scopes == vec!["graph"]
        }));
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
}
