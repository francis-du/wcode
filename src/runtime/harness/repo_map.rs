use super::harness_retrieval::{
    classify_repo_map_intent, design_boost as retrieval_design_boost,
    direct_seed_boost as retrieval_direct_seed_boost,
    exact_query_match as repo_map_exact_query_match,
    experience_boost as retrieval_experience_boost, query_needs_semantic_relationships,
    relationship_boost as retrieval_relationship_boost, retain_repo_candidates_with_task_evidence,
    routing_value as retrieval_routing_value, select_repo_candidates,
    test_boost as retrieval_test_boost, test_path as repo_map_test_path, RepoMapCandidate,
    RepoMapIntent,
};
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::Ordering;

const REPO_MAP_MAX_ITEMS: usize = 16;
const REPO_MAP_ITERATIONS: usize = 6;
const REPO_MAP_RESTART: f64 = 0.28;

impl ToolHarness {
    pub(super) fn ranked_repo_map(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        query: &str,
        context: &SoftwareContext,
        max_items: usize,
    ) -> Result<Value> {
        let max_items = max_items.clamp(1, REPO_MAP_MAX_ITEMS);
        let started = Instant::now();
        let routing = classify_repo_map_intent(query);
        let scope_path = if routing.reason == "no_specific_retrieval_signal"
            && !query_needs_semantic_relationships(query)
        {
            repo_map_scope_path(context)
        } else {
            ".".to_owned()
        };
        let (graph, cache_hit) = self.repo_map_graph(workspace_id, workspace, &scope_path)?;
        let graph = harness_retrieval::augment_relationship_graph(
            self,
            workspace_id,
            workspace,
            query,
            context,
            graph.as_ref(),
        )?;
        let query_tokens = repo_map_tokens(query);
        let direct_ids = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("id").and_then(Value::as_str))
            .map(|id| format!("symbol:{id}"))
            .collect::<HashSet<_>>();
        let given_context_paths = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("path").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let design_paths = context
            .coverage
            .requirements
            .iter()
            .flat_map(|requirement| requirement.implementation.iter())
            .filter_map(|reference| repo_map_target_path(&reference.target))
            .collect::<HashSet<_>>();
        let experience_anchors = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("path").and_then(Value::as_str))
            .map(str::to_owned)
            .chain(design_paths.iter().cloned())
            .collect::<BTreeSet<_>>();
        let experience_started = Instant::now();
        let experience = crate::experience_store::related_paths_for_intent(
            workspace,
            &experience_anchors,
            Some(routing.name()),
            REPO_MAP_MAX_ITEMS.saturating_mul(2),
        )
        .unwrap_or_else(|_| crate::experience_store::ExperienceMatches::unavailable());
        let experience_lookup_ms = experience_started.elapsed().as_millis();

        let mut candidates = graph
            .graph
            .nodes
            .values()
            .filter(|node| repo_map_symbol_node(node.kind))
            .filter_map(|node| {
                let path = node
                    .attributes
                    .get("path")
                    .and_then(Value::as_str)?
                    .to_owned();
                let qualified_name = node
                    .attributes
                    .get("qualified_name")
                    .and_then(Value::as_str)
                    .unwrap_or(node.label.as_str())
                    .to_owned();
                let name = node
                    .attributes
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(qualified_name.as_str())
                    .to_owned();
                let kind = node
                    .attributes
                    .get("symbol_kind")
                    .and_then(Value::as_str)
                    .unwrap_or("symbol")
                    .to_owned();
                let mut haystack = qualified_name.to_ascii_lowercase();
                haystack.push(' ');
                haystack.push_str(&path.to_ascii_lowercase());
                let token_hits = query_tokens
                    .iter()
                    .filter(|token| haystack.contains(token.as_str()))
                    .count();
                let direct = direct_ids.contains(&node.id);
                let design_path = design_paths.contains(&path);
                let test_path = repo_map_test_path(&path, &kind);
                let experience_weight = experience.weights.get(&path).copied().unwrap_or_default();
                let exact_direct =
                    direct && repo_map_exact_query_match(&name, &qualified_name, &query_tokens);
                // Recall-oriented seeds stay below exact task targets.
                let relevance = if exact_direct {
                    140.0
                } else if direct {
                    100.0
                        + retrieval_direct_seed_boost(
                            routing.intent,
                            design_path,
                            test_path,
                            experience_weight,
                        )
                } else {
                    (token_hits as f64 * 18.0)
                        + retrieval_design_boost(routing.intent, design_path)
                        + retrieval_test_boost(routing.intent, test_path)
                        + retrieval_experience_boost(routing.intent, experience_weight)
                };
                Some(RepoMapCandidate {
                    id: node.id.clone(),
                    path,
                    name,
                    qualified_name,
                    kind,
                    relevance,
                    direct,
                    exact_direct,
                    design_path,
                    query_hits: token_hits,
                    experience_weight,
                    degree: 0,
                    rank: 0.0,
                })
            })
            .collect::<Vec<_>>();
        let precision_targets = repo_map_precision_targets(context, &candidates, &query_tokens);
        if candidates.is_empty() {
            return Ok(json!({
                "provider": "tree-sitter",
                "precision": "syntax",
                "items": [],
                "candidates": 0,
                "scope_path": scope_path,
                "routing": retrieval_routing_value(routing),
                "files_indexed": graph.files_indexed,
                "cache_hit": cache_hit,
                "build_ms": started.elapsed().as_millis(),
                "experience": {
                    "available": experience.available,
                    "provider": "verified-change-history",
                    "precision": "heuristic",
                    "model": crate::experience_store::retrieval_model(),
                    "evidence": "deterministic-verification",
                    "matched_records": experience.matched_records,
                    "scanned_records": experience.scanned_records,
                    "boosted_paths": experience.weights.len(),
                    "intent_conditioned": true,
                    "activation": {
                        "policy": crate::experience_store::activation_policy(),
                        "state": experience.activation.state,
                        "reason": experience.activation.reason,
                        "factor": experience.activation.factor,
                        "evaluable_records": experience.activation.evaluable_records,
                    },
                    "truncated": experience.truncated,
                    "lookup_ms": experience_lookup_ms,
                },
                "truncated": graph.truncated || graph.scan_truncated,
            }));
        }

        let index_by_id = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.id.clone(), index))
            .collect::<HashMap<_, _>>();
        let mut index_by_path = HashMap::<String, ProviderCandidatePathIndex>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            let path_index = index_by_path.entry(candidate.path.clone()).or_default();
            path_index
                .qualified
                .entry(candidate.qualified_name.clone())
                .or_default()
                .push(index);
            path_index
                .names
                .entry(candidate.name.clone())
                .or_default()
                .push(index);
        }

        let mut neighbors = vec![Vec::<usize>::new(); candidates.len()];
        let mut direct_relations = HashMap::<String, Vec<Value>>::new();
        for edge in &graph.graph.edges {
            if !repo_map_edge(edge.kind) {
                continue;
            }
            let (Some(&from), Some(&to)) = (index_by_id.get(&edge.from), index_by_id.get(&edge.to))
            else {
                continue;
            };
            if from == to {
                continue;
            }
            neighbors[from].push(to);
            neighbors[to].push(from);
            if precision_targets.contains(&edge.from) && !precision_targets.contains(&edge.to) {
                direct_relations
                    .entry(edge.to.clone())
                    .or_default()
                    .push(repo_map_relation(
                        edge.kind,
                        true,
                        &edge.from,
                        &edge.provenance.provider,
                        edge.provenance.precision,
                    ));
            }
            if precision_targets.contains(&edge.to) && !precision_targets.contains(&edge.from) {
                direct_relations
                    .entry(edge.from.clone())
                    .or_default()
                    .push(repo_map_relation(
                        edge.kind,
                        false,
                        &edge.to,
                        &edge.provenance.provider,
                        edge.provenance.precision,
                    ));
            }
        }
        let mut provider_edges_mapped = 0usize;
        let mut provider_nodes_mapped = 0usize;
        let mut providers_used = BTreeSet::<String>::new();
        for stored in graph_provider_store::load_latest(workspace)? {
            if graph_provider_store::freshness(workspace, &stored.import)
                != graph_provider_store::GraphProviderFreshness::Fresh
                || !matches!(
                    stored.import.precision,
                    GraphPrecision::Semantic
                        | GraphPrecision::Runtime
                        | GraphPrecision::Deterministic
                )
            {
                continue;
            }
            let provider_indices = stored
                .import
                .nodes
                .iter()
                .filter_map(|node| {
                    provider_candidate_index(node, &index_by_id, &index_by_path)
                        .map(|index| (node.id.clone(), index))
                })
                .collect::<HashMap<_, _>>();
            provider_nodes_mapped = provider_nodes_mapped.saturating_add(provider_indices.len());
            let mut provider_used = false;
            for edge in &stored.import.edges {
                if !repo_map_edge(edge.kind) {
                    continue;
                }
                let (Some(&from), Some(&to)) = (
                    provider_indices.get(&edge.from),
                    provider_indices.get(&edge.to),
                ) else {
                    continue;
                };
                if from == to {
                    continue;
                }
                provider_used = true;
                provider_edges_mapped = provider_edges_mapped.saturating_add(1);
                neighbors[from].push(to);
                neighbors[to].push(from);
                let boost = provider_precision_boost(stored.import.precision);
                if precision_targets.contains(&candidates[from].id)
                    && !precision_targets.contains(&candidates[to].id)
                {
                    candidates[to].relevance += boost;
                    direct_relations
                        .entry(candidates[to].id.clone())
                        .or_default()
                        .push(repo_map_relation(
                            edge.kind,
                            true,
                            &candidates[from].id,
                            &stored.import.provider,
                            stored.import.precision,
                        ));
                }
                if precision_targets.contains(&candidates[to].id)
                    && !precision_targets.contains(&candidates[from].id)
                {
                    candidates[from].relevance += boost;
                    direct_relations
                        .entry(candidates[from].id.clone())
                        .or_default()
                        .push(repo_map_relation(
                            edge.kind,
                            false,
                            &candidates[to].id,
                            &stored.import.provider,
                            stored.import.precision,
                        ));
                }
            }
            if provider_used {
                providers_used.insert(stored.import.provider.clone());
            }
        }
        for relations in direct_relations.values_mut() {
            relations.sort_by(|left, right| {
                relation_precision_rank(right).cmp(&relation_precision_rank(left))
            });
        }
        for candidate in &mut candidates {
            if let Some(relations) = direct_relations.get(&candidate.id) {
                candidate.relevance += retrieval_relationship_boost(routing.intent, relations);
            }
        }
        let covered_precision = repo_map_covered_precision(&precision_targets, &direct_relations);
        for (candidate, connected) in candidates.iter_mut().zip(&mut neighbors) {
            connected.sort_unstable();
            connected.dedup();
            candidate.degree = connected.len();
        }

        let rank_cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let personalization = repo_map_personalization(&candidates);
        let mut rank = personalization.clone();
        let mut next = vec![0.0; personalization.len()];
        let propagation = 1.0 - REPO_MAP_RESTART;
        for _ in 0..REPO_MAP_ITERATIONS {
            for (next, base) in next.iter_mut().zip(&personalization) {
                *next = *base * REPO_MAP_RESTART;
            }
            for (from, connected) in neighbors.iter().enumerate() {
                if connected.is_empty() {
                    next[from] += rank[from] * propagation;
                    continue;
                }
                let share = rank[from] * propagation / connected.len() as f64;
                for &to in connected {
                    next[to] += share;
                }
            }
            normalize_scores(&mut next);
            std::mem::swap(&mut rank, &mut next);
        }
        for (candidate, score) in candidates.iter_mut().zip(rank) {
            let centrality = (candidate.degree as f64 + 1.0).ln();
            candidate.rank = score * 1_000.0 + candidate.relevance * 1.6 + centrality * 8.0;
        }
        retain_repo_candidates_with_task_evidence(&mut candidates, &neighbors);
        if matches!(
            routing.intent,
            RepoMapIntent::CommentToContext | RepoMapIntent::FailureTraceToCode
        ) {
            candidates.retain(|candidate| !given_context_paths.contains(&candidate.path));
        }
        let eligible_candidates = candidates.len();
        select_repo_candidates(&mut candidates, max_items);
        drop(rank_cpu);

        let metadata = crate::resource::parallel_io(&candidates, |candidate| {
            self.code_index
                .symbol_metadata(workspace, &candidate.id)
                .ok()
                .map(|metadata| (candidate.id.clone(), metadata))
        })?
        .into_iter()
        .flatten()
        .collect::<HashMap<_, _>>();
        let items = candidates
            .iter()
            .map(|candidate| {
                repo_map_item(
                    candidate,
                    metadata.get(&candidate.id),
                    direct_relations
                        .get(&candidate.id)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "provider": if providers_used.is_empty() { "tree-sitter" } else { "wcode-composite" },
            "precision": covered_precision,
            "providers_used": providers_used,
            "provider_nodes_mapped": provider_nodes_mapped,
            "provider_edges_mapped": provider_edges_mapped,
            "items": items,
            "scope_path": scope_path,
            "routing": retrieval_routing_value(routing),
            "candidates": index_by_id.len(),
            "eligible_candidates": eligible_candidates,
            "filtered_candidates": index_by_id.len().saturating_sub(eligible_candidates),
            "files_indexed": graph.files_indexed,
            "graph_edges": graph.edge_count,
            "cache_hit": cache_hit,
            "build_ms": started.elapsed().as_millis(),
            "experience": {
                "available": experience.available,
                "provider": "verified-change-history",
                "precision": "heuristic",
                "model": crate::experience_store::retrieval_model(),
                "evidence": "deterministic-verification",
                "matched_records": experience.matched_records,
                "scanned_records": experience.scanned_records,
                "boosted_paths": experience.weights.len(),
                "intent_conditioned": true,
                "activation": {
                    "policy": crate::experience_store::activation_policy(),
                    "state": experience.activation.state,
                    "reason": experience.activation.reason,
                    "factor": experience.activation.factor,
                    "evaluable_records": experience.activation.evaluable_records,
                },
                "truncated": experience.truncated,
                "lookup_ms": experience_lookup_ms,
            },
            "scan_truncated": graph.scan_truncated,
            "graph_truncated": graph.truncated,
            "truncated": graph.truncated || graph.scan_truncated || eligible_candidates > max_items,
        }))
    }

    pub(super) fn repo_map_graph(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        path: &str,
    ) -> Result<(Arc<SoftwareGraphSnapshot>, bool)> {
        let cache_key = (workspace.root().to_path_buf(), path.to_owned());
        let flight = self.repo_map_flight(&cache_key)?;
        let observed_generation = flight.generation.load(Ordering::Acquire);
        let participant =
            harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
        #[cfg(test)]
        flight.entrants.fetch_add(1, Ordering::AcqRel);
        let _flight = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Even generations are completed validation epochs; odd generations
        // mean a validation is already in progress. Only a caller that observed
        // the previous completed epoch may reuse the owner's result, because
        // that owner's scan necessarily began after this caller arrived. A
        // caller that observed an odd generation started after the scan began
        // and must validate again, preserving external-edit freshness.
        if flight.can_reuse_after(observed_generation) {
            let mut cache = self
                .repo_map_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("repo map cache poisoned"))?;
            if let Some(cached) = cache.get_mut(&cache_key) {
                cached.last_used = Instant::now();
                return Ok((cached.snapshot.clone(), true));
            }
        }

        let mut validation = harness_cache_flight::ValidationGuard::begin(&flight);
        let (fingerprint, paths, scan_truncated) = repo_map_fingerprint(workspace, path)?;
        {
            let mut cache = self
                .repo_map_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("repo map cache poisoned"))?;
            if let Some(cached) = cache
                .get_mut(&cache_key)
                .filter(|cached| cached.fingerprint == fingerprint)
            {
                let snapshot = cached.snapshot.clone();
                cached.last_used = Instant::now();
                drop(cache);
                ensure_shared_repo_map_fingerprint_current(
                    &participant,
                    workspace,
                    path,
                    fingerprint,
                )?;
                validation.mark_success();
                return Ok((snapshot, true));
            }
        }

        // Reuse the exact source enumeration that established the cache key.
        // The index still validates each file's metadata around parsing, while
        // the next request recomputes this fingerprint and invalidates on any
        // added, removed, or modified source. A second full-tree scan after the
        // build only duplicated work without making the returned snapshot newer.
        let mut snapshot = self.code_index.software_graph_from_paths(
            workspace,
            paths,
            scan_truncated,
            REPO_MAP_MAX_SYMBOLS,
            &HashSet::new(),
            &HashSet::new(),
        )?;
        snapshot.workspace = workspace_id.to_owned();
        snapshot.path = path.to_owned();
        let snapshot = Arc::new(snapshot);
        if !validation.is_current() {
            bail!("repo map invalidated while building; retry the request");
        }
        ensure_shared_repo_map_fingerprint_current(&participant, workspace, path, fingerprint)?;
        let mut cache = self
            .repo_map_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("repo map cache poisoned"))?;
        if !validation.is_current() {
            bail!("repo map invalidated while building; retry the request");
        }
        let limit = crate::resource::limits().repo_map_cache_limit();
        if cache.len() >= limit {
            if let Some(oldest) = cache
                .iter()
                .min_by(|(_, left), (_, right)| left.last_used.cmp(&right.last_used))
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            cache_key,
            CachedRepoMapGraph {
                fingerprint,
                last_used: Instant::now(),
                snapshot: snapshot.clone(),
            },
        );
        validation.mark_success();
        Ok((snapshot, false))
    }
}

fn repo_map_scope_path(context: &SoftwareContext) -> String {
    let direct_paths = context
        .symbols
        .iter()
        .filter_map(|symbol| symbol.get("path").and_then(Value::as_str))
        .collect::<Vec<_>>();
    // Local lookup stays narrow; relationship queries select a broader graph
    // before reaching here. Broad Design ownership must not widen localization.
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
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    repo_map_common_scope(&source_paths)
}

pub(super) fn repo_map_common_scope(source_paths: &[String]) -> String {
    let mut directories = source_paths
        .iter()
        .filter_map(|path| repo_map_parent_path(path))
        .collect::<Vec<_>>();
    directories.sort();
    directories.dedup();
    let Some(first) = directories.first() else {
        return ".".to_owned();
    };
    let mut common = first.split('/').collect::<Vec<_>>();
    for directory in directories.iter().skip(1) {
        let matching = common
            .iter()
            .copied()
            .zip(directory.split('/'))
            .take_while(|(left, right)| left == right)
            .count();
        common.truncate(matching);
        if common.is_empty() {
            return ".".to_owned();
        }
    }
    let path = common.join("/");
    if path.is_empty() {
        ".".to_owned()
    } else {
        path
    }
}

fn repo_map_parent_path(path: &str) -> Option<&str> {
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

fn repo_map_fingerprint(workspace: &Workspace, path: &str) -> Result<(u64, Vec<String>, bool)> {
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

fn repo_map_symbol_node(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Symbol
            | NodeKind::Function
            | NodeKind::Struct
            | NodeKind::Trait
            | NodeKind::Class
            | NodeKind::Interface
            | NodeKind::Api
            | NodeKind::Test
    )
}

fn repo_map_edge(kind: EdgeKind) -> bool {
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

fn repo_map_tokens(query: &str) -> Vec<String> {
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

fn repo_map_precision_targets(
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

fn repo_map_target_path(target: &str) -> Option<String> {
    let candidate = target.split_once("::").map_or(target, |(path, _)| path);
    (candidate.contains('/') || candidate.contains('\\') || candidate.contains('.'))
        .then(|| candidate.to_owned())
}

fn repo_map_personalization(candidates: &[RepoMapCandidate]) -> Vec<f64> {
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

fn normalize_scores(values: &mut [f64]) {
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
struct ProviderCandidatePathIndex {
    qualified: HashMap<String, Vec<usize>>,
    names: HashMap<String, Vec<usize>>,
}

fn provider_candidate_index(
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

fn provider_precision_boost(precision: GraphPrecision) -> f64 {
    match precision {
        GraphPrecision::Runtime => 80.0,
        GraphPrecision::Deterministic => 70.0,
        GraphPrecision::Semantic => 60.0,
        _ => 0.0,
    }
}

fn graph_precision_name(precision: GraphPrecision) -> &'static str {
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

fn relation_precision_rank(value: &Value) -> u8 {
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

fn repo_map_relation(
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

fn repo_map_item(
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
