use super::*;

pub(super) fn build_drift_status(
    workspace: String,
    state: &design::DesignState,
    traceability: &TraceabilityStatus,
    review: &ChangeReviewReport,
) -> DriftStatus {
    let design_changed = review.files.iter().any(|file| is_design_path(&file.path));
    let changed_actual_paths = review
        .files
        .iter()
        .filter(|file| is_actual_state_change(file))
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let implementation_changed = !changed_actual_paths.is_empty();
    let mut findings = Vec::new();

    for requirement in traceability
        .requirements
        .iter()
        .filter(|requirement| requirement.status != RequirementTraceStatus::Complete)
    {
        let mut paths = requirement
            .implementation
            .iter()
            .chain(&requirement.verification)
            .filter(|reference| !reference.resolved)
            .filter_map(|reference| trace_target_path(&reference.target))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        paths.truncate(16);
        let message = match requirement.status {
            RequirementTraceStatus::Missing => {
                "Declared requirement has no complete implementation/verification trace."
            }
            RequirementTraceStatus::Partial => {
                "Declared requirement has unresolved implementation or verification references."
            }
            RequirementTraceStatus::Complete => continue,
        };
        findings.push(DriftFinding {
            id: stable_prefixed_id("DRIFT", &format!("trace:{}:{message}", requirement.id)),
            kind: DriftKind::ImplementationDrift,
            risk_level: requirement_risk_level(state.requirements.get(&requirement.id)),
            subject: requirement.id.clone(),
            message: message.to_owned(),
            affected_requirements: vec![requirement.id.clone()],
            paths,
        });
        if findings.len() >= MAX_DRIFT_FINDINGS {
            break;
        }
    }

    if findings.len() < MAX_DRIFT_FINDINGS && design_changed && !implementation_changed {
        let mut affected_requirements = state.requirements.keys().cloned().collect::<Vec<_>>();
        affected_requirements.truncate(32);
        findings.push(DriftFinding {
            id: stable_prefixed_id("DRIFT", "desired-state-changed-without-actual-state-change"),
            kind: DriftKind::ImplementationDrift,
            risk_level: RiskLevel::High,
            subject: "design:desired-state".into(),
            message: "Design State changed without a corresponding non-design Actual State change; implementation may still reflect the previous desired state.".into(),
            affected_requirements,
            paths: review
                .files
                .iter()
                .filter(|file| is_design_path(&file.path))
                .take(16)
                .map(|file| file.path.clone())
                .collect(),
        });
    }

    if findings.len() < MAX_DRIFT_FINDINGS && implementation_changed && !design_changed {
        for path in &changed_actual_paths {
            let (components, requirements) = design_impact_for_path(state, path);
            if components.is_empty() && requirements.is_empty() {
                continue;
            }
            let risk_level = requirements
                .iter()
                .filter_map(|id| state.requirements.get(id))
                .map(|requirement| requirement_risk_level(Some(requirement)))
                .max()
                .unwrap_or(RiskLevel::Medium);
            let subject = if components.len() == 1 {
                components[0].clone()
            } else {
                format!("implementation:{path}")
            };
            findings.push(DriftFinding {
                id: stable_prefixed_id("DRIFT", &format!("actual-state:{path}")),
                kind: DriftKind::DesignDrift,
                risk_level,
                subject,
                message: "Design-mapped Actual State changed without a corresponding Design State change; confirm this is an implementation-only refactor or update the design intent.".into(),
                affected_requirements: requirements,
                paths: vec![path.clone()],
            });
            if findings.len() >= MAX_DRIFT_FINDINGS {
                break;
            }
        }
    }

    let implementation_drift = findings
        .iter()
        .filter(|finding| finding.kind == DriftKind::ImplementationDrift)
        .count();
    let design_drift = findings
        .iter()
        .filter(|finding| finding.kind == DriftKind::DesignDrift)
        .count();
    let truncated = findings.len() >= MAX_DRIFT_FINDINGS
        || traceability.requirements_total > traceability.requirements_returned;

    DriftStatus {
        workspace,
        design_changed,
        implementation_changed,
        implementation_drift,
        design_drift,
        findings,
        truncated,
    }
}

pub(super) fn assess_risk(
    workspace: &str,
    review: &ChangeReviewReport,
    traceability: &TraceabilityStatus,
    drift: &DriftStatus,
) -> (RiskLevel, Vec<Risk>) {
    let mut risks = Vec::new();
    let mut overall = match review.risk_level.as_str() {
        "high" => RiskLevel::High,
        "moderate" => RiskLevel::Medium,
        _ => RiskLevel::Low,
    };

    for finding in review.findings.iter().take(64) {
        let level = match finding.severity.as_str() {
            "error" | "high" => RiskLevel::High,
            "warning" => RiskLevel::Medium,
            _ => RiskLevel::Low,
        };
        let subject = finding
            .paths
            .first()
            .cloned()
            .unwrap_or_else(|| format!("change:{workspace}"));
        let mut signals = vec![finding.code.clone()];
        signals.extend(
            finding
                .paths
                .iter()
                .take(8)
                .map(|path| format!("path:{path}")),
        );
        push_valid_risk(
            &mut risks,
            Risk {
                id: stable_prefixed_id("RISK", &format!("review:{}:{subject}", finding.code)),
                subject,
                category: review_risk_category(&finding.code),
                level,
                summary: bounded_message(&finding.message),
                signals,
                guards: Vec::new(),
            },
        );
        overall = overall.max(level);
    }

    for finding in &drift.findings {
        let category = match finding.kind {
            DriftKind::ImplementationDrift => RiskCategory::VerificationGap,
            DriftKind::DesignDrift => RiskCategory::Architecture,
        };
        let mut signals = vec![match finding.kind {
            DriftKind::ImplementationDrift => "implementation-drift".to_owned(),
            DriftKind::DesignDrift => "design-drift".to_owned(),
        }];
        signals.extend(
            finding
                .paths
                .iter()
                .take(8)
                .map(|path| format!("path:{path}")),
        );
        push_valid_risk(
            &mut risks,
            Risk {
                id: stable_prefixed_id("RISK", &finding.id),
                subject: finding.subject.clone(),
                category,
                level: finding.risk_level,
                summary: bounded_message(&finding.message),
                signals,
                guards: Vec::new(),
            },
        );
        overall = overall.max(finding.risk_level);
    }

    if !traceability.initialized {
        let level = RiskLevel::Medium;
        push_valid_risk(
            &mut risks,
            Risk {
                id: stable_prefixed_id("RISK", &format!("design-uninitialized:{workspace}")),
                subject: format!("workspace:{workspace}"),
                category: RiskCategory::Architecture,
                level,
                summary: "Structured Design State is not initialized, so requirement-to-implementation intent cannot be proven.".into(),
                signals: vec!["design-state-uninitialized".into()],
                guards: Vec::new(),
            },
        );
        overall = overall.max(level);
    } else if !traceability.valid_design {
        let level = RiskLevel::High;
        push_valid_risk(
            &mut risks,
            Risk {
                id: stable_prefixed_id("RISK", &format!("design-invalid:{workspace}")),
                subject: format!("workspace:{workspace}"),
                category: RiskCategory::Architecture,
                level,
                summary: "Structured Design State is invalid; drift and impact results may be incomplete until design diagnostics are resolved.".into(),
                signals: vec!["invalid-design-state".into()],
                guards: Vec::new(),
            },
        );
        overall = overall.max(level);
    }

    if traceability.initialized
        && (traceability.design_to_implementation.percent < 100
            || traceability.acceptance_to_verification.percent < 100)
    {
        let level = if traceability.missing_requirements > 0 {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        };
        push_valid_risk(
            &mut risks,
            Risk {
                id: stable_prefixed_id("RISK", &format!("traceability-gap:{workspace}")),
                subject: format!("workspace:{workspace}"),
                category: RiskCategory::VerificationGap,
                level,
                summary: format!(
                    "Traceability is incomplete: implementation {}%, acceptance verification {}%.",
                    traceability.design_to_implementation.percent,
                    traceability.acceptance_to_verification.percent
                ),
                signals: vec![
                    format!(
                        "implementation-coverage:{}",
                        traceability.design_to_implementation.percent
                    ),
                    format!(
                        "acceptance-coverage:{}",
                        traceability.acceptance_to_verification.percent
                    ),
                ],
                guards: Vec::new(),
            },
        );
        overall = overall.max(level);
    }

    risks.truncate(128);
    (overall, risks)
}

pub(super) fn build_impact_analysis(
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

pub(super) fn transitive_call_impact(
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

pub(super) fn design_changes_from_review(review: &ChangeReviewReport) -> Vec<DesignChange> {
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

pub(super) fn workspace_revision(workspace: &Workspace) -> Result<Revision> {
    let load = design::load_design(workspace)?;
    let design_revision = if load.initialized {
        Some(workspace_tree_revision(workspace, true)?)
    } else {
        None
    };
    Ok(Revision {
        design: design_revision,
        code: workspace_tree_revision(workspace, false)?,
    })
}

pub(super) fn workspace_tree_revision(workspace: &Workspace, design_only: bool) -> Result<String> {
    let root = if design_only { ".wcode" } else { "." };
    let (mut paths, truncated) = workspace.source_files(root, MAX_REVISION_FILES)?;
    paths.sort();
    let mut hasher = Sha256::new();
    let mut included = 0usize;

    for path in paths {
        let include = if design_only {
            path == design::PROJECT_FILE
                || path == design::DESIGN_ROOT
                || path.starts_with(&format!("{}/", design::DESIGN_ROOT))
        } else {
            !path.starts_with(".wcode/") && path != ".wcode"
        };
        if !include {
            continue;
        }
        let before = workspace.source_stamp(&path)?;
        let bytes = std::fs::read(workspace.root().join(&path))?;
        let after = workspace.source_stamp(&path)?;
        if before != after {
            return Err(anyhow!(
                "workspace changed while computing software revision; retry the request"
            ));
        }
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        included = included.saturating_add(1);
    }
    hasher.update(format!("files={included};truncated={truncated}").as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    Ok(if truncated {
        format!("sha256:{digest}:partial")
    } else {
        format!("sha256:{digest}")
    })
}

pub(super) fn evidence_kind_for_check(check_id: &str) -> EvidenceKind {
    let lower = check_id.to_ascii_lowercase();
    if lower.contains("test") {
        EvidenceKind::IntegrationTest
    } else if lower.contains("clippy")
        || lower.contains("lint")
        || lower.contains("format")
        || lower.contains("diff")
    {
        EvidenceKind::StaticAnalysis
    } else if lower.contains("build") || lower.contains("check") || lower.contains("compile") {
        EvidenceKind::Compiler
    } else {
        EvidenceKind::StaticAnalysis
    }
}

pub(super) fn verification_reference_outcome(
    reference: &VerificationRef,
    report: &VerificationReport,
) -> Option<bool> {
    match reference {
        VerificationRef::Check { id } => report
            .checks
            .iter()
            .find(|check| check.id == *id)
            .map(|check| check.success),
        VerificationRef::Test { .. } => {
            let tests = report
                .checks
                .iter()
                .filter(|check| check.id.to_ascii_lowercase().contains("test"))
                .collect::<Vec<_>>();
            (!tests.is_empty()).then(|| tests.iter().all(|check| check.success))
        }
    }
}

pub(super) fn push_evidence(
    records: &mut Vec<StoredEvidence>,
    workspace: &str,
    evidence: Evidence,
) {
    if records.len() >= MAX_EVIDENCE_RECORDS {
        let overflow = records
            .len()
            .saturating_add(1)
            .saturating_sub(MAX_EVIDENCE_RECORDS);
        records.drain(..overflow);
    }
    records.push(StoredEvidence {
        workspace: workspace.to_owned(),
        evidence,
    });
}

pub(super) fn digest_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn stable_prefixed_id(prefix: &str, value: &str) -> String {
    let digest = digest_text(value);
    format!("{prefix}-{}", &digest[..12])
}

pub(super) fn push_valid_risk(risks: &mut Vec<Risk>, risk: Risk) {
    if risks.len() < 128 && risk.validate().is_ok() {
        risks.push(risk);
    }
}

pub(super) fn review_risk_category(code: &str) -> RiskCategory {
    let lower = code.to_ascii_lowercase();
    if lower.contains("security") || lower.contains("auth") {
        RiskCategory::Security
    } else if lower.contains("manifest") || lower.contains("dependency") {
        RiskCategory::Dependency
    } else if lower.contains("migration") {
        RiskCategory::Migration
    } else if lower.contains("test") || lower.contains("coverage") {
        RiskCategory::VerificationGap
    } else if lower.contains("diff") || lower.contains("build") || lower.contains("format") {
        RiskCategory::Build
    } else if lower.contains("large")
        || lower.contains("architecture")
        || lower.contains("maintainability")
    {
        RiskCategory::Architecture
    } else {
        RiskCategory::Reliability
    }
}

pub(super) fn is_design_path(path: &str) -> bool {
    path == design::PROJECT_FILE
        || path == design::DESIGN_ROOT
        || path.starts_with(&format!("{}/", design::DESIGN_ROOT))
}

pub(super) fn is_actual_state_change(file: &crate::harness::ChangedFileReview) -> bool {
    !is_design_path(&file.path)
        && matches!(
            file.category.as_str(),
            "source" | "test" | "config" | "manifest" | "workflow" | "migration"
        )
}

pub(super) fn design_impact_for_path(
    state: &design::DesignState,
    path: &str,
) -> (Vec<String>, Vec<String>) {
    let components = state
        .components
        .values()
        .filter(|component| {
            component
                .implementation
                .iter()
                .any(|reference| reference.path() == path)
        })
        .map(|component| component.id.clone())
        .collect::<BTreeSet<_>>();
    let requirements = state
        .requirements
        .values()
        .filter(|requirement| {
            requirement
                .implemented_by
                .iter()
                .any(|component| components.contains(component))
        })
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    (
        components.into_iter().collect(),
        requirements.into_iter().collect(),
    )
}

pub(super) fn trace_target_path(target: &str) -> Option<String> {
    let candidate = target.split("::").next().unwrap_or(target);
    (candidate.contains('/') || candidate.contains('.')).then(|| candidate.to_owned())
}

pub(super) fn requirement_risk_level(requirement: Option<&design::Requirement>) -> RiskLevel {
    let Some(requirement) = requirement else {
        return RiskLevel::Medium;
    };
    let mut level = match requirement.priority {
        Priority::Critical => RiskLevel::High,
        Priority::High => RiskLevel::Medium,
        Priority::Medium | Priority::Low => RiskLevel::Low,
    };
    for declared in [
        requirement.risk.security,
        requirement.risk.compatibility,
        requirement.risk.performance,
        requirement.risk.reliability,
    ]
    .into_iter()
    .flatten()
    {
        level = level.max(match declared {
            design::RiskLevel::Low => RiskLevel::Low,
            design::RiskLevel::Medium => RiskLevel::Medium,
            design::RiskLevel::High => RiskLevel::High,
            design::RiskLevel::Critical => RiskLevel::Critical,
        });
    }
    level
}

pub(super) fn public_api_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "src/lib.rs"
        || lower.contains("/api/")
        || lower.contains("protocol")
        || lower.contains("schema")
        || lower.starts_with("include/")
}

pub(super) fn security_boundary_path(path: &str) -> bool {
    path.to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            matches!(
                token,
                "auth"
                    | "authentication"
                    | "authorization"
                    | "oauth"
                    | "token"
                    | "permission"
                    | "security"
                    | "crypto"
                    | "workspace"
                    | "command"
                    | "process"
                    | "sandbox"
                    | "exec"
            )
        })
}
