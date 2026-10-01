use super::*;
use ratatui::backend::TestBackend;

fn fixture() -> (tempfile::TempDir, MonitorConfig, TaskMonitor) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("backend");
    std::fs::create_dir(&path).unwrap();
    let config = MonitorConfig {
        version: "test".into(),
        instance_id: "inspect-fixture".into(),
        local_health_url: "http://127.0.0.1:8765/healthz".into(),
        public_url: Arc::new(std::sync::RwLock::new("https://example.test".into())),
        intelligence_url: "http://127.0.0.1:8765/intelligence".into(),
        project_url: "https://example.test/project".into(),
        author_url: "https://example.test/author".into(),
        author_handle: "fixture".into(),
        pairing_code: "123456".into(),
        max_parallel: 2,
        input_token_price_per_million_usd: 0.0,
        semantic_auto: false,
        workspaces: Workspaces::new(&[path], true, false).unwrap(),
        harness: ToolHarness::new(2).unwrap(),
    };
    (root, config, TaskMonitor::new(["backend".into()]))
}

// A serializer/navigation fixture, never a native verification or Ready receipt.
fn record() -> Value {
    let sha = format!("sha256:{}", "a".repeat(64));
    serde_json::json!({
        "schema_version": 1, "producer": "wcode/native-acceptance/v1",
        "captured_at_ms": 10, "id": "CAR-inspect-fixture", "record_digest": sha,
        "workspace": "backend", "revision": {"code": sha, "design": null},
        "state": "blocked", "risk_level": "high", "partial": false,
        "summary": {"required": 1, "discovered": 1, "mapped": 1, "executed": 0,
            "passed": 0, "failed": 0, "skipped": 0, "unavailable": 0, "stale": 0, "unknown": 1},
        "reasons": [{"code": "required_check_failed", "subject": "rust-test", "action": "inspect_failure"}],
        "actions": ["inspect_failure"], "verification": null,
        "checks": [{"id": "rust-test", "signature": sha, "required": true,
            "discovered": true, "mapped": true, "execution": "unknown", "outcome": "unknown",
            "freshness": "missing", "required_level": "full", "level": null,
            "evidence_ids": ["E-exact"]}],
        "evidence_ids": ["E-exact"],
        "evidence": [{"id": "E-exact", "targets": ["src/lib.rs::answer"]}],
        "git": {"changes": []}
    })
}

fn publish(monitor: &TaskMonitor, value: Value) {
    monitor.record_intelligence_result(
        "backend",
        "project_observatory",
        &serde_json::json!({"repository_revision": value["revision"], "acceptance": value}),
    );
}

fn request(path: &str, start: usize, expected_sha256: Option<String>) -> InspectRequest {
    InspectRequest {
        workspace: "backend".into(),
        target: inspect_target(path).unwrap(),
        start,
        expected_sha256,
    }
}

fn source(config: &MonitorConfig, content: &str) {
    let (_, workspace) = config.workspaces.select(Some("backend")).unwrap();
    std::fs::create_dir_all(workspace.root().join("src")).unwrap();
    std::fs::write(workspace.root().join("src/lib.rs"), content).unwrap();
}

fn page(config: &MonitorConfig, request: &InspectRequest) -> anyhow::Result<InspectPage> {
    let (_, workspace) = config.workspaces.select(Some("backend"))?;
    read_inspect_page(&config.harness, &workspace, request)
}

fn buffer(inspection: &SourceInspection) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal
        .draw(|frame| render_source_inspection(frame, frame.area(), inspection))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn ui(monitor: &TaskMonitor) -> DashboardState {
    publish(monitor, record());
    DashboardState {
        intelligence_open: true,
        workspace_focus_id: Some("backend".into()),
        acceptance_detail_open: true,
        ..Default::default()
    }
}

#[test]
fn source_inspection_reads_real_file_and_exact_symbol_without_execution() {
    let (_root, config, _) = fixture();
    let content = format!(
        "{}pub fn answer() -> u32 {{ 42 }}\n",
        "// unchanged\n".repeat(260)
    );
    source(&config, &content);
    let read = page(&config, &request("src/lib.rs::answer", 0, None)).unwrap();
    assert_eq!(read.path, "src/lib.rs");
    assert_eq!(read.start, 261);
    assert!(read.content.contains("pub fn answer"));
    assert!(read.outline_notice.contains("syntax"));
    assert_eq!(read.symbols[read.selected_symbol].name, "answer");
    let (_, workspace) = config.workspaces.select(Some("backend")).unwrap();
    assert!(!workspace.exec_enabled());
    assert_eq!(
        std::fs::read_to_string(workspace.root().join("src/lib.rs")).unwrap(),
        content
    );
}

#[test]
fn source_inspection_pages_are_bounded_and_changed_sha_is_rejected() {
    let (_root, config, _) = fixture();
    source(
        &config,
        &format!("pub fn answer() {{}}\n{}", "// source\n".repeat(600)),
    );
    let first = page(&config, &request("src/lib.rs", 1, None)).unwrap();
    assert_eq!((first.start, first.end, first.total), (1, 240, 601));
    let second = page(
        &config,
        &request("src/lib.rs", 241, Some(first.sha256.clone())),
    )
    .unwrap();
    assert_eq!((second.start, second.end, second.total), (241, 480, 601));
    assert_eq!(first.sha256, second.sha256);
    source(&config, "pub fn changed() {}\n");
    let error = page(&config, &request("src/lib.rs", 1, Some(first.sha256))).unwrap_err();
    assert!(error.to_string().contains("Source changed"));
}

#[test]
fn source_inspection_rejects_unsafe_targets_and_never_guesses_symbol_ids() {
    for value in [
        "",
        "../secret",
        "/tmp/file.rs",
        ".env::",
        "language:rust",
        "symbol:ts:opaque",
        "C:/secret",
        "src\\secret.rs",
        "src/a.rs\n",
        "./src/a.rs",
    ] {
        assert!(inspect_target(value).is_none(), "{value:?}");
    }
    let target = inspect_target("file:src/lib.rs::module::answer").unwrap();
    assert_eq!(target.path, "src/lib.rs");
    assert_eq!(target.symbol.as_deref(), Some("module::answer"));
}

#[test]
fn source_inspection_protected_missing_and_non_utf8_files_fail_visibly() {
    let (_root, config, _) = fixture();
    let (_, workspace) = config.workspaces.select(Some("backend")).unwrap();
    std::fs::write(workspace.root().join(".env"), "fixture=private").unwrap();
    std::fs::write(workspace.root().join("binary.rs"), [0xff, 0xfe]).unwrap();
    for path in [".env", "missing.rs", "binary.rs"] {
        assert!(page(&config, &request(path, 1, None)).is_err(), "{path}");
    }
    std::fs::write(
        workspace.root().join("oversized.rs"),
        vec![b'a'; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(page(&config, &request("oversized.rs", 1, None)).is_err());
}

#[cfg(unix)]
#[test]
fn source_inspection_symlink_escape_remains_denied() {
    let (_root, config, _) = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.rs"), "pub fn outside() {}").unwrap();
    let (_, workspace) = config.workspaces.select(Some("backend")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.rs"),
        workspace.root().join("linked.rs"),
    )
    .unwrap();
    assert!(page(&config, &request("linked.rs", 1, None)).is_err());
}

#[test]
fn source_inspection_exact_evidence_targets_do_not_substitute_missing_records() {
    let (_, _, monitor) = fixture();
    publish(&monitor, record());
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["backend"];
    let targets = acceptance_inspect_targets(stats, "backend", 0);
    assert_eq!(targets.len(), 1);
    assert!(!targets[0].candidate);
    assert_eq!(targets[0].symbol.as_deref(), Some("answer"));
    assert!(acceptance_inspect_targets(stats, "other", 0).is_empty());

    let mut missing = record();
    missing["checks"][0]["evidence_ids"] = serde_json::json!(["E-missing"]);
    missing["evidence"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id": "E-other", "targets": ["private.rs"]}));
    publish(&monitor, missing);
    assert!(
        acceptance_inspect_targets(&monitor.snapshot().intelligence["backend"], "backend", 0)
            .is_empty()
    );
}

#[test]
fn source_inspection_candidate_rename_paths_are_distinct_from_evidence() {
    let (_, _, monitor) = fixture();
    let mut value = record();
    value["evidence"][0]["targets"] = serde_json::json!(["language:rust"]);
    value["git"]["changes"] = serde_json::json!([
        {"old_path":"src/old.rs", "new_path":"src/new.rs"},
        {"old_path":null, "new_path":"../private"},
        {"old_path":null, "new_path":"src/new.rs"}
    ]);
    publish(&monitor, value);
    let targets =
        acceptance_inspect_targets(&monitor.snapshot().intelligence["backend"], "backend", 0);
    assert_eq!(
        targets
            .iter()
            .map(|target| target.path.as_str())
            .collect::<Vec<_>>(),
        ["src/new.rs", "src/old.rs"]
    );
    assert!(targets.iter().all(|target| target.candidate));
}

#[test]
fn source_inspection_redacts_source_and_terminal_controls_without_minting_evidence() {
    let (_root, config, monitor) = fixture();
    let secret = format!("ghp_{}", "A".repeat(36));
    source(
        &config,
        &format!("pub fn answer() {{}}\n// {secret}\n// \u{1b}[31mfixture\n"),
    );
    let mut ui = ui(&monitor);
    open_source_inspection(&mut ui, &monitor.snapshot(), &config);
    let inspection = ui.source_inspection.as_mut().unwrap();
    inspection.request = None;
    inspection.page = Some(page(&config, &request("src/lib.rs", 1, None)).unwrap());
    let rendered = buffer(inspection);
    assert!(!rendered.contains(&secret));
    assert!(!rendered.contains('\u{1b}'));
    assert!(rendered.contains("redacted"));
    assert!(rendered.contains("not historical Evidence"));
    assert_eq!(
        acceptance_view(&monitor.snapshot().intelligence["backend"], "backend")
            .record
            .unwrap()["state"],
        "blocked"
    );
}

#[test]
fn source_inspection_keys_preserve_existing_views_and_require_unmodified_press() {
    let (_root, config, monitor) = fixture();
    let mut ui = ui(&monitor);
    let snapshot = monitor.snapshot();
    let area = Rect::new(0, 0, 100, 30);
    assert!(!handle_console_key(
        event::KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(ui.source_inspection.is_none());
    assert!(handle_console_key(
        event::KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(ui.source_inspection.is_some());
    assert!(handle_console_key(
        event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(ui.source_inspection.is_none());
    assert!(ui.intelligence_open);
    assert!(ui.acceptance_detail_open);
    open_source_inspection(&mut ui, &snapshot, &config);
    assert!(handle_console_key(
        event::KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Summary);
    assert!(ui.source_inspection.is_none());
    assert!(config.workspaces.authorization_requests(16).is_empty());
}

#[test]
fn source_inspection_stale_acceptance_and_foreign_workspace_drop_pending_reply() {
    let (root, mut config, monitor) = fixture();
    let other = root.path().join("other");
    std::fs::create_dir(&other).unwrap();
    config.workspaces = Workspaces::new([root.path().join("backend"), other], true, false).unwrap();
    source(&config, "pub fn answer() {}\n");
    let mut ui = ui(&monitor);
    open_source_inspection(&mut ui, &monitor.snapshot(), &config);
    let completed = page(&config, &request("src/lib.rs", 1, None)).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(Ok(completed)).unwrap();
    let inspection = ui.source_inspection.as_mut().unwrap();
    inspection.request = None;
    inspection.pending = Some(receiver);
    let mut next = record();
    next["record_digest"] = Value::String(format!("sha256:{}", "b".repeat(64)));
    publish(&monitor, next);
    refresh_source_inspection(&mut ui, &monitor.snapshot(), &config);
    let inspection = ui.source_inspection.as_ref().unwrap();
    assert!(inspection.page.is_none());
    assert!(inspection.pending.is_none());
    assert!(inspection
        .error
        .as_deref()
        .unwrap()
        .contains("Acceptance changed"));
    ui.workspace_focus_id = Some("other".into());
    // An actually selected sibling workspace rejects the pending old identity.
    refresh_source_inspection(&mut ui, &monitor.snapshot(), &config);
    assert!(ui.source_inspection.is_none());
}

#[test]
fn source_inspection_symbol_jump_and_page_navigation_keep_exact_sha() {
    let (_root, config, monitor) = fixture();
    source(
        &config,
        &format!("pub fn answer() {{}}\n{}", "// source\n".repeat(500)),
    );
    let mut ui = ui(&monitor);
    open_source_inspection(&mut ui, &monitor.snapshot(), &config);
    let read = page(&config, &request("src/lib.rs", 1, None)).unwrap();
    let expected = read.sha256.clone();
    let inspection = ui.source_inspection.as_mut().unwrap();
    inspection.request = None;
    inspection.page = Some(read);
    let _ = buffer(inspection);
    assert!(handle_source_inspection_key(
        event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut ui
    ));
    let queued = ui
        .source_inspection
        .as_ref()
        .unwrap()
        .request
        .as_ref()
        .unwrap();
    assert_eq!(queued.expected_sha256.as_deref(), Some(expected.as_str()));
    assert_eq!(queued.start, 1);
    let inspection = ui.source_inspection.as_mut().unwrap();
    inspection.request = None;
    assert!(handle_source_inspection_key(
        event::KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
        &mut ui
    ));
    assert!(handle_source_inspection_key(
        event::KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        &mut ui
    ));
    assert_eq!(
        ui.source_inspection
            .as_ref()
            .unwrap()
            .request
            .as_ref()
            .unwrap()
            .start,
        241
    );
}
#[test]
fn source_inspection_missing_symbol_never_substitutes_and_outline_is_bounded() {
    let (_root, config, monitor) = fixture();
    let content = (0..140)
        .map(|i| format!("pub fn answer_{i}() {{}}\n"))
        .collect::<String>();
    source(&config, &content);
    let read = page(&config, &request("src/lib.rs::missing", 0, None)).unwrap();
    assert_eq!(read.start, 1);
    assert_eq!(read.symbols.len(), 128);
    assert_eq!(read.total_symbols, 140);
    assert_eq!(read.selected_symbol, usize::MAX);
    assert!(read.outline_notice.contains("truncated"));
    assert!(read.outline_notice.contains("no substitute"));
    let mut ui = ui(&monitor);
    open_source_inspection(&mut ui, &monitor.snapshot(), &config);
    let inspection = ui.source_inspection.as_mut().unwrap();
    inspection.request = None;
    inspection.selected_symbol = read.selected_symbol;
    inspection.page = Some(read);
    assert!(buffer(inspection).contains("No symbol selected"));
    assert!(handle_source_inspection_key(
        event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut ui
    ));
    assert_eq!(ui.source_inspection.as_ref().unwrap().selected_symbol, 0);
}
