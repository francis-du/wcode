use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub type NodeId = String;

/// ASCII case-insensitive comparisons that avoid allocating a lowercased copy
/// of every node label and path on each query. Search needles are already
/// lowercased, so only the haystack bytes are folded.
pub(crate) fn eq_ascii_case_insensitive(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| a.to_ascii_lowercase() == b)
}

pub(crate) fn starts_with_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .bytes()
            .take(needle.len())
            .zip(needle.bytes())
            .all(|(a, b)| a.to_ascii_lowercase() == b)
}

pub(crate) fn ends_with_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .bytes()
            .rev()
            .take(needle.len())
            .zip(needle.bytes().rev())
            .all(|(a, b)| a.to_ascii_lowercase() == b)
}

pub(crate) fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
    })
}

/// Appends the ASCII-lowercased form of `value` without allocating a copy per
/// field. Fields are joined with spaces, so callers that need the boundary
/// semantics of the joined haystack must keep appending to one buffer.
pub(crate) fn push_ascii_lowercased(target: &mut String, value: &str) {
    for character in value.chars() {
        target.push(character.to_ascii_lowercase());
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Product,
    Requirement,
    AcceptanceCriterion,
    Constraint,
    Decision,
    Component,
    Package,
    Module,
    File,
    Symbol,
    Function,
    Struct,
    Trait,
    Class,
    Enum,
    Interface,
    Api,
    Database,
    Queue,
    Config,
    Test,
    Verification,
    Risk,
    Evidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Defines,
    References,
    Calls,
    Imports,
    DependsOn,
    Implements,
    Extends,
    ImplementsRequirement,
    ConstrainedBy,
    TestedBy,
    VerifiedBy,
    GuardsAgainst,
    ProducesEvidence,
    RuntimeCalls,
    ConflictsWith,
}

impl EdgeKind {
    /// Allocation-free name for sort tiebreakers. Matches the `Debug` rendering
    /// that `graph_edge_order` used to format once per comparison.
    pub(crate) const fn sort_name(self) -> &'static str {
        match self {
            Self::Contains => "Contains",
            Self::Defines => "Defines",
            Self::References => "References",
            Self::Calls => "Calls",
            Self::Imports => "Imports",
            Self::DependsOn => "DependsOn",
            Self::Implements => "Implements",
            Self::Extends => "Extends",
            Self::ImplementsRequirement => "ImplementsRequirement",
            Self::ConstrainedBy => "ConstrainedBy",
            Self::TestedBy => "TestedBy",
            Self::VerifiedBy => "VerifiedBy",
            Self::GuardsAgainst => "GuardsAgainst",
            Self::ProducesEvidence => "ProducesEvidence",
            Self::RuntimeCalls => "RuntimeCalls",
            Self::ConflictsWith => "ConflictsWith",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphPrecision {
    Declared,
    Syntax,
    Semantic,
    Runtime,
    Deterministic,
    Heuristic,
    Mixed,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphProvenance {
    pub provider: String,
    pub precision: GraphPrecision,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
    pub provenance: GraphProvenance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub provenance: GraphProvenance,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphImportNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphImportEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphProviderImport {
    pub provider: String,
    pub precision: GraphPrecision,
    pub revision: String,
    #[serde(default)]
    pub nodes: Vec<GraphImportNode>,
    #[serde(default)]
    pub edges: Vec<GraphImportEdge>,
}

impl GraphProviderImport {
    pub fn validate(&self) -> Result<(), GraphError> {
        let provenance = GraphProvenance {
            provider: self.provider.clone(),
            precision: self.precision,
            revision: self.revision.clone(),
        };
        validate_provenance(&provenance)?;
        if matches!(
            self.precision,
            GraphPrecision::Declared | GraphPrecision::Syntax | GraphPrecision::Mixed
        ) || self.nodes.len() > 10_000
            || self.edges.len() > 50_000
        {
            return Err(GraphError::InvalidProviderImport);
        }
        let mut ids = HashSet::new();
        for node in &self.nodes {
            if !valid_id(&node.id)
                || node.label.trim().is_empty()
                || node.label.len() > 500
                || !ids.insert(node.id.as_str())
            {
                return Err(GraphError::InvalidProviderImport);
            }
        }
        for edge in &self.edges {
            if !valid_id(&edge.from) || !valid_id(&edge.to) || edge.from == edge.to {
                return Err(GraphError::InvalidProviderImport);
            }
        }
        Ok(())
    }

    pub fn provenance(&self) -> GraphProvenance {
        GraphProvenance {
            provider: self.provider.clone(),
            precision: self.precision,
            revision: self.revision.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SoftwareGraph {
    pub nodes: BTreeMap<NodeId, GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphBuildFailure {
    pub path: String,
    pub error: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SoftwareGraphSnapshot {
    pub workspace: String,
    pub path: String,
    pub provider: String,
    pub precision: GraphPrecision,
    pub files_considered: usize,
    pub files_indexed: usize,
    pub files_failed: usize,
    pub scan_truncated: bool,
    pub truncated: bool,
    pub node_count: usize,
    pub edge_count: usize,
    /// Source-scan counts before the Design and provider overlays are applied.
    /// Present only on a composite snapshot, so a caller that asked for a small
    /// graph can see how much of `node_count` the overlays added rather than
    /// reading `truncated` (which describes the source scan) as a statement
    /// about the whole response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_edge_count: Option<usize>,
    pub failures: Vec<GraphBuildFailure>,
    pub graph: SoftwareGraph,
}

impl SoftwareGraph {
    pub fn add_node(&mut self, node: GraphNode) -> Result<(), GraphError> {
        validate_node(&node)?;
        if self.nodes.insert(node.id.clone(), node).is_some() {
            return Err(GraphError::DuplicateNode);
        }
        Ok(())
    }

    pub fn add_edge(&mut self, edge: GraphEdge) -> Result<(), GraphError> {
        if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
            return Err(GraphError::UnknownEndpoint);
        }
        if edge.from == edge.to && edge.kind != EdgeKind::ConflictsWith {
            return Err(GraphError::InvalidSelfEdge);
        }
        if self.edges.iter().any(|existing| {
            existing.from == edge.from
                && existing.to == edge.to
                && existing.kind == edge.kind
                && existing.provenance == edge.provenance
        }) {
            return Err(GraphError::DuplicateEdge);
        }
        validate_provenance(&edge.provenance)?;
        self.edges.push(edge);
        Ok(())
    }

    /// Append an edge whose endpoints and uniqueness the caller already
    /// established. Provenance is still validated; only the O(E) duplicate
    /// rescan is skipped, so a bulk overlay stays linear in its edge count.
    pub fn add_edge_after_dedup(&mut self, edge: GraphEdge) -> Result<(), GraphError> {
        if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
            return Err(GraphError::UnknownEndpoint);
        }
        if edge.from == edge.to && edge.kind != EdgeKind::ConflictsWith {
            return Err(GraphError::InvalidSelfEdge);
        }
        validate_provenance(&edge.provenance)?;
        self.edges.push(edge);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), GraphError> {
        for node in self.nodes.values() {
            validate_node(node)?;
        }
        let mut seen = HashSet::new();
        for edge in &self.edges {
            if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
                return Err(GraphError::UnknownEndpoint);
            }
            validate_provenance(&edge.provenance)?;
            if edge.from == edge.to && edge.kind != EdgeKind::ConflictsWith {
                return Err(GraphError::InvalidSelfEdge);
            }
            if !seen.insert((
                edge.from.as_str(),
                edge.to.as_str(),
                edge.kind,
                edge.provenance.provider.as_str(),
                edge.provenance.precision,
                edge.provenance.revision.as_str(),
            )) {
                return Err(GraphError::DuplicateEdge);
            }
        }
        Ok(())
    }

    pub fn nodes_of_kind(&self, kind: NodeKind) -> impl Iterator<Item = &GraphNode> {
        self.nodes.values().filter(move |node| node.kind == kind)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphError {
    InvalidNode,
    InvalidProvenance,
    DuplicateNode,
    UnknownEndpoint,
    DuplicateEdge,
    InvalidSelfEdge,
    InvalidProviderImport,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidNode => "graph node id, label, or provenance is invalid",
            Self::InvalidProvenance => "graph provenance is invalid",
            Self::DuplicateNode => "graph node already exists",
            Self::UnknownEndpoint => "graph edge endpoint does not exist",
            Self::DuplicateEdge => "graph edge already exists",
            Self::InvalidSelfEdge => "graph edge cannot point to itself",
            Self::InvalidProviderImport => "external graph provider import is invalid or unbounded",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for GraphError {}

fn validate_node(node: &GraphNode) -> Result<(), GraphError> {
    if !valid_id(&node.id) || node.label.trim().is_empty() || node.label.len() > 500 {
        return Err(GraphError::InvalidNode);
    }
    validate_provenance(&node.provenance).map_err(|_| GraphError::InvalidNode)
}

fn validate_provenance(provenance: &GraphProvenance) -> Result<(), GraphError> {
    if provenance.provider.trim().is_empty()
        || provenance.provider.len() > 128
        || provenance.revision.trim().is_empty()
        || provenance.revision.len() > 256
    {
        return Err(GraphError::InvalidProvenance);
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "../../tests/unit/graph/mod.rs"]
mod tests;
