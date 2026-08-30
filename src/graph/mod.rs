use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub type NodeId = String;

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
