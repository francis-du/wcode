use super::harness_repo_map_support::*;
use super::harness_retrieval::{
    classify_repo_map_intent, design_boost as retrieval_design_boost,
    direct_seed_boost as retrieval_direct_seed_boost,
    exact_query_match as repo_map_exact_query_match,
    experience_boost as retrieval_experience_boost,
    relationship_boost as retrieval_relationship_boost, retain_repo_candidates_with_task_evidence,
    retain_returnable_repo_candidates, routing_value as retrieval_routing_value,
    select_repo_candidates, test_boost as retrieval_test_boost, test_path as repo_map_test_path,
    RepoMapCandidate, RepoMapIntent,
};
use super::*;
use std::sync::atomic::Ordering;

pub(super) use super::harness_repo_map_support::{
    ensure_shared_repo_map_fingerprint_current, repo_map_covered_precision,
};

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
        // Start from the narrowest task-owned scope for every query.
        // Cross-file relationship tasks expand through targeted exact-search
        // supplementation instead of parsing the whole repository up front.
        let scope_path = repo_map_scope_path(context, query);
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
                let signature = node
                    .attributes
                    .get("signature")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mut haystack =
                    String::with_capacity(qualified_name.len() + path.len() + signature.len() + 8);
                crate::graph::push_ascii_lowercased(&mut haystack, &qualified_name);
                haystack.push(' ');
                crate::graph::push_ascii_lowercased(&mut haystack, &path);
                if !signature.is_empty() {
                    haystack.push(' ');
                    crate::graph::push_ascii_lowercased(&mut haystack, signature);
                }
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
                    test_target_match: node
                        .attributes
                        .get("test_target_match")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
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
        retain_repo_candidates_with_task_evidence(&mut candidates, &neighbors, routing.intent);
        retain_returnable_repo_candidates(&mut candidates);
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
        let _flight = flight.acquire()?;

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
