//! Bounded, revision-bound inspection supplement to native Acceptance.
//! Reuses the Risk trace and the exact Graph used by impact analysis. It does
//! not execute checks, create Evidence or infer symbol-body changes from paths.
use super::{harness_policy_select, ToolHarness};
use crate::design::{DesignState, VerificationRef};
use crate::evidence::Revision;
use crate::graph::{GraphNode, GraphPrecision, NodeKind, SoftwareGraphSnapshot};
use crate::intelligence::{RiskStatus, TraceReferenceKind};
use crate::reconcile::ImpactAnalysis;
use crate::verification::acceptance::{
    AcceptanceAction, AcceptanceChangeInspection, AcceptanceFreshness, AcceptanceImpactSummary,
    AcceptanceInspectedSymbol, AcceptanceInspectionCoverage, AcceptanceMappedVerification,
    AcceptanceMappingKind, AcceptanceRiskFinding, AcceptanceRiskSummary, AcceptanceSymbolSelection,
};
use crate::verification::change::GitChangeSnapshot;
use crate::workspace::Workspace;
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, BTreeSet};

const ROWS: usize = 128;
const RISK_ROWS: usize = 32;
const BYTE_BUDGET: usize = 256 * 1024;

impl ToolHarness {
    pub(crate) fn acceptance_change_inspection(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        revision: &Revision,
        git: &GitChangeSnapshot,
        risk: &RiskStatus,
    ) -> Result<AcceptanceChangeInspection> {
        ensure!(
            risk.workspace == workspace_id
                && risk.revision == *revision
                && risk.traceability.workspace == workspace_id
                && risk.drift.workspace == workspace_id,
            "acceptance inspection Risk identity does not match"
        );
        ensure!(
            self.current_revision(workspace)? == *revision,
            "acceptance inspection revision changed before capture"
        );
        let load = self.intelligence.design_load(workspace)?;
        let review = harness_policy_select::candidate_review(workspace_id, git);
        let (impact, graph) = self.intelligence.impact_analysis_with_graph_from_snapshot(
            workspace_id,
            workspace,
            &self.code_index,
            &review,
            &load.state,
            risk.level,
        )?;
        let result = project_inspection(
            workspace_id,
            revision,
            git,
            risk,
            &load.state,
            &impact,
            &graph,
        )?;
        ensure!(
            self.current_revision(workspace)? == *revision,
            "acceptance inspection revision changed during capture"
        );
        Ok(result)
    }
}

fn project_inspection(
    workspace_id: &str,
    revision: &Revision,
    git: &GitChangeSnapshot,
    risk: &RiskStatus,
    state: &DesignState,
    impact: &ImpactAnalysis,
    graph: &SoftwareGraphSnapshot,
) -> Result<AcceptanceChangeInspection> {
    ensure!(
        impact.workspace == workspace_id && graph.workspace == workspace_id,
        "acceptance inspection Graph/Impact workspace does not match"
    );
    let paths = git
        .changes
        .iter()
        .flat_map(|change| change.old_path.iter().chain(change.new_path.iter()))
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in &paths {
        crate::verification::change::changed_path(path)?;
    }
    let mut unknown = BTreeSet::new();
    let complete_hash = |value: &str| {
        value.strip_prefix("sha256:").is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    if !complete_hash(&revision.code) || !revision.design.as_deref().is_some_and(complete_hash) {
        unknown.insert("revision_incomplete".into());
    }
    if !git.complete {
        unknown.insert("git_capture_incomplete".into());
    }
    let graph_complete = !graph.truncated && !graph.scan_truncated && graph.files_failed == 0;
    if !graph_complete {
        unknown.insert("graph_capture_incomplete".into());
    }
    let mappings_complete = risk.traceability.initialized
        && risk.traceability.valid_design
        && !risk.traceability.truncated;
    if !mappings_complete {
        unknown.insert("traceability_incomplete".into());
    }
    let graph_files = graph
        .graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::File)
        .filter_map(|node| Some((attribute(node, "path")?, attribute(node, "sha256")?)))
        .collect::<BTreeMap<_, _>>();
    let mut symbols = Vec::new();
    let mut symbols_observed = 0usize;
    for node in graph.graph.nodes.values() {
        let Some(path) = attribute(node, "path") else {
            continue;
        };
        if !paths.contains(path) || !symbol_kind(node.kind) {
            continue;
        }
        symbols_observed += 1;
        let name = attribute(node, "name").unwrap_or(&node.label);
        if !safe_text(&node.id, 512)
            || !safe_text(name, 256)
            || !safe_text(&node.provenance.provider, 128)
            || crate::verification::change::changed_path(path).is_err()
        {
            unknown.insert("unsafe_graph_metadata_omitted".into());
            continue;
        }
        let freshness = symbol_freshness(node, graph_files.get(path).copied());
        if freshness != AcceptanceFreshness::Current {
            unknown.insert("provider_source_binding_unknown".into());
        }
        if symbols.len() < ROWS {
            let range = node.attributes.get("range");
            symbols.push(AcceptanceInspectedSymbol {
                id: node.id.clone(),
                path: path.into(),
                name: name.into(),
                provider: node.provenance.provider.clone(),
                precision: node.provenance.precision,
                selection: AcceptanceSymbolSelection::FileMembership,
                freshness,
                start_line: range
                    .and_then(|value| value.get("start_line"))
                    .and_then(|value| value.as_u64())
                    .and_then(|value| usize::try_from(value).ok()),
                end_line: range
                    .and_then(|value| value.get("end_line"))
                    .and_then(|value| value.as_u64())
                    .and_then(|value| usize::try_from(value).ok()),
            });
        }
    }
    symbols.sort_by(|a, b| (&a.path, &a.id).cmp(&(&b.path, &b.id)));
    // Current-source graph membership is not a base-to-head symbol body diff.
    if !paths.is_empty() {
        unknown.insert("symbol_body_delta_not_captured".into());
    }
    let mapped = state
        .components
        .values()
        .flat_map(|component| component.implementation.iter())
        .map(|reference| reference.path())
        .collect::<BTreeSet<_>>();
    let unmapped = paths
        .iter()
        .filter(|path| !mapped.contains(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let uncovered = paths
        .iter()
        .filter(|path| !graph_files.contains_key(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unmapped.is_empty() {
        unknown.insert("changed_paths_unmapped".into());
    }
    if !uncovered.is_empty() {
        unknown.insert("changed_paths_not_indexed".into());
    }
    let (mapped_verification, verification_observed) =
        verification_mapping(state, risk, impact, &mut unknown);
    let risk_findings = risk
        .risks
        .iter()
        .filter(|finding| {
            let safe = safe_text(&finding.id, 160);
            if !safe {
                unknown.insert("unsafe_risk_metadata_omitted".into());
            }
            safe
        })
        .take(RISK_ROWS)
        .map(|finding| AcceptanceRiskFinding {
            id: finding.id.clone(),
            category: finding.category,
            level: finding.level,
        })
        .collect::<Vec<_>>();
    let impact_limited = impact.impacted_components.len() >= 512
        || impact.impacted_requirements.len() >= 512
        || impact.impacted_acceptance.len() >= 512
        || impact.impacted_symbols.len() >= 2_000;
    if impact_limited {
        unknown.insert("impact_inventory_may_be_truncated".into());
    }
    let mut result = AcceptanceChangeInspection {
        workspace: workspace_id.into(),
        revision: revision.clone(),
        base_sha: git.base_sha.clone(),
        target_sha: git.target_sha.clone(),
        git_binding: git.binding.clone(),
        producer: "wcode/native-change-inspection/v1".into(),
        symbols,
        impacted_components: bounded_ids(&impact.impacted_components, &mut unknown),
        impacted_requirements: bounded_ids(&impact.impacted_requirements, &mut unknown),
        impacted_acceptance: bounded_ids(&impact.impacted_acceptance, &mut unknown),
        mapped_verification: mapped_verification.into_iter().take(ROWS).collect(),
        impact: AcceptanceImpactSummary {
            graph_provider: impact.graph_provider.clone(),
            graph_precision: impact.graph_precision.clone(),
            graph_truncated: impact.graph_truncated,
            transitive_callers: impact.transitive_callers,
            public_api: impact.public_api,
            security_boundary: impact.security_boundary,
        },
        risk: AcceptanceRiskSummary {
            level: risk.level,
            precision: "native_heuristic_assessment".into(),
            findings_total: risk.risks.len(),
            findings: risk_findings,
            bug_pattern_matches: risk.bug_patterns.matches,
            drift_findings: risk.drift.findings.len(),
            truncated: risk.risks.len() > RISK_ROWS
                || risk.bug_patterns.truncated
                || risk.drift.truncated,
        },
        coverage: AcceptanceInspectionCoverage {
            changed_paths_total: paths.len(),
            graph_files_indexed: graph.files_indexed,
            symbols_observed,
            symbols_returned: 0,
            mapped_paths_total: paths.len().saturating_sub(unmapped.len()),
            unmapped_paths_total: unmapped.len(),
            unmapped_paths: unmapped.into_iter().take(ROWS).collect(),
            uncovered_paths_total: uncovered.len(),
            uncovered_paths: uncovered.into_iter().take(ROWS).collect(),
            verification_observed,
            verification_returned: 0,
            components_observed: impact.impacted_components.len(),
            requirements_observed: impact.impacted_requirements.len(),
            acceptance_observed: impact.impacted_acceptance.len(),
            mappings_complete,
            graph_complete,
            totals_complete: mappings_complete
                && graph_complete
                && !impact_limited
                && !unknown.contains("verification_inventory_truncated"),
        },
        unknown_reasons: unknown.into_iter().collect(),
        recommended_actions: Vec::new(),
        truncated: false,
        complete: false,
    };
    finalize(&mut result)?;
    Ok(result)
}

fn verification_mapping(
    state: &DesignState,
    risk: &RiskStatus,
    impact: &ImpactAnalysis,
    unknown: &mut BTreeSet<String>,
) -> (Vec<AcceptanceMappedVerification>, usize) {
    const REFERENCE_LIMIT: usize = 4096;
    let mut resolutions = BTreeMap::new();
    for (index, reference) in risk
        .traceability
        .requirements
        .iter()
        .flat_map(|requirement| &requirement.verification)
        .enumerate()
    {
        if index == REFERENCE_LIMIT {
            unknown.insert("verification_inventory_truncated".into());
            break;
        }
        resolutions.insert(
            (reference.owner.as_str(), reference.target.as_str()),
            reference,
        );
    }
    let mut rows = BTreeMap::new();
    let mut observed = 0;
    'criteria: for id in &impact.impacted_acceptance {
        let Some(criterion) = state.acceptance.get(id) else {
            unknown.insert("impacted_acceptance_missing".into());
            continue;
        };
        if criterion.verification.is_empty() {
            unknown.insert("impacted_acceptance_uncovered".into());
        }
        for reference in &criterion.verification {
            if observed == REFERENCE_LIMIT {
                unknown.insert("verification_inventory_truncated".into());
                break 'criteria;
            }
            observed += 1;
            if rows.len() == ROWS {
                continue;
            }
            let (kind, target) = match reference {
                VerificationRef::Test { path, symbol } => {
                    if crate::verification::change::changed_path(path).is_err()
                        || !safe_text(symbol, 256)
                    {
                        unknown.insert("unsafe_verification_metadata_omitted".into());
                        continue;
                    }
                    (AcceptanceMappingKind::Test, format!("{path}::{symbol}"))
                }
                VerificationRef::Check { id } => (AcceptanceMappingKind::Check, id.clone()),
            };
            if !safe_text(&criterion.id, 160) || !safe_text(&target, 1536) {
                unknown.insert("unsafe_verification_metadata_omitted".into());
                continue;
            }
            let resolved = resolutions
                .get(&(criterion.id.as_str(), target.as_str()))
                .filter(|reference| {
                    matches!(
                        reference.kind,
                        TraceReferenceKind::Test | TraceReferenceKind::Check
                    )
                });
            if resolved.is_none_or(|reference| !reference.resolved) {
                unknown.insert("mapped_verification_unresolved".into());
            }
            let provider = resolved
                .map(|reference| reference.provider.as_str())
                .unwrap_or("design");
            let precision = resolved
                .map(|reference| reference.precision.as_str())
                .unwrap_or("declared");
            if !safe_text(provider, 128) || !safe_text(precision, 64) {
                unknown.insert("unsafe_verification_metadata_omitted".into());
                continue;
            }
            let resolved = resolved.is_some_and(|reference| reference.resolved);
            let provider = provider.to_owned();
            let precision = precision.to_owned();
            rows.insert(
                (criterion.id.clone(), target.clone()),
                AcceptanceMappedVerification {
                    owner: criterion.id.clone(),
                    target,
                    kind,
                    resolved,
                    provider,
                    precision,
                    relation: "declared_verification".into(),
                },
            );
        }
    }
    (rows.into_values().collect(), observed)
}

fn bounded_ids(ids: &[String], unknown: &mut BTreeSet<String>) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            let safe = safe_text(id, 160);
            if !safe {
                unknown.insert("unsafe_design_metadata_omitted".into());
            }
            safe
        })
        .take(ROWS)
        .cloned()
        .collect()
}

fn symbol_freshness(node: &GraphNode, source: Option<&str>) -> AcceptanceFreshness {
    let Some(source) = source else {
        return AcceptanceFreshness::Missing;
    };
    let expected = if node.provenance.provider == "tree-sitter"
        && node.provenance.precision == GraphPrecision::Syntax
    {
        Some(node.provenance.revision.as_str())
    } else {
        attribute(node, "source_sha256")
    };
    let Some(expected) = expected else {
        return AcceptanceFreshness::Unbound;
    };
    if expected.strip_prefix("sha256:").unwrap_or(expected)
        == source.strip_prefix("sha256:").unwrap_or(source)
    {
        AcceptanceFreshness::Current
    } else {
        AcceptanceFreshness::Stale
    }
}

fn attribute<'a>(node: &'a GraphNode, key: &str) -> Option<&'a str> {
    node.attributes.get(key).and_then(|value| value.as_str())
}

fn symbol_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Symbol
            | NodeKind::Function
            | NodeKind::Struct
            | NodeKind::Trait
            | NodeKind::Class
            | NodeKind::Enum
            | NodeKind::Interface
            | NodeKind::Module
            | NodeKind::Api
            | NodeKind::Test
    )
}

fn safe_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.chars().any(char::is_control)
        && !crate::workspace::redact_sensitive_text(value).1
}

fn finalize(result: &mut AcceptanceChangeInspection) -> Result<()> {
    result.coverage.symbols_returned = result.symbols.len();
    result.coverage.verification_returned = result.mapped_verification.len();
    result.truncated = !result.coverage.totals_complete
        || result.risk.truncated
        || result.coverage.symbols_observed > result.symbols.len()
        || result.coverage.verification_observed > result.mapped_verification.len()
        || result.coverage.components_observed > result.impacted_components.len()
        || result.coverage.requirements_observed > result.impacted_requirements.len()
        || result.coverage.acceptance_observed > result.impacted_acceptance.len()
        || result.coverage.unmapped_paths_total > result.coverage.unmapped_paths.len()
        || result.coverage.uncovered_paths_total > result.coverage.uncovered_paths.len();
    if result.truncated {
        result
            .unknown_reasons
            .push("inspection_inventory_truncated".into());
    }
    loop {
        result.unknown_reasons.sort();
        result.unknown_reasons.dedup();
        result.complete = result.unknown_reasons.is_empty();
        result.recommended_actions = if result.complete {
            Vec::new()
        } else {
            vec![AcceptanceAction::CaptureContext]
        };
        result.coverage.symbols_returned = result.symbols.len();
        result.coverage.verification_returned = result.mapped_verification.len();
        if serde_json::to_vec(result)?.len() <= BYTE_BUDGET {
            break;
        }
        result.truncated = true;
        result.unknown_reasons.push("inspection_byte_budget".into());
        if !result.symbols.is_empty() {
            result.symbols.pop();
        } else if !result.mapped_verification.is_empty() {
            result.mapped_verification.pop();
        } else if !result.coverage.unmapped_paths.is_empty() {
            result.coverage.unmapped_paths.pop();
        } else if !result.coverage.uncovered_paths.is_empty() {
            result.coverage.uncovered_paths.pop();
        } else {
            anyhow::bail!("acceptance inspection fixed metadata exceeds byte budget");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/acceptance_context.rs"]
mod tests;
