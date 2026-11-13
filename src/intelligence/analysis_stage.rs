use super::*;

pub(super) fn advanced_stage_language_target(
    language: crate::semantic_provider::SemanticLanguage,
    registry: &StageExecutorRegistry,
) -> bool {
    if !matches!(
        language,
        crate::semantic_provider::SemanticLanguage::Css
            | crate::semantic_provider::SemanticLanguage::Html
    ) {
        return true;
    }
    registry.executors.iter().any(|executor| {
        executor.spec.languages.is_empty() || executor.spec.languages.contains(&language)
    })
}

pub(in crate::intelligence) fn required_verification_stages(
    profile: &VerificationProfile,
) -> Vec<VerificationStage> {
    let mut required = Vec::new();
    if profile.require_property {
        required.push(VerificationStage::Property);
    }
    if profile.require_mutation {
        required.push(VerificationStage::Mutation);
    }
    if profile.require_fuzz {
        required.push(VerificationStage::Fuzz);
    }
    if profile
        .deterministic_checks
        .iter()
        .any(|check| check == "runtime-gate")
    {
        required.push(VerificationStage::RuntimeCanary);
    }
    required
}

pub(in crate::intelligence) fn verification_automation_gaps(
    profile: &VerificationProfile,
    registry: &StageExecutorRegistry,
    targets: &[String],
) -> Vec<String> {
    let mut missing = Vec::new();
    for stage in required_verification_stages(profile) {
        let stage_key = format!("{stage:?}").to_ascii_lowercase();
        if targets.is_empty() {
            if !registry
                .executors
                .iter()
                .any(|executor| executor.available && executor.spec.stage == stage)
            {
                missing.push(stage_key);
            }
            continue;
        }
        for target in targets {
            if !stage_executor::stage_target_available(registry, stage, target) {
                missing.push(format!("{stage_key}:{target}"));
            }
        }
    }
    missing
}

pub(in crate::intelligence) fn append_verification_automation_gap(
    workspace: &str,
    profile: &VerificationProfile,
    registry: &StageExecutorRegistry,
    targets: &[String],
    risks: &mut Vec<Risk>,
) {
    let missing = verification_automation_gaps(profile, registry, targets);
    if missing.is_empty() {
        return;
    }

    let mut signals = vec!["local-verification-automation-gap".to_owned()];
    signals.extend(
        missing
            .iter()
            .map(|gap| format!("missing-local-stage-target:{gap}")),
    );
    signals.extend(
        targets
            .iter()
            .map(|target| format!("verification-target:{target}")),
    );
    signals.truncate(32);
    push_valid_risk(
        risks,
        Risk {
            id: stable_prefixed_id(
                "RISK",
                &format!(
                    "verification-automation-gap:{workspace}:{}",
                    missing.join(",")
                ),
            ),
            subject: format!("workspace:{workspace}"),
            category: RiskCategory::VerificationGap,
            level: profile.level,
            summary: bounded_message(&format!(
                "Risk-adaptive verification has no runnable local executor for required stage/target coverage: {}. Run a configured native stage executor; submitted stage reports remain advisory.",
                missing.join(", ")
            )),
            signals,
            guards: Vec::new(),
        },
    );
}

pub(in crate::intelligence) fn build_impact_analysis(
    workspace: String,
    state: &design::DesignState,
    review: &ChangeReviewReport,
    risk_level: RiskLevel,
    graph: Option<&SoftwareGraphSnapshot>,
) -> ImpactAnalysis {
    let changed_paths = review
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let changed = changed_paths.iter().cloned().collect::<HashSet<_>>();
    let design_changed = changed_paths.iter().any(|path| is_design_path(path));
    let mut impacted_components = BTreeSet::new();
    let mut impacted_requirements = BTreeSet::new();
    let mut impacted_acceptance = BTreeSet::new();
    let mut impacted_symbols = BTreeSet::new();
    let (transitive_paths, transitive_symbols, transitive_callers) = graph
        .map(|graph| transitive_call_impact(graph, &changed))
        .unwrap_or_default();
    impacted_symbols.extend(transitive_symbols);

    for component in state.components.values() {
        let implementation_hit = component.implementation.iter().any(|reference| {
            changed.contains(reference.path()) || transitive_paths.contains(reference.path())
        });
        if design_changed || implementation_hit {
            impacted_components.insert(component.id.clone());
        }
        if design_changed || implementation_hit {
            for reference in &component.implementation {
                if design_changed
                    || changed.contains(reference.path())
                    || transitive_paths.contains(reference.path())
                {
                    if let CodeRef::Symbol { path, symbol } = reference {
                        impacted_symbols.insert(format!("{path}::{symbol}"));
                    }
                }
            }
        }
    }

    for requirement in state.requirements.values() {
        if design_changed
            || requirement
                .implemented_by
                .iter()
                .any(|component| impacted_components.contains(component))
        {
            impacted_requirements.insert(requirement.id.clone());
            impacted_acceptance.extend(requirement.acceptance.iter().cloned());
        }
    }
    if design_changed {
        impacted_acceptance.extend(state.acceptance.keys().cloned());
    }

    let public_api = changed_paths.iter().any(|path| public_api_path(path))
        || transitive_paths.iter().any(|path| public_api_path(path))
        || impacted_requirements.iter().any(|id| {
            state.requirements.get(id).is_some_and(|requirement| {
                let text =
                    format!("{} {}", requirement.title, requirement.intent).to_ascii_lowercase();
                text.contains("public api") || text.contains("protocol")
            })
        });
    let security_boundary = changed_paths
        .iter()
        .chain(transitive_paths.iter())
        .any(|path| security_boundary_path(path))
        || impacted_requirements.iter().any(|id| {
            state.requirements.get(id).is_some_and(|requirement| {
                requirement.risk.security.is_some_and(|level| {
                    matches!(level, design::RiskLevel::High | design::RiskLevel::Critical)
                })
            })
        });

    ImpactAnalysis {
        workspace,
        changed_paths,
        impacted_components: impacted_components.into_iter().take(512).collect(),
        impacted_requirements: impacted_requirements.into_iter().take(512).collect(),
        impacted_acceptance: impacted_acceptance.into_iter().take(512).collect(),
        impacted_symbols: impacted_symbols
            .into_iter()
            .take(MAX_TRANSITIVE_IMPACT_SYMBOLS)
            .collect(),
        transitive_callers,
        graph_provider: graph
            .map(|snapshot| snapshot.provider.clone())
            .unwrap_or_else(|| "none".to_owned()),
        graph_precision: graph
            .map(|snapshot| format!("{:?}", snapshot.precision).to_ascii_lowercase())
            .unwrap_or_else(|| "none".to_owned()),
        graph_truncated: graph
            .is_some_and(|snapshot| snapshot.truncated || snapshot.scan_truncated),
        public_api,
        security_boundary,
        risk_level,
    }
}

pub(in crate::intelligence) fn transitive_call_impact(
    snapshot: &SoftwareGraphSnapshot,
    changed_paths: &HashSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>, usize) {
    let initial = snapshot
        .graph
        .nodes
        .values()
        .filter(|node| node.kind != NodeKind::File)
        .filter(|node| {
            node.attributes
                .get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| changed_paths.contains(path))
        })
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let initial_count = initial.len();
    let mut impacted = initial.clone();
    let mut frontier = initial;

    while !frontier.is_empty() && impacted.len() < MAX_TRANSITIVE_IMPACT_SYMBOLS {
        let mut next = BTreeSet::new();
        for edge in &snapshot.graph.edges {
            if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::RuntimeCalls)
                || !frontier.contains(&edge.to)
                || impacted.contains(&edge.from)
            {
                continue;
            }
            next.insert(edge.from.clone());
            if impacted.len().saturating_add(next.len()) >= MAX_TRANSITIVE_IMPACT_SYMBOLS {
                break;
            }
        }
        if next.is_empty() {
            break;
        }
        impacted.extend(next.iter().cloned());
        frontier = next;
    }

    let mut paths = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    for id in &impacted {
        let Some(node) = snapshot.graph.nodes.get(id) else {
            continue;
        };
        let Some(path) = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        paths.insert(path.to_owned());
        let name = node
            .attributes
            .get("qualified_name")
            .or_else(|| node.attributes.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&node.label);
        symbols.insert(format!("{path}::{name}"));
    }
    let transitive_callers = impacted.len().saturating_sub(initial_count);
    (paths, symbols, transitive_callers)
}

pub(in crate::intelligence) fn design_changes_from_review(
    review: &ChangeReviewReport,
) -> Vec<DesignChange> {
    review
        .files
        .iter()
        .filter(|file| is_design_path(&file.path))
        .take(128)
        .map(|file| DesignChange {
            subject: file.path.clone(),
            kind: match file.status.as_str() {
                "added" | "untracked" => DesignChangeKind::Added,
                "modified" | "renamed" | "copied" => DesignChangeKind::Modified,
                "deleted" => DesignChangeKind::Removed,
                _ => DesignChangeKind::Unknown,
            },
            summary: format!(
                "Design State file is {} in the current change set.",
                file.status
            ),
        })
        .collect()
}

pub(in crate::intelligence) fn workspace_revision_from_design_state(
    workspace: &Workspace,
    design_initialized: bool,
) -> Result<Revision> {
    #[cfg(test)]
    REVISION_CALLS.with(|count| count.set(count.get() + 1));

    // Most repositories can derive both code and Design revisions from one
    // bounded tree walk. If the root scan truncates, preserve the historical
    // Design precision by performing one dedicated .wcode scan instead of
    // guessing from an incomplete shared snapshot.
    let (paths, code_truncated) = revision_paths(workspace, ".")?;
    let code = workspace_tree_revision_from_paths(workspace, &paths, false, code_truncated)?;
    let design_revision = if design_initialized {
        if code_truncated {
            Some(workspace_tree_revision(workspace, true)?)
        } else {
            Some(workspace_tree_revision_from_paths(
                workspace, &paths, true, false,
            )?)
        }
    } else {
        None
    };
    Ok(Revision {
        design: design_revision,
        code,
    })
}

pub(super) fn workspace_tree_revision(workspace: &Workspace, design_only: bool) -> Result<String> {
    let root = if design_only { ".wcode" } else { "." };
    let (paths, truncated) = revision_paths(workspace, root)?;
    workspace_tree_revision_from_paths(workspace, &paths, design_only, truncated)
}
