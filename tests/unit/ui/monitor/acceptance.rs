use super::*;
use ratatui::backend::TestBackend;

fn record(state: &str) -> Value {
    let hash = format!("sha256:{}", "a".repeat(64));
    serde_json::json!({
        "schema_version": 1, "producer": "wcode/native-acceptance/v1", "captured_at_ms": 10,
        "id": "CAR-fixture", "record_digest": hash, "workspace": "backend",
        "revision": {"code": hash, "design": null},
        "state": state, "risk_level": "high", "partial": false,
        "git": {"binding": {"head_sha": "actual-head"}, "authority": "metadata_only"},
        "policy": null, "plan": {"id": "VP-bound"},
        "summary": {"required": 1, "discovered": 1, "mapped": 1, "executed": 1,
            "passed": 0, "failed": 1, "skipped": 0, "unavailable": 0, "stale": 0, "unknown": 0},
        "reasons": [{"code": "required_check_failed", "subject": "cargo-test", "action": "inspect_failure"}],
        "actions": ["inspect_failure"],
        "checks": [{"id": "cargo-test", "signature": hash, "required": true,
            "discovered": true, "mapped": true, "execution": "executed", "outcome": "fail",
            "freshness": "current", "required_level": "full", "level": "quick", "evidence_ids": ["E-exact"]}],
        "evidence_ids": ["E-exact"],
        "evidence": [{"id": "E-exact", "producer": "native-cargo", "kind": "deterministic",
            "authority": "native_verification", "freshness": "current", "revision": {"code": hash},
            "verification_relation": "required_check", "targets": ["src/lib.rs::answer"]}],
        "verification": null
    })
}

fn observe(monitor: &TaskMonitor, record: Value) {
    monitor.record_intelligence_result(
        "backend",
        "project_observatory",
        &serde_json::json!({
            "repository_revision": record["revision"].clone(), "acceptance": record
        }),
    );
}

fn stats(record: Value) -> IntelligenceStats {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    observe(&monitor, record);
    monitor.snapshot().intelligence["backend"].clone()
}

fn render(stats: &IntelligenceStats, ui: &DashboardState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render_acceptance_console(frame, frame.area(), stats, ui, "backend"))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

use crate::native_acceptance_fixture as native_fixture;

#[tokio::test]
async fn acceptance_real_native_serializer_projects_incomplete_then_ready_without_synthetic_fields()
{
    let fixture = native_fixture::NativeFixture::new();
    assert_eq!(
        fixture.root.path().canonicalize().unwrap(),
        fixture.workspace.root()
    );
    let monitor = TaskMonitor::new([native_fixture::ID.to_owned()]);
    let incomplete = fixture.capture().await;
    assert_eq!(
        incomplete.record().state,
        crate::verification::acceptance::AcceptanceState::Incomplete
    );
    for native in [incomplete, {
        fixture.activate();
        fixture.verify_and_review().await
    }] {
        let record = native.record();
        let serialized = serde_json::to_value(record).unwrap();
        assert!(serialized["revision"].get("complete").is_none());
        monitor.record_intelligence_result(
            native_fixture::ID,
            "project_observatory",
            &serde_json::json!({
                "repository_revision": record.revision, "acceptance": serialized,
            }),
        );
        let snapshot = monitor.snapshot();
        let stats = &snapshot.intelligence[native_fixture::ID];
        let expected = if record.state == crate::verification::acceptance::AcceptanceState::Ready {
            "ready"
        } else {
            "incomplete"
        };
        assert_eq!(acceptance_view(stats, native_fixture::ID).state, expected);
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|frame| {
                render_acceptance_console(
                    frame,
                    frame.area(),
                    stats,
                    &DashboardState::default(),
                    native_fixture::ID,
                )
            })
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains(state_label(expected, UiLanguage::En)));
        assert!(!rendered.contains("Unsupported or incomplete Acceptance Record"));
        if expected == "ready" {
            assert!(rendered.contains("node-lint"));
            assert!(rendered.contains("node-test"));
        }
    }
}

// These are public projection/rendering contracts; native Acceptance evaluation
// is verified independently in tests/unit/verification/acceptance.rs.
#[test]
fn acceptance_default_is_unknown_despite_favorable_monitor_counts() {
    assert_eq!(
        DashboardState::default().console_tab,
        ConsoleTab::Acceptance
    );
    let stats = IntelligenceStats {
        evidence_total: 100,
        verification_ready: Some(true),
        ..Default::default()
    };
    let text = render(&stats, &DashboardState::default(), 100, 16);
    assert!(text.contains("Unknown"));
    assert!(text.contains("Evidence counts do not determine Acceptance"));
    assert!(!text.contains("Ready"));
}

#[test]
fn acceptance_canonical_state_is_not_recomputed_from_check_counts() {
    for (state, label) in [
        ("ready", "Ready"),
        ("blocked", "Blocked"),
        ("needs_review", "Needs review"),
        ("incomplete", "Incomplete"),
        ("stale", "Stale"),
    ] {
        let stats = stats(record(state));
        assert_eq!(acceptance_view(&stats, "backend").state, state);
        assert!(render(&stats, &DashboardState::default(), 100, 16).contains(label));
    }
}

#[test]
fn acceptance_malformed_or_unsupported_projection_never_shows_ready() {
    for mutation in 0..9 {
        let mut record = record("ready");
        match mutation {
            0 => record["schema_version"] = 2.into(),
            1 => record["producer"] = "model/native-spoof".into(),
            2 => record["record_digest"] = "sha256:short".into(),
            3 => record["checks"][0]["outcome"] = "approved".into(),
            4 => record["checks"][0]["execution"] = "running".into(),
            5 => record["summary"]["required"] = (-1).into(),
            6 => record["actions"][0] = "https://untrusted.test".into(),
            7 => record["captured_at_ms"] = Value::Null,
            _ => {
                record["verification"] =
                    serde_json::json!({"stage_results": {"property": "approved"}})
            }
        }
        assert_eq!(acceptance_view(&stats(record), "backend").state, "unknown");
    }
}

#[test]
fn acceptance_foreign_workspace_is_not_displayed_or_inspected() {
    let mut record = record("ready");
    record["workspace"] = "foreign".into();
    record["checks"][0]["id"] = "foreign-private-target".into();
    let stats = stats(record);
    let ui = DashboardState {
        acceptance_detail_open: true,
        ..Default::default()
    };
    let text = render(&stats, &ui, 100, 16);
    assert!(text.contains("another workspace"));
    assert!(!text.contains("foreign-private-target"));
    assert_eq!(acceptance_entry_count(&stats, "backend"), 0);
}

#[test]
fn acceptance_refresh_error_or_revision_drift_keeps_history_stale() {
    for mutation in 0..4 {
        let mut stats = stats(record("ready"));
        match mutation {
            0 => stats.refreshing = true,
            1 => stats.refresh_error = Some("native capture failed".into()),
            2 => stats.project_revision = None,
            _ => {
                stats.project_revision.as_mut().unwrap()["code"] =
                    format!("sha256:{}", "b".repeat(64)).into()
            }
        }
        let view = acceptance_view(&stats, "backend");
        assert_eq!(view.state, "stale");
        assert!(view.record.is_some());
        assert!(
            render(&stats, &DashboardState::default(), 100, 16).contains("Historical observation")
        );
    }
}

#[test]
fn acceptance_partial_ready_is_incomplete_and_nullable_subject_remains_valid() {
    let mut record = record("ready");
    record["partial"] = true.into();
    record["reasons"][0]["subject"] = Value::Null;
    assert_eq!(
        acceptance_view(&stats(record), "backend").state,
        "incomplete"
    );
}

#[test]
fn acceptance_check_detail_separates_execution_outcome_freshness_and_level() {
    let stats = stats(record("blocked"));
    let ui = DashboardState {
        console_focus: 1,
        acceptance_detail_open: true,
        ..Default::default()
    };
    let text = render(&stats, &ui, 120, 24);
    for expected in [
        "execution executed",
        "outcome fail",
        "freshness current",
        "required full / reported quick",
        "signature sha256:",
        "Evidence E-exact",
        "native-cargo",
        "authority native_verification",
        "src/lib.rs::answer",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert!(text.contains("W Web: Acceptance"));
}

#[test]
fn acceptance_missing_evidence_reference_never_substitutes_another_record() {
    let mut record = record("blocked");
    record["checks"][0]["evidence_ids"] = serde_json::json!(["E-missing"]);
    let stats = stats(record);
    let ui = DashboardState {
        console_focus: 1,
        acceptance_detail_open: true,
        ..Default::default()
    };
    let text = render(&stats, &ui, 120, 24);
    assert!(text.contains("Evidence E-missing"));
    assert!(text.contains("no substitute selected"));
    assert!(!text.contains("native-cargo"));
}

#[test]
fn acceptance_detail_redacts_secrets_and_terminal_controls() {
    let mut record = record("blocked");
    record["evidence"][0]["targets"] = serde_json::json!([
        "Authorization: Bearer fixture_acceptance_secret_12345",
        "\u{001b}[31msrc/lib.rs"
    ]);
    let stats = stats(record);
    let ui = DashboardState {
        console_focus: 1,
        acceptance_detail_open: true,
        ..Default::default()
    };
    let text = render(&stats, &ui, 120, 24);
    assert!(!text.contains("fixture_acceptance_secret_12345"));
    assert!(!text.contains('\u{001b}'));
}

#[test]
fn acceptance_bounded_reasons_explicitly_show_omission() {
    let mut record = record("blocked");
    record["reasons"] = Value::Array(
        (0..40)
            .map(|id| {
                serde_json::json!({
                    "code": format!("blocking-{id}"), "subject": null, "action": "inspect_failure"
                })
            })
            .collect(),
    );
    let stats = stats(record);
    assert_eq!(acceptance_entry_count(&stats, "backend"), 33);
    let text = render(&stats, &DashboardState::default(), 120, 16);
    assert!(text.contains("32 / 40 reasons"));
}

#[test]
fn acceptance_inspection_keys_do_not_run_checks_or_approve_requests() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("backend");
    std::fs::create_dir(&path).unwrap();
    let workspaces = Workspaces::new(&[path], true, true).unwrap();
    let config = MonitorConfig {
        version: "test".into(),
        instance_id: "test".into(),
        local_health_url: "http://127.0.0.1:8765/healthz".into(),
        public_url: Arc::new(std::sync::RwLock::new("https://example.test".into())),
        intelligence_url: "http://127.0.0.1:8765/intelligence".into(),
        project_url: "https://example.test".into(),
        author_url: "https://example.test".into(),
        author_handle: "operator".into(),
        pairing_code: "123456".into(),
        max_parallel: 4,
        input_token_price_per_million_usd: 0.0,
        semantic_auto: false,
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
    };
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    observe(&monitor, record("blocked"));
    let snapshot = monitor.snapshot();
    let mut ui = DashboardState {
        intelligence_open: true,
        workspace_focus_id: Some("backend".into()),
        ..Default::default()
    };
    let area = Rect::new(0, 0, 120, 30);
    let key = |code| event::KeyEvent::new(code, KeyModifiers::NONE);
    assert!(handle_console_key(
        key(KeyCode::Down),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_focus, 1);
    assert!(handle_console_key(
        key(KeyCode::Enter),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(ui.acceptance_detail_open);
    assert!(!handle_console_key(
        key(KeyCode::Char('y')),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(!handle_console_key(
        event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(handle_console_key(
        key(KeyCode::Char('6')),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Summary);
    assert!(!ui.acceptance_detail_open);
    assert!(monitor.snapshot().tasks.is_empty());
}
