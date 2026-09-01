use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

const REPO_MAP_MAX_SYMBOLS: usize = 6_000;
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
        let scope_path = repo_map_scope_path(context);
        let (graph, cache_hit) = self.repo_map_graph(workspace_id, workspace, &scope_path)?;
        let query_tokens = repo_map_tokens(query);
        let direct_ids = context
            .symbols
            .iter()
            .filter_map(|symbol| symbol.get("id").and_then(Value::as_str))
            .map(|id| format!("symbol:{id}"))
            .collect::<HashSet<_>>();
        let design_paths = context
            .coverage
            .requirements
            .iter()
            .flat_map(|requirement| requirement.implementation.iter())
            .filter_map(|reference| repo_map_target_path(&reference.target))
            .collect::<HashSet<_>>();

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
                let relevance = if direct {
                    100.0
                } else {
                    (token_hits as f64 * 18.0) + if design_path { 35.0 } else { 0.0 }
                };
                Some(RepoMapCandidate {
                    id: node.id.clone(),
                    path,
                    name,
                    qualified_name,
                    kind,
                    relevance,
                    direct,
                    design_path,
                    degree: 0,
                    rank: 0.0,
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(json!({
                "provider": "tree-sitter",
                "precision": "syntax",
                "items": [],
                "candidates": 0,
                "scope_path": scope_path,
                "files_indexed": graph.files_indexed,
                "cache_hit": cache_hit,
                "build_ms": started.elapsed().as_millis(),
                "truncated": graph.truncated || graph.scan_truncated,
            }));
        }

        let index_by_id = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.id.clone(), index))
            .collect::<HashMap<_, _>>();
        let mut index_by_qualified = HashMap::<(String, String), Vec<usize>>::new();
        let mut index_by_name = HashMap::<(String, String), Vec<usize>>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            index_by_qualified
                .entry((candidate.path.clone(), candidate.qualified_name.clone()))
                .or_default()
                .push(index);
            index_by_name
                .entry((candidate.path.clone(), candidate.name.clone()))
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
            if direct_ids.contains(&edge.from) && !direct_ids.contains(&edge.to) {
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
            if direct_ids.contains(&edge.to) && !direct_ids.contains(&edge.from) {
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
        let mut strongest_precision = GraphPrecision::Syntax;
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
                    provider_candidate_index(
                        node,
                        &index_by_id,
                        &index_by_qualified,
                        &index_by_name,
                    )
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
                if candidates[from].direct && !candidates[to].direct {
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
                if candidates[to].direct && !candidates[from].direct {
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
                if graph_precision_rank(stored.import.precision)
                    > graph_precision_rank(strongest_precision)
                {
                    strongest_precision = stored.import.precision;
                }
            }
        }
        for relations in direct_relations.values_mut() {
            relations.sort_by(|left, right| {
                relation_precision_rank(right).cmp(&relation_precision_rank(left))
            });
        }
        for (candidate, connected) in candidates.iter_mut().zip(&mut neighbors) {
            connected.sort_unstable();
            connected.dedup();
            candidate.degree = connected.len();
        }

        let rank_cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let personalization = repo_map_personalization(&candidates);
        let mut rank = personalization.clone();
        for _ in 0..REPO_MAP_ITERATIONS {
            let mut next = personalization
                .iter()
                .map(|value| value * REPO_MAP_RESTART)
                .collect::<Vec<_>>();
            let propagation = 1.0 - REPO_MAP_RESTART;
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
            rank = next;
        }
        for (candidate, score) in candidates.iter_mut().zip(rank) {
            let centrality = (candidate.degree as f64 + 1.0).ln();
            candidate.rank = score * 1_000.0 + candidate.relevance * 1.6 + centrality * 8.0;
        }
        candidates.sort_by(|left, right| {
            right
                .rank
                .total_cmp(&left.rank)
                .then_with(|| right.direct.cmp(&left.direct))
                .then_with(|| left.qualified_name.cmp(&right.qualified_name))
                .then_with(|| left.path.cmp(&right.path))
        });
        candidates.truncate(max_items);
        drop(rank_cpu);

        let metadata = candidates
            .iter()
            .filter_map(|candidate| {
                self.code_index
                    .symbol_metadata(workspace, &candidate.id)
                    .ok()
                    .map(|metadata| (candidate.id.clone(), metadata))
            })
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
            "precision": graph_precision_name(strongest_precision),
            "providers_used": providers_used,
            "provider_nodes_mapped": provider_nodes_mapped,
            "provider_edges_mapped": provider_edges_mapped,
            "items": items,
            "scope_path": scope_path,
            "candidates": index_by_id.len(),
            "files_indexed": graph.files_indexed,
            "graph_edges": graph.edge_count,
            "cache_hit": cache_hit,
            "build_ms": started.elapsed().as_millis(),
            "scan_truncated": graph.scan_truncated,
            "graph_truncated": graph.truncated,
            "truncated": graph.truncated || graph.scan_truncated || index_by_id.len() > max_items,
        }))
    }

    fn repo_map_graph(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        path: &str,
    ) -> Result<(Arc<SoftwareGraphSnapshot>, bool)> {
        let cache_key = (workspace.root().to_path_buf(), path.to_owned());
        let fingerprint_before = repo_map_fingerprint(workspace, path)?;
        if let Some(snapshot) = self
            .repo_map_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("repo map cache poisoned"))?
            .get(&cache_key)
            .filter(|cached| cached.fingerprint == fingerprint_before)
            .map(|cached| cached.snapshot.clone())
        {
            return Ok((snapshot, true));
        }

        let snapshot = Arc::new(self.code_index.software_graph(
            workspace_id.to_owned(),
            workspace,
            path,
            REPO_MAP_MAX_FILES,
            REPO_MAP_MAX_SYMBOLS,
        )?);
        let fingerprint_after = repo_map_fingerprint(workspace, path)?;
        if fingerprint_before == fingerprint_after {
            let mut cache = self
                .repo_map_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("repo map cache poisoned"))?;
            let limit = crate::resource::limits().repo_map_cache_limit();
            if cache.len() >= limit {
                if let Some(oldest) = cache.keys().next().cloned() {
                    cache.remove(&oldest);
                }
            }
            cache.insert(
                cache_key,
                CachedRepoMapGraph {
                    fingerprint: fingerprint_after,
                    snapshot: snapshot.clone(),
                },
            );
        }
        Ok((snapshot, false))
    }
}

fn repo_map_scope_path(context: &SoftwareContext) -> String {
    let source_paths = context
        .symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .chain(
            context
                .coverage
                .requirements
                .iter()
                .flat_map(|requirement| requirement.implementation.iter())
                .filter_map(|reference| repo_map_target_path(&reference.target)),
        )
        .collect::<Vec<_>>();
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
        let parts = directory.split('/').collect::<Vec<_>>();
        let matching = common
            .iter()
            .zip(parts.iter())
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

fn repo_map_fingerprint(workspace: &Workspace, path: &str) -> Result<u64> {
    let (paths, truncated) = workspace.source_files(path, REPO_MAP_MAX_FILES)?;
    let mut hasher = DefaultHasher::new();
    workspace.root().hash(&mut hasher);
    path.hash(&mut hasher);
    truncated.hash(&mut hasher);
    paths.len().hash(&mut hasher);
    for path in paths {
        path.hash(&mut hasher);
        match workspace.source_metadata_stamp(&path) {
            Ok(stamp) => stamp.hash(&mut hasher),
            Err(_) => 0u8.hash(&mut hasher),
        }
    }
    Ok(hasher.finish())
}

#[derive(Clone, Debug)]
struct RepoMapCandidate {
    id: String,
    path: String,
    name: String,
    qualified_name: String,
    kind: String,
    relevance: f64,
    direct: bool,
    design_path: bool,
    degree: usize,
    rank: f64,
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
    query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn provider_candidate_index(
    node: &crate::graph::GraphImportNode,
    index_by_id: &HashMap<String, usize>,
    index_by_qualified: &HashMap<(String, String), Vec<usize>>,
    index_by_name: &HashMap<(String, String), Vec<usize>>,
) -> Option<usize> {
    if let Some(index) = index_by_id.get(&node.id) {
        return Some(*index);
    }
    let path = node.attributes.get("path")?.as_str()?.to_owned();
    if let Some(qualified_name) = node
        .attributes
        .get("qualified_name")
        .and_then(Value::as_str)
    {
        if let Some(indices) = index_by_qualified.get(&(path.clone(), qualified_name.to_owned())) {
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
    index_by_name
        .get(&(path, name.to_owned()))
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

fn graph_precision_rank(precision: GraphPrecision) -> u8 {
    match precision {
        GraphPrecision::Runtime => 6,
        GraphPrecision::Semantic => 5,
        GraphPrecision::Deterministic => 4,
        GraphPrecision::Syntax => 3,
        GraphPrecision::Declared => 2,
        GraphPrecision::Heuristic => 1,
        GraphPrecision::Mixed => 0,
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
    let reason = if candidate.direct {
        "direct_match"
    } else if let Some(relation) = direct_relations
        .first()
        .and_then(|relation| relation.get("relation"))
        .and_then(Value::as_str)
    {
        relation
    } else if candidate.design_path && candidate.relevance > 0.0 {
        "design_and_query"
    } else if candidate.relevance > 0.0 {
        "query_related"
    } else if candidate.degree > 0 {
        "graph_neighbor"
    } else {
        "central"
    };
    json!({
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
    })
}
