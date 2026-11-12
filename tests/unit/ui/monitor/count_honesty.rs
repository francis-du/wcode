use super::*;

#[test]
fn drift_and_evidence_counts_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence_labels::drift_findings_label(Some(2), UiLanguage::En),
        "2 drift finding(s)"
    );
    let unknown = super::monitor_intelligence_labels::drift_findings_label(None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let zh = super::monitor_intelligence_labels::drift_findings_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence_labels::evidence_counts_label(
            Some(7),
            Some(0),
            Some(1),
            UiLanguage::En
        ),
        "7 total · 0 failed · 1 disagreed"
    );
    let absent =
        super::monitor_intelligence_labels::evidence_counts_label(None, Some(0), Some(0), UiLanguage::En);
    assert!(absent.contains("unknown"), "{absent}");
    assert!(!absent.contains('7'), "{absent}");
}

#[test]
fn convergence_counts_stay_unknown_until_observed() {
    let unknown = super::monitor_intelligence_labels::convergence_label(0, 0, 0, 0, false, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let zh = super::monitor_intelligence_labels::convergence_label(0, 0, 0, 0, false, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence_labels::convergence_label(3, 1, 2, 1, true, UiLanguage::En),
        "3 stable · 1 changing · 3 need work"
    );
    assert_eq!(
        super::monitor_intelligence_labels::convergence_label(3, 1, 2, 1, true, UiLanguage::ZhCn),
        "3 稳定 · 1 变更中 · 3 需处理"
    );
}

#[test]
fn reconcile_line_keeps_an_unobserved_pending_count_unknown() {
    let label = super::monitor_intelligence_labels::pending_tasks_label(None, UiLanguage::En);
    assert!(label.contains("unknown"), "{label}");
    assert!(!label.contains('0'), "{label}");
    let zh = super::monitor_intelligence_labels::pending_tasks_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence_labels::pending_tasks_label(Some(0), UiLanguage::En),
        "0 pending task(s)"
    );
    assert_eq!(
        super::monitor_intelligence_labels::pending_tasks_label(Some(2), UiLanguage::En),
        "2 pending task(s)"
    );
}

#[test]
fn verification_blockers_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence_labels::blockers_label(Some(2), UiLanguage::En),
        "2 blocker(s)"
    );
    assert_eq!(
        super::monitor_intelligence_labels::blockers_label(Some(0), UiLanguage::En),
        "0 blocker(s)"
    );
    let unknown = super::monitor_intelligence_labels::blockers_label(None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let zh = super::monitor_intelligence_labels::blockers_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
}

#[test]
fn graph_counts_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence_labels::graph_label(Some(12), Some(30), UiLanguage::En),
        "12 node(s) · 30 edge(s)"
    );
    let unknown = super::monitor_intelligence_labels::graph_label(None, None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let half = super::monitor_intelligence_labels::graph_label(Some(12), None, UiLanguage::En);
    assert!(half.contains("unknown"), "{half}");
    let zh = super::monitor_intelligence_labels::graph_label(None, None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence_labels::graph_diff_label(
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            UiLanguage::En
        ),
        "node(s) +1/-2/~3 · edge(s) +4/-5/~6"
    );
    let diff_unknown = super::monitor_intelligence_labels::graph_diff_label(
        Some(1),
        None,
        Some(3),
        Some(4),
        Some(5),
        Some(6),
        UiLanguage::En,
    );
    assert!(diff_unknown.contains("unknown"), "{diff_unknown}");
}

#[test]
fn semantic_and_provider_groups_stay_unknown_until_observed() {
    let language = UiLanguage::En;
    assert_eq!(
        super::monitor_intelligence_labels::observed_count(7, true, language),
        "7"
    );
    let unobserved = super::monitor_intelligence_labels::observed_count(0, false, language);
    assert_eq!(unobserved, "unknown");
    let zh = super::monitor_intelligence_labels::observed_count(0, false, UiLanguage::ZhCn);
    assert_eq!(zh, "未知");
}

#[test]
fn observation_groups_record_whether_their_event_arrived() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["api"];
    assert!(!stats.design_observed);
    assert!(!stats.convention_observed);
    assert!(!stats.scope_observed);
    assert!(!stats.semantic_observed);
    assert!(!stats.provider_observed);
    assert!(!stats.session_observed);
    assert!(!stats.graph_provider_observed);

    monitor.record_intelligence_result(
        "api",
        "design_status",
        &serde_json::json!({"valid": false, "initialized": false, "requirements": 3, "components": 2}),
    );
    monitor.record_intelligence_result(
        "api",
        "convention_status",
        &serde_json::json!({"errors": 1, "warnings": 2}),
    );
    monitor.record_intelligence_result(
        "api",
        "scope_status",
        &serde_json::json!({"source_files": 9, "mapped_files": 7}),
    );
    monitor.record_intelligence_result(
        "api",
        "semantic_status",
        &serde_json::json!({"confirmed": 4, "candidates": 5}),
    );
    monitor.record_intelligence_result("api", "semantic_provider_status", &serde_json::json!([]));
    monitor.record_intelligence_result(
        "api",
        "semantic_session_status",
        &serde_json::json!({"sessions": 1, "documents": 1, "starts": 1, "requests": 1}),
    );
    monitor.record_intelligence_result("api", "graph_provider_status", &serde_json::json!([]));

    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["api"];
    assert!(stats.design_observed && stats.convention_observed && stats.scope_observed);
    assert!(
        stats.semantic_observed
            && stats.provider_observed
            && stats.session_observed
            && stats.graph_provider_observed
    );
    assert_eq!(stats.requirements, 3);
    assert_eq!(stats.policy_errors, 1);
    assert_eq!(stats.scope_source_files, 9);
    assert_eq!(stats.semantic_confirmed, 4);
}
