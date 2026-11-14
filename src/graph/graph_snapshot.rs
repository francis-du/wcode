use super::*;

/// The modification time and the length identify a snapshot file. A symlink,
/// a non-file or a file past the size bound is not a readable snapshot.
pub(super) fn snapshot_identity(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return None,
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_GRAPH_SNAPSHOT_BYTES.saturating_add(512 * 1024)
    {
        return None;
    }
    Some((metadata.modified().unwrap_or(UNIX_EPOCH), metadata.len()))
}

pub(crate) fn query(workspace: &Workspace, input: &GraphQueryInput) -> Result<GraphQueryResult> {
    let stored = load_selected(workspace, input.snapshot_id.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
    let limit = input.limit.clamp(1, 500);
    let label = input
        .label_contains
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let mut selected = BTreeSet::new();
    for node in stored.snapshot.graph.nodes.values() {
        if input.node_id.as_ref().is_some_and(|id| id != &node.id)
            || input.kind.is_some_and(|kind| kind != node.kind)
            || label
                .as_ref()
                .is_some_and(|needle| !contains_ascii_case_insensitive(&node.label, needle))
        {
            continue;
        }
        selected.insert(node.id.clone());
        if selected.len() >= limit {
            break;
        }
    }

    let mut edges = Vec::new();
    if let Some(related_to) = input.related_to.as_deref() {
        let direction = input.direction.unwrap_or(GraphDirection::Both);
        for edge in &stored.snapshot.graph.edges {
            if input.edge_kind.is_some_and(|kind| kind != edge.kind) {
                continue;
            }
            let matches = match direction {
                GraphDirection::Incoming => edge.to == related_to,
                GraphDirection::Outgoing => edge.from == related_to,
                GraphDirection::Both => edge.from == related_to || edge.to == related_to,
            };
            if !matches {
                continue;
            }
            selected.insert(edge.from.clone());
            selected.insert(edge.to.clone());
            edges.push(edge.clone());
            if edges.len() >= limit {
                break;
            }
        }
    } else {
        for edge in &stored.snapshot.graph.edges {
            if input.edge_kind.is_some_and(|kind| kind != edge.kind)
                || (!selected.contains(&edge.from) && !selected.contains(&edge.to))
            {
                continue;
            }
            edges.push(edge.clone());
            if edges.len() >= limit {
                break;
            }
        }
    }

    if selected.is_empty()
        && input.node_id.is_none()
        && input.kind.is_none()
        && label.is_none()
        && input.related_to.is_none()
    {
        selected.extend(stored.snapshot.graph.nodes.keys().take(limit).cloned());
    }
    let nodes = selected
        .iter()
        .filter_map(|id| stored.snapshot.graph.nodes.get(id).cloned())
        .take(limit)
        .collect::<Vec<_>>();
    let truncated = nodes.len() >= limit
        || edges.len() >= limit
        || stored.snapshot.graph.nodes.len() > nodes.len()
            && input.node_id.is_none()
            && input.kind.is_none()
            && label.is_none();
    Ok(GraphQueryResult {
        snapshot_id: stored.id.clone(),
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider.clone(),
        precision: stored.snapshot.precision,
        nodes,
        edges,
        truncated,
    })
}

pub(crate) fn chain(workspace: &Workspace, input: &GraphChainInput) -> Result<GraphChainResult> {
    crate::graph_chain::query(workspace, input)
}

pub(crate) fn diff(workspace: &Workspace, input: &GraphDiffInput) -> Result<GraphDiffResult> {
    let (from, to) = select_diff_snapshots(workspace, input)?;
    let limit = input.limit.clamp(1, 200);

    let mut added_nodes = Vec::new();
    let mut changed_nodes = Vec::new();
    for (id, after) in &to.snapshot.graph.nodes {
        match from.snapshot.graph.nodes.get(id) {
            None => added_nodes.push(after.clone()),
            Some(before) if before != after => changed_nodes.push(GraphNodeChange {
                before: before.clone(),
                after: after.clone(),
            }),
            Some(_) => {}
        }
    }
    let mut removed_nodes = from
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|(id, _)| !to.snapshot.graph.nodes.contains_key(*id))
        .map(|(_, node)| node.clone())
        .collect::<Vec<_>>();

    let from_edges = grouped_edges(&from.snapshot.graph.edges);
    let mut to_edges = grouped_edges(&to.snapshot.graph.edges);
    let mut added_edges = Vec::new();
    let mut removed_edges = Vec::new();
    let mut changed_edges = Vec::new();
    for (identity, before_group) in from_edges {
        let after_group = to_edges.remove(&identity).unwrap_or_default();
        diff_edge_group(
            before_group,
            after_group,
            &mut added_edges,
            &mut removed_edges,
            &mut changed_edges,
        );
    }
    for after_group in to_edges.into_values() {
        added_edges.extend(after_group);
    }

    added_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    removed_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    changed_nodes.sort_by(|left, right| left.before.id.cmp(&right.before.id));
    added_edges.sort_by(graph_edge_order);
    removed_edges.sort_by(graph_edge_order);
    changed_edges.sort_by(|left, right| graph_edge_order(&left.before, &right.before));

    let added_node_count = added_nodes.len();
    let removed_node_count = removed_nodes.len();
    let changed_node_count = changed_nodes.len();
    let added_edge_count = added_edges.len();
    let removed_edge_count = removed_edges.len();
    let changed_edge_count = changed_edges.len();
    added_nodes.truncate(limit);
    removed_nodes.truncate(limit);
    changed_nodes.truncate(limit);
    added_edges.truncate(limit);
    removed_edges.truncate(limit);
    changed_edges.truncate(limit);
    let truncated = added_node_count > added_nodes.len()
        || removed_node_count > removed_nodes.len()
        || changed_node_count > changed_nodes.len()
        || added_edge_count > added_edges.len()
        || removed_edge_count > removed_edges.len()
        || changed_edge_count > changed_edges.len();

    Ok(GraphDiffResult {
        from_snapshot_id: from.id.clone(),
        to_snapshot_id: to.id.clone(),
        from_captured_at_ms: from.captured_at_ms,
        to_captured_at_ms: to.captured_at_ms,
        added_node_count,
        removed_node_count,
        changed_node_count,
        added_edge_count,
        removed_edge_count,
        changed_edge_count,
        added_nodes,
        removed_nodes,
        changed_nodes,
        added_edges,
        removed_edges,
        changed_edges,
        truncated,
    })
}
