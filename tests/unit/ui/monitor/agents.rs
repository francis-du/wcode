use super::*;
use ratatui::backend::TestBackend;

fn render(stats: &IntelligenceStats, focus: usize, scroll: usize) -> String {
    let mut terminal = Terminal::new(TestBackend::new(90, 18)).unwrap();
    let ui = DashboardState {
        console_focus: focus,
        console_scroll: scroll,
        ..Default::default()
    };
    terminal
        .draw(|frame| render_agent_console(frame, frame.area(), stats, &ui))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn agent_projection_excludes_claim_capabilities_and_bounds_items() {
    let items = (0..24).map(|index| serde_json::json!({
        "id": format!("lane-{index}"), "title": "Work", "status": "in_progress", "write_paths": ["src/ui"],
        "claim": { "actor": "worker-one", "expires_at_ms": 1,
            "claim_id": "PRIVATE-CLAIM-CAPABILITY", "token": "PRIVATE-CLAIM-TOKEN", "state": "active" },
        "result": { "actor": "worker-one", "outcome": "complete", "summary": "Worker changed the view.",
            "claim_id": "PRIVATE-RESULT-CAPABILITY" }
    })).collect::<Vec<_>>();
    let projection = project_worklist_projection(
        &serde_json::json!({"exists": true, "revision": 7, "items": items}),
    );
    assert_eq!(projection["items"].as_array().unwrap().len(), 20);
    assert_eq!(projection["total"], 24);
    assert_eq!(projection["truncated"], true);
    let serialized = projection.to_string();
    assert!(!serialized.contains("PRIVATE-"));
    assert!(!serialized.contains("claim_id"));
    assert!(!serialized.contains("\"token\""));
    let stats = IntelligenceStats {
        project_worklist: Some(projection),
        ..Default::default()
    };
    let text = render(&stats, 0, 0);
    assert!(text.contains("worker-one"));
    assert!(text.contains("Scope src/ui"));
    assert!(text.contains("expired"));
    assert!(text.contains("truncated"));
}

#[test]
fn absent_agent_snapshot_is_unknown_and_worker_reports_are_not_proof() {
    let text = render(&IntelligenceStats::default(), 0, 0);
    assert!(text.contains("state unknown"));
    assert!(text.contains("not verification evidence"));
    let stats = IntelligenceStats {
        project_worklist: Some(project_worklist_projection(&serde_json::json!({
            "exists": true, "revision": 4, "items": [{
                "id": "ui", "title": "Improve view", "status": "done",
                "result": { "actor": "review-worker", "outcome": "complete", "summary": "Implemented the selected view." }
            }]
        }))),
        ..Default::default()
    };
    let text = render(&stats, 0, 0);
    assert!(text.contains("not Verification evidence"));
    assert!(text.contains("Reported result complete"));
    assert!(text.contains("Implemented the selected view"));
    assert!(!text.contains("verification ready"));
}

#[test]
fn agent_results_are_scrollable_and_no_worklist_is_explicit() {
    let stats = IntelligenceStats {
        project_worklist: Some(project_worklist_projection(&serde_json::json!({
            "exists": true, "revision": 9, "items": [{
                "id": "worker", "title": "Observe source", "status": "done",
                "result": { "actor": "worker", "outcome": "complete",
                    "summary": format!("START {} END_OF_WORKER_REPORT", "long result context ".repeat(40)) }
            }]
        }))),
        ..Default::default()
    };
    assert!(render(&stats, 0, 0).contains("START"));
    assert!(render(&stats, 0, usize::MAX).contains("END_OF_WORKER_REPORT"));
    let absent = IntelligenceStats {
        project_worklist: Some(project_worklist_projection(&serde_json::json!({
            "exists": false, "revision": 0, "items": []
        }))),
        ..Default::default()
    };
    assert!(render(&absent, 0, 0).contains("No Worklist observed"));
}

#[test]
fn agent_revision_and_object_evidence_keep_only_bounded_public_metadata() {
    let value = serde_json::json!({
        "exists": true, "revision": 9, "items": [{
            "id": "worker", "title": "Observe source", "status": "in_progress",
            "claim": { "actor": "worker", "base_revision": {
                "code": "code-base", "design": "design-base", "private": "PRIVATE-REVISION"
            }, "claim_id": "PRIVATE-CLAIM", "expires_at_ms": 1 },
            "result": {
                "actor": "worker", "outcome": "complete", "summary": "Reported source inspection.",
                "base_revision": { "code": "code-base", "design": "design-base" },
                "repository_revision": { "code": "code-current", "design": "design-current" },
                "proof_status": "current_references_only",
                "evidence": [{
                    "id": "EV-test", "kind": "verification", "result": "passed", "producer": "native-check",
                    "raw_record": "PRIVATE-EVIDENCE", "claim_id": "PRIVATE-REFERENCE"
                }]
            }
        }]
    });
    let projection = project_worklist_projection(&value);
    let item = &projection["items"][0];
    assert_eq!(item["claim"]["base_revision"]["code"], "code-base");
    assert_eq!(
        item["result"]["repository_revision"]["design"],
        "design-current"
    );
    assert_eq!(item["result"]["evidence_count"], 1);
    assert_eq!(item["result"]["evidence"][0]["id"], "EV-test");
    assert!(!projection.to_string().contains("PRIVATE-"));
    assert!(!projection.to_string().contains("raw_record"));
    let stats = IntelligenceStats {
        project_worklist: Some(projection),
        ..Default::default()
    };
    let text = render(&stats, 0, 0);
    assert!(text.contains("Claim base · code code-base · design design-base"));
    assert!(text.contains("Reported references 1"));
    assert!(render(&stats, 0, usize::MAX)
        .contains("Reference EV-test · verification · passed · native-check"));

    let bounded = project_worklist_projection(&serde_json::json!({
        "exists": true, "items": [{
            "claim": {"base_revision": {"code": "c".repeat(200), "design": "d".repeat(300)}},
            "result": {"evidence": (0..17).map(|_| serde_json::json!({"id":"EV"})).collect::<Vec<_>>()}
        }]
    }));
    assert_eq!(
        bounded["items"][0]["claim"]["base_revision"]["code"]
            .as_str()
            .unwrap()
            .len(),
        160
    );
    assert_eq!(
        bounded["items"][0]["claim"]["base_revision"]["design"]
            .as_str()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(
        bounded["items"][0]["result"]["evidence"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
    assert_eq!(bounded["items"][0]["result"]["evidence_count"], 17);
    assert_eq!(bounded["items"][0]["result"]["evidence_truncated"], true);
}
