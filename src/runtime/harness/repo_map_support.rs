use super::harness_retrieval::{self, RepoMapCandidate};
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

pub(super) fn repo_map_scope_path(context: &SoftwareContext, query: &str) -> String {
    let mut direct_paths = harness_retrieval::explicit_query_paths(context, query);
    if direct_paths.is_empty() {
        direct_paths = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("path").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
    }
    // Broad Design ownership is only a fallback when retrieval produced no
    // symbol path. Explicit code identities stay anchored to the task.
    let source_paths = if direct_paths.is_empty() {
        context
            .coverage
            .requirements
            .iter()
            .flat_map(|requirement| requirement.implementation.iter())
            .filter_map(|reference| repo_map_target_path(&reference.target))
            .collect::<Vec<_>>()
    } else {
        direct_paths
    };
    super::harness_repo_map::repo_map_common_scope(&source_paths)
}

pub(super) fn repo_map_parent_path(path: &str) -> Option<&str> {
    let path = path.trim_matches('/');
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .or(Some("."))
}

pub(super) fn ensure_shared_repo_map_fingerprint_current(
    participant: &harness_cache_flight::ValidationParticipant<'_>,
    workspace: &Workspace,
    path: &str,
    expected_fingerprint: u64,
) -> Result<()> {
    if participant.has_coalescible_peer() {
        let (confirmed_fingerprint, _, _) = repo_map_fingerprint(workspace, path)?;
        if confirmed_fingerprint != expected_fingerprint {
            bail!("repo map changed during shared validation; retry the request");
        }
    }
    Ok(())
}

pub(super) fn repo_map_fingerprint(
    workspace: &Workspace,
    path: &str,
) -> Result<(u64, Vec<String>, bool)> {
    let (entries, truncated) = workspace.source_files_with_stamps(path, REPO_MAP_MAX_FILES)?;
    let mut hasher = DefaultHasher::new();
    workspace.root().hash(&mut hasher);
    path.hash(&mut hasher);
    truncated.hash(&mut hasher);
    entries.len().hash(&mut hasher);
    let mut paths = Vec::with_capacity(entries.len());
    for (source_path, stamp) in entries {
        source_path.hash(&mut hasher);
        stamp.hash(&mut hasher);
        paths.push(source_path);
    }
    Ok((hasher.finish(), paths, truncated))
}

pub(super) fn repo_map_symbol_node(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Symbol
            | NodeKind::Function
            | NodeKind::Struct
            | NodeKind::Trait
            | NodeKind::Class
            | NodeKind::Enum
            | NodeKind::Interface
            | NodeKind::Api
            | NodeKind::Test
    )
}

pub(super) fn repo_map_edge(kind: EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls
            | EdgeKind::References
            | EdgeKind::Imports
            | EdgeKind::DependsOn
            | EdgeKind::Implements
            | EdgeKind::Extends
            | EdgeKind::RuntimeCalls
    )
}

pub(super) fn repo_map_tokens(query: &str) -> Vec<String> {
    let code_tokens = crate::intelligence::code_query_tokens(query);
    if !code_tokens.is_empty() {
        return code_tokens;
    }
    query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn repo_map_precision_targets(
    context: &SoftwareContext,
    candidates: &[RepoMapCandidate],
    query_tokens: &[String],
) -> BTreeSet<String> {
    let direct_ids = candidates
        .iter()
        .filter(|candidate| candidate.direct)
        .map(|candidate| candidate.id.as_str())
        .collect::<HashSet<_>>();
    let mut targets = context
        .coverage
        .requirements
        .iter()
        .flat_map(|requirement| requirement.implementation.iter())
        .filter(|reference| reference.resolved)
        .filter_map(|reference| reference.node_id.as_deref())
        .filter(|node_id| direct_ids.contains(*node_id))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    // Only exact lexical targets may back readiness precision without Design.
    if targets.is_empty() {
        let exact_tokens = query_tokens
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for candidate in candidates.iter().filter(|candidate| candidate.direct) {
            let name = candidate.name.to_ascii_lowercase();
            let qualified_name = candidate.qualified_name.to_ascii_lowercase();
            if exact_tokens.contains(name.as_str())
                || exact_tokens.contains(qualified_name.as_str())
            {
                targets.insert(candidate.id.clone());
            }
        }
    }
    targets
}

pub(super) fn repo_map_target_path(target: &str) -> Option<String> {
    let candidate = target.split_once("::").map_or(target, |(path, _)| path);
    (candidate.contains('/') || candidate.contains('\\') || candidate.contains('.'))
        .then(|| candidate.to_owned())
}

pub(super) fn repo_map_personalization(candidates: &[RepoMapCandidate]) -> Vec<f64> {
    let mut values = candidates
        .iter()
        .map(|candidate| {
            if candidate.relevance > 0.0 {
                candidate.relevance
            } else {
                0.15
            }
        })
        .collect::<Vec<_>>();
    normalize_scores(&mut values);
    values
}

pub(super) fn normalize_scores(values: &mut [f64]) {
    let total = values.iter().copied().sum::<f64>();
    if total > f64::EPSILON {
        for value in values {
            *value /= total;
        }
    } else if !values.is_empty() {
        let uniform = 1.0 / values.len() as f64;
        values.fill(uniform);
    }
}

#[derive(Default)]
pub(super) struct ProviderCandidatePathIndex {
    pub(super) qualified: HashMap<String, Vec<usize>>,
    pub(super) names: HashMap<String, Vec<usize>>,
}

pub(super) fn provider_candidate_index(
    node: &crate::graph::GraphImportNode,
    index_by_id: &HashMap<String, usize>,
    index_by_path: &HashMap<String, ProviderCandidatePathIndex>,
) -> Option<usize> {
    if let Some(index) = index_by_id.get(&node.id) {
        return Some(*index);
    }
    let path = node.attributes.get("path")?.as_str()?;
    let path_index = index_by_path.get(path)?;
    if let Some(qualified_name) = node
        .attributes
        .get("qualified_name")
        .and_then(Value::as_str)
    {
        if let Some(indices) = path_index.qualified.get(qualified_name) {
            if indices.len() == 1 {
                return indices.first().copied();
            }
        }
    }
    let name = node
        .attributes
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(node.label.as_str());
    path_index
        .names
        .get(name)
        .filter(|indices| indices.len() == 1)
        .and_then(|indices| indices.first().copied())
}

pub(super) fn provider_precision_boost(precision: GraphPrecision) -> f64 {
    match precision {
        GraphPrecision::Runtime => 80.0,
        GraphPrecision::Deterministic => 70.0,
        GraphPrecision::Semantic => 60.0,
        _ => 0.0,
    }
}

pub(super) fn graph_precision_name(precision: GraphPrecision) -> &'static str {
    match precision {
        GraphPrecision::Runtime => "runtime",
        GraphPrecision::Semantic => "semantic",
        GraphPrecision::Deterministic => "deterministic",
        GraphPrecision::Syntax => "syntax",
        GraphPrecision::Declared => "declared",
        GraphPrecision::Heuristic => "heuristic",
        GraphPrecision::Mixed => "mixed",
    }
}

pub(super) fn repo_map_covered_precision(
    precision_targets: &BTreeSet<String>,
    direct_relations: &HashMap<String, Vec<Value>>,
) -> &'static str {
    if precision_targets.is_empty() {
        return "syntax";
    }
    let mut coverage = precision_targets
        .iter()
        .map(|target| (target.as_str(), ("syntax", 3u8)))
        .collect::<HashMap<_, _>>();
    for relation in direct_relations.values().flatten() {
        let Some(direct) = relation.get("direct").and_then(Value::as_str) else {
            continue;
        };
        let Some(current) = coverage.get_mut(direct) else {
            continue;
        };
        let Some(precision) = relation.get("precision").and_then(Value::as_str) else {
            continue;
        };
        let rank = relation_precision_rank(relation);
        if rank > current.1 {
            current.0 = match precision {
                "runtime" => "runtime",
                "semantic" => "semantic",
                "deterministic" => "deterministic",
                "syntax" => "syntax",
                "declared" => "declared",
                "heuristic" => "heuristic",
                _ => current.0,
            };
            current.1 = rank;
        }
    }
    coverage
        .values()
        .min_by_key(|(_, rank)| *rank)
        .map(|(precision, _)| *precision)
        .unwrap_or("syntax")
}

pub(super) fn relation_precision_rank(value: &Value) -> u8 {
    value
        .get("precision")
        .and_then(Value::as_str)
        .map(|precision| match precision {
            "runtime" => 6,
            "semantic" => 5,
            "deterministic" => 4,
            "syntax" => 3,
            "declared" => 2,
            "heuristic" => 1,
            _ => 0,
        })
        .unwrap_or(0)
}

pub(super) fn repo_map_relation(
    kind: EdgeKind,
    direct_is_from: bool,
    direct_id: &str,
    provider: &str,
    precision: GraphPrecision,
) -> Value {
    let relation = match (kind, direct_is_from) {
        (EdgeKind::Calls | EdgeKind::RuntimeCalls, true) => "callee_of_direct",
        (EdgeKind::Calls | EdgeKind::RuntimeCalls, false) => "caller_of_direct",
        (EdgeKind::References, true) => "referenced_by_direct",
        (EdgeKind::References, false) => "references_direct",
        (EdgeKind::Imports, true) => "imported_by_direct",
        (EdgeKind::Imports, false) => "imports_direct",
        (EdgeKind::DependsOn, true) => "dependency_of_direct",
        (EdgeKind::DependsOn, false) => "depends_on_direct",
        (EdgeKind::Implements, true) => "implemented_by_direct",
        (EdgeKind::Implements, false) => "implements_direct",
        (EdgeKind::Extends, true) => "extended_by_direct",
        (EdgeKind::Extends, false) => "extends_direct",
        _ => "related_to_direct",
    };
    json!({
        "relation": relation,
        "kind": format!("{:?}", kind).to_ascii_lowercase(),
        "direct": direct_id,
        "provider": provider,
        "precision": graph_precision_name(precision),
    })
}

pub(super) fn repo_map_item(
    candidate: &RepoMapCandidate,
    metadata: Option<&Value>,
    direct_relations: &[Value],
) -> Value {
    let signature = metadata
        .and_then(|metadata| metadata.get("signature"))
        .cloned()
        .unwrap_or(Value::Null);
    let start_line = metadata
        .and_then(|metadata| metadata.pointer("/range/start_line"))
        .cloned()
        .unwrap_or(Value::Null);
    let end_line = metadata
        .and_then(|metadata| metadata.pointer("/range/end_line"))
        .cloned()
        .unwrap_or(Value::Null);
    let reason = if candidate.exact_direct {
        "direct_match"
    } else if candidate.test_target_match && direct_relations.is_empty() {
        "test_target_text_match"
    } else if candidate.direct {
        "retrieval_seed"
    } else if let Some(relation) = direct_relations
        .first()
        .and_then(|relation| relation.get("relation"))
        .and_then(Value::as_str)
    {
        relation
    } else if candidate.design_path && candidate.relevance > 0.0 {
        "design_and_query"
    } else if candidate.query_hits > 0 {
        "query_related"
    } else if candidate.experience_weight > 0 {
        "verified_experience"
    } else if candidate.degree > 0 {
        "graph_neighbor"
    } else {
        "central"
    };
    let mut item = json!({
        "id": candidate.id,
        "path": candidate.path,
        "qualified_name": candidate.qualified_name,
        "kind": candidate.kind,
        "signature": signature,
        "start_line": start_line,
        "end_line": end_line,
        "reason": reason,
        "relationships": direct_relations.iter().take(3).cloned().collect::<Vec<_>>(),
        "degree": candidate.degree,
        "score": (candidate.rank * 100.0).round() / 100.0,
    });
    if candidate.experience_weight > 0 {
        item["historical_relevance"] = json!({
            "provider": "verified-change-history",
            "precision": "heuristic",
            "model": crate::experience_store::retrieval_model(),
            "evidence": "deterministic-verification",
            "weight": candidate.experience_weight,
        });
    }
    item
}
