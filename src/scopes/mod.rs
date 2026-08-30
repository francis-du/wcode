use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductScope {
    Runtime,
    Integrations,
    Workspace,
    Design,
    Graph,
    Semantics,
    Traceability,
    Risk,
    Verification,
    Evidence,
    Reconciliation,
    Experience,
}

impl ProductScope {
    pub const ALL: [Self; 12] = [
        Self::Runtime,
        Self::Integrations,
        Self::Workspace,
        Self::Design,
        Self::Graph,
        Self::Semantics,
        Self::Traceability,
        Self::Risk,
        Self::Verification,
        Self::Evidence,
        Self::Reconciliation,
        Self::Experience,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Integrations => "integrations",
            Self::Workspace => "workspace",
            Self::Design => "design",
            Self::Graph => "graph",
            Self::Semantics => "semantics",
            Self::Traceability => "traceability",
            Self::Risk => "risk",
            Self::Verification => "verification",
            Self::Evidence => "evidence",
            Self::Reconciliation => "reconciliation",
            Self::Experience => "experience",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Runtime => "Runtime",
            Self::Integrations => "Model & Agent Integrations",
            Self::Workspace => "Workspace & Coding Boundary",
            Self::Design => "Design State",
            Self::Graph => "Software Graph",
            Self::Semantics => "Semantic Registry & Providers",
            Self::Traceability => "Traceability, Drift & Impact",
            Self::Risk => "Risk",
            Self::Verification => "Verification Mesh",
            Self::Evidence => "Evidence",
            Self::Reconciliation => "Reconciliation",
            Self::Experience => "TUI & Web Experience",
        }
    }

    pub const fn purpose(self) -> &'static str {
        match self {
            Self::Runtime => "process lifecycle, global orchestration, health, concurrency and platform runtime services",
            Self::Integrations => "MCP transports, OAuth, Tasks and portable agent/plugin integration surfaces",
            Self::Workspace => "workspace isolation, bounded coding primitives, scheduling, authorization and repository conventions",
            Self::Design => "machine-operable desired software state, constraints, acceptance criteria and decisions",
            Self::Graph => "syntax index plus durable provider-neutral Software Graph state",
            Self::Semantics => "confirmed/candidate workspace semantics and first-party semantic provider adapters",
            Self::Traceability => "software context, requirement traceability, drift and transitive impact analysis",
            Self::Risk => "multi-dimensional risk derivation and risk-adaptive policy",
            Self::Verification => "deterministic and independent reviewer/stage verification orchestration",
            Self::Evidence => "durable provenance-bearing proof and disagreement records",
            Self::Reconciliation => "desired-state to actual-state convergence plans and resumable execution",
            Self::Experience => "local TUI, Setup/Intelligence WebUI and operator-facing observability",
        }
    }

    pub const fn source_roots(self) -> &'static [&'static str] {
        match self {
            Self::Runtime => &[
                "src/main.rs",
                "src/lib.rs",
                "src/app/",
                "src/runtime/",
                "src/scopes/",
            ],
            Self::Integrations => &["src/integrations/"],
            Self::Workspace => &["src/workspace/"],
            Self::Design => &["src/design/"],
            Self::Graph => &["src/graph/"],
            Self::Semantics => &["src/semantics/"],
            Self::Traceability => &[
                "src/intelligence/mod.rs",
                "src/intelligence/analysis.rs",
                "src/intelligence/context.rs",
                "src/intelligence/observatory.rs",
                "src/intelligence/observatory/",
                "src/intelligence/runtime/",
                "src/intelligence/types.rs",
            ],
            Self::Risk => &["src/intelligence/risk.rs"],
            Self::Verification => &["src/verification/"],
            Self::Evidence => &["src/evidence/"],
            Self::Reconciliation => &["src/reconciliation/"],
            Self::Experience => &["src/ui/"],
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductScopeDescriptor {
    pub id: &'static str,
    pub title: &'static str,
    pub purpose: &'static str,
    pub source_roots: &'static [&'static str],
}

pub fn registry() -> Vec<ProductScopeDescriptor> {
    ProductScope::ALL
        .into_iter()
        .map(|scope| ProductScopeDescriptor {
            id: scope.as_str(),
            title: scope.title(),
            purpose: scope.purpose(),
            source_roots: scope.source_roots(),
        })
        .collect()
}

pub fn parse(value: &str) -> Option<ProductScope> {
    match normalize_token(value).as_str() {
        "runtime" | "control-plane" | "control_plane" => Some(ProductScope::Runtime),
        "integration" | "integrations" | "mcp" | "agent" | "agents" | "connector"
        | "connectors" => Some(ProductScope::Integrations),
        "workspace" | "coding" | "repository" | "repo" => Some(ProductScope::Workspace),
        "design" | "design-state" | "design_state" | "desired-state" | "desired_state" => {
            Some(ProductScope::Design)
        }
        "graph" | "software-graph" | "software_graph" | "index" => Some(ProductScope::Graph),
        "semantic" | "semantics" | "semantic-registry" | "semantic_registry" | "lsp" => {
            Some(ProductScope::Semantics)
        }
        "traceability" | "drift" | "impact" | "software-context" | "software_context"
        | "intelligence" => Some(ProductScope::Traceability),
        "risk" => Some(ProductScope::Risk),
        "verification" | "verification-mesh" | "verification_mesh" | "test" | "tests" => {
            Some(ProductScope::Verification)
        }
        "evidence" | "proof" => Some(ProductScope::Evidence),
        "reconciliation" | "reconcile" | "convergence" => Some(ProductScope::Reconciliation),
        "experience" | "ui" | "tui" | "web" | "webui" | "dashboard" => {
            Some(ProductScope::Experience)
        }
        _ => None,
    }
}

pub fn canonical_name(value: &str) -> String {
    parse(value)
        .map(|scope| scope.as_str().to_owned())
        .unwrap_or_else(|| normalize_token(value))
}

pub fn canonicalize(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| canonical_name(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn source_scope(path: &str) -> Option<ProductScope> {
    if path == "src/intelligence/risk.rs" {
        return Some(ProductScope::Risk);
    }
    ProductScope::ALL.into_iter().find(|scope| {
        scope.source_roots().iter().any(|root| {
            if root.ends_with('/') {
                path.starts_with(root)
            } else {
                path == *root
            }
        })
    })
}

pub fn source_roots_for(requested: &[String]) -> Vec<&'static str> {
    let scopes = requested
        .iter()
        .filter_map(|scope| parse(scope))
        .collect::<BTreeSet<_>>();
    if scopes.is_empty() {
        return vec!["."];
    }
    scopes
        .into_iter()
        .flat_map(|scope| scope.source_roots().iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn tool_scopes(name: &str) -> Vec<ProductScope> {
    use ProductScope::*;
    let scopes: &[ProductScope] = match name {
        "workspace_info" | "scope_status" => &[Runtime, Workspace, Integrations],
        "project_context" | "convention_status" | "list_files" | "search_code" | "search_many"
        | "read_file" | "read_files" | "read_media" | "path_info" | "replace_text"
        | "apply_edits" | "write_file" | "create_directory" | "create_file" | "create_files"
        | "apply_file_edits" | "move_path" | "move_paths" | "delete_path" | "run_command"
        | "parallel_tools" | "review_changes" => &[Workspace],
        "design_init" | "design_status" => &[Design],
        "software_graph"
        | "graph_provider_import"
        | "graph_provider_status"
        | "graph_history"
        | "graph_query"
        | "graph_diff"
        | "file_outline"
        | "find_symbol"
        | "symbol_context" => &[Graph],
        "semantic_provider_status"
        | "semantic_provider_refresh"
        | "semantic_navigation"
        | "semantic_status"
        | "semantic_query"
        | "semantic_record"
        | "semantic_confirm"
        | "semantic_retire" => &[Semantics],
        "traceability_status" | "drift_status" | "impact_analysis" => &[Traceability],
        "software_context" => &[Design, Graph, Semantics, Traceability, Risk],
        "agent_context" => &[
            Design,
            Graph,
            Semantics,
            Traceability,
            Risk,
            Verification,
            Workspace,
        ],
        "risk_status" => &[Risk],
        "language_quality_status" => &[Verification, Workspace, Semantics],
        "language_quality_run" => &[Verification, Workspace, Evidence],
        "verification_plan"
        | "verification_claim"
        | "verification_submit"
        | "verification_executor_status"
        | "verification_execute_stages"
        | "verification_stage_submit"
        | "verification_approve"
        | "verification_status"
        | "verification_history"
        | "verify_project" => &[Verification],
        "evidence_status" => &[Evidence],
        "reconciliation_plan"
        | "reconciliation_status"
        | "reconciliation_history"
        | "reconciliation_execution_status"
        | "reconciliation_claim"
        | "reconciliation_submit"
        | "reconciliation_retry" => &[Reconciliation],
        _ => &[],
    };
    scopes.to_vec()
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "-")
}

#[cfg(test)]
#[path = "../../tests/unit/scopes/mod.rs"]
mod tests;
