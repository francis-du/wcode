use super::*;
use crate::intelligence_types::DriftDeviation;
use crate::resource::ProcessQueueSnapshot;

const PARALLEL_FIRST_CONSTRAINT: &str = "CONSTRAINT-PARALLEL-FIRST";
const RECENT_RUNTIME_WAIT_MS: u64 = 60_000;

pub(super) fn findings(state: &design::DesignState) -> Vec<DriftFinding> {
    let Some(constraint) = state.constraints.get(PARALLEL_FIRST_CONSTRAINT) else {
        return Vec::new();
    };
    let snapshot = crate::resource::snapshot();
    let Some(deviation) = queue_wait_deviation(&snapshot.child_queue) else {
        return Vec::new();
    };
    let affected_requirements = state
        .requirements
        .values()
        .filter(|requirement| {
            requirement
                .constraints
                .iter()
                .any(|id| id == PARALLEL_FIRST_CONSTRAINT)
        })
        .take(16)
        .map(|requirement| requirement.id.clone())
        .collect();
    let message = format!(
        "Recent process admission exceeded the designed queue-wait bound: last wait {} ms exceeds {} ms while outer tool capacity is {} and the child-process queue is {}/{} active with {} waiting (lifetime peak {} ms). This runtime observation is not source-revision-bound, so it is an explicit advisory Runtime Drift signal rather than deterministic source proof.",
        snapshot.child_queue.last_wait_ms,
        crate::resource::PROCESS_QUEUE_WAIT_CAP.as_millis(),
        snapshot.effective_parallel_tools,
        snapshot.child_queue.active,
        snapshot.child_queue.limit,
        snapshot.child_queue.waiting,
        snapshot.child_queue.max_wait_ms
    );
    vec![DriftFinding {
        id: stable_prefixed_id(
            "DRIFT",
            &format!(
                "runtime-process-queue:{}:{}",
                snapshot.child_queue.limit, snapshot.child_queue.last_wait_ms
            ),
        ),
        kind: DriftKind::RuntimeDrift,
        risk_level: RiskLevel::Medium,
        subject: constraint.id.clone(),
        message,
        affected_requirements,
        paths: Vec::new(),
        deviation: Some(deviation),
    }]
}

pub(super) fn counts(findings: &[DriftFinding]) -> (usize, usize, usize) {
    let implementation = findings
        .iter()
        .filter(|finding| finding.kind == DriftKind::ImplementationDrift)
        .count();
    let design = findings
        .iter()
        .filter(|finding| finding.kind == DriftKind::DesignDrift)
        .count();
    let runtime = findings
        .iter()
        .filter(|finding| finding.kind == DriftKind::RuntimeDrift)
        .count();
    (implementation, design, runtime)
}

fn queue_wait_deviation(queue: &ProcessQueueSnapshot) -> Option<DriftDeviation> {
    let expected_max =
        u64::try_from(crate::resource::PROCESS_QUEUE_WAIT_CAP.as_millis()).unwrap_or(u64::MAX);
    let recent = queue
        .last_wait_age_ms
        .is_some_and(|age| age <= RECENT_RUNTIME_WAIT_MS);
    if !recent || queue.last_wait_ms <= expected_max {
        return None;
    }
    let excess = queue.last_wait_ms.saturating_sub(expected_max);
    let deviation_percent = (excess as f64 / expected_max as f64) * 100.0;
    Some(DriftDeviation {
        metric: "child_process_queue_wait".into(),
        expected_max: expected_max as f64,
        observed: queue.last_wait_ms as f64,
        deviation_percent,
        unit: "ms".into(),
        precision: "runtime_observed".into(),
        revision_bound: false,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/runtime_drift.rs"]
mod tests;
