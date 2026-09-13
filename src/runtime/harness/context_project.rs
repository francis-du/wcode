use super::*;

pub(super) fn compact_checks(profile: &ProjectProfile, limit: usize) -> Vec<Value> {
    profile
        .recommended_checks
        .iter()
        .take(limit)
        .map(|check| {
            let mut value = json!({
                "id": check.id,
                "level": check.level,
                "phase": check.phase,
                "command": format_command(&check.program, &check.args),
            });
            if check.cwd != "." {
                value["cwd"] = json!(check.cwd);
                value["island"] = json!(check.island);
                value["languages"] = json!(check.languages);
            }
            value
        })
        .collect()
}

pub(super) fn compact_contracts(profile: &ProjectProfile) -> Value {
    json!({
        "bridges": profile.contracts.bridges.iter().take(8).map(|bridge| json!({
            "kind": bridge.kind,
            "source": bridge.source,
            "consumer": bridge.consumer_island,
            "provider": bridge.provider,
            "precision": bridge.precision,
        })).collect::<Vec<_>>(),
        "diagnostics": profile.contracts.diagnostics.iter().take(4).map(|diagnostic| json!({
            "path": diagnostic.path,
            "reason": diagnostic.reason,
        })).collect::<Vec<_>>(),
        "truncated": profile.contracts.truncated,
    })
}

pub(super) fn compact_islands(profile: &ProjectProfile) -> Vec<Value> {
    if profile.islands.len() <= 1 {
        return Vec::new();
    }
    profile
        .islands
        .iter()
        .take(12)
        .map(|island| {
            json!({
                "root": island.root,
                "types": island.project_types,
                "languages": island.languages,
                "manifests": island.manifests.len(),
                "checks": island.check_ids.len(),
                "depends_on": island.dependencies.iter().take(8).map(|dependency| dependency.island.as_str()).collect::<Vec<_>>(),
                "verification": island.verification_status,
                "verification_gaps": island.verification_gaps,
                "provider": island.provider,
                "precision": island.precision,
            })
        })
        .collect()
}
