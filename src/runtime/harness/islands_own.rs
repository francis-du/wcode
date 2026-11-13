use super::*;

pub(super) fn island_owns_path(root: &str, path: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) const MAX_VERIFICATION_IMPACT_REASONS: usize = 128;

#[cfg(test)]
pub(crate) fn verification_checks_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
    level: &str,
) -> Vec<CheckSpec> {
    let impact = verification_impact_for_snapshot(profile, snapshot);
    verification_checks_for_impact(profile, &impact, level)
}

pub(crate) fn verification_checks_for_impact(
    profile: &ProjectProfile,
    impact: &ProjectVerificationImpact,
    level: &str,
) -> Vec<CheckSpec> {
    let targets_island = |island: &str| {
        !impact.selective
            || impact
                .affected_islands
                .binary_search_by(|candidate| candidate.as_str().cmp(island))
                .is_ok()
    };
    let mut plan = profile
        .recommended_checks
        .iter()
        .filter(|check| level == "full" || check.level == "quick")
        .filter(|check| check.island == "workspace" || targets_island(&check.island))
        .cloned()
        .collect::<Vec<_>>();

    if level == "quick" {
        // Quick verification must still execute a real project-owned gate for
        // every affected language island. If an ecosystem exposes only a full
        // gate (for example pytest), promote one bounded full check instead of
        // silently treating the workspace-level diff check as sufficient.
        for island in profile
            .islands
            .iter()
            .filter(|island| targets_island(&island.id))
        {
            for project_type in &island.project_types {
                let Some(prefix) = project_type_check_prefix(project_type) else {
                    continue;
                };
                let has_quick = profile.recommended_checks.iter().any(|check| {
                    check.island == island.id
                        && check.level == "quick"
                        && check_id_has_prefix(check, prefix)
                });
                if has_quick {
                    continue;
                }
                if let Some(mut fallback) = profile
                    .recommended_checks
                    .iter()
                    .filter(|check| {
                        check.island == island.id
                            && check.level == "full"
                            && check_id_has_prefix(check, prefix)
                    })
                    .min_by(|left, right| {
                        left.phase
                            .cmp(&right.phase)
                            .then_with(|| left.id.cmp(&right.id))
                    })
                    .cloned()
                {
                    fallback.reason = format!(
                        "Quick fallback for {project_type}: no dedicated quick gate is declared; {}",
                        fallback.reason
                    );
                    if !plan.iter().any(|check| check.id == fallback.id) {
                        plan.push(fallback);
                    }
                }
            }
        }
    }

    sort_checks(&mut plan);
    plan
}

#[cfg(test)]
pub(crate) fn verification_gaps_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
    level: &str,
) -> Vec<ProjectIslandVerificationGap> {
    let impact = verification_impact_for_snapshot(profile, snapshot);
    verification_gaps_for_impact(profile, &impact, level)
}

pub(crate) fn verification_gaps_for_impact(
    profile: &ProjectProfile,
    impact: &ProjectVerificationImpact,
    level: &str,
) -> Vec<ProjectIslandVerificationGap> {
    profile
        .islands
        .iter()
        .filter(|island| {
            island.project_types != ["generic"]
                && (!impact.selective || impact.affected_islands.binary_search(&island.id).is_ok())
        })
        .filter_map(|island| {
            let mut project_types = island
                .project_types
                .iter()
                .filter(|project_type| {
                    let Some(prefix) = project_type_check_prefix(project_type) else {
                        return true;
                    };
                    !project_type_has_level_coverage(profile, &island.id, prefix, level)
                })
                .cloned()
                .collect::<Vec<_>>();
            project_types.sort();
            project_types.dedup();
            (!project_types.is_empty()).then(|| ProjectIslandVerificationGap {
                island: island.id.clone(),
                root: island.root.clone(),
                project_types,
                level: level.to_owned(),
            })
        })
        .collect()
}

pub(crate) fn verification_impact_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
) -> ProjectVerificationImpact {
    let Some(snapshot) = snapshot else {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_unavailable");
    };
    if snapshot.get("available").and_then(Value::as_bool) != Some(true) {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_unavailable");
    }
    if snapshot.get("truncated").and_then(Value::as_bool) == Some(true) {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_truncated");
    }
    let Some(files) = snapshot.get("files").and_then(Value::as_array) else {
        return broad_verification_impact(profile, "worktree_snapshot", "changed_files_missing");
    };
    if files.is_empty() {
        return broad_verification_impact(profile, "worktree_snapshot", "changed_files_empty");
    }

    let mut affected = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    for path in files
        .iter()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
    {
        let mut matched = false;
        if let Some(island) = owning_island(&profile.islands, path) {
            matched = true;
            affected.insert(island.id.clone());
            reasons.insert(ProjectVerificationImpactReason {
                island: island.id.clone(),
                kind: "direct_change",
                source: path.to_owned(),
                relationship: "manifest_ownership".to_owned(),
                evidence: format!("nearest manifest root {}", island.root),
                provider: island.provider,
                precision: island.precision,
            });
        }
        for bridge in super::contracts::matching_contract_bridges(&profile.contracts, path) {
            matched = true;
            affected.insert(bridge.consumer_island.clone());
            reasons.insert(ProjectVerificationImpactReason {
                island: bridge.consumer_island.clone(),
                kind: "contract_bridge",
                source: path.to_owned(),
                relationship: bridge.kind.to_owned(),
                evidence: bridge.evidence.clone(),
                provider: bridge.provider,
                precision: bridge.precision,
            });
        }
        if !matched {
            return broad_verification_impact(profile, path, "unresolved_change_ownership");
        }
    }

    propagate_dependents_with_provenance(&profile.islands, &mut affected, &mut reasons);
    let truncated = reasons.len() > MAX_VERIFICATION_IMPACT_REASONS;
    let mut reasons = reasons.into_iter().collect::<Vec<_>>();
    reasons.truncate(MAX_VERIFICATION_IMPACT_REASONS);
    ProjectVerificationImpact {
        selective: true,
        affected_islands: affected.into_iter().collect(),
        reasons,
        truncated,
        provider: "workspace-impact",
        precision: "structural",
    }
}
