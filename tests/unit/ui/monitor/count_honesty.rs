use super::*;

#[test]
fn drift_and_evidence_counts_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence::drift_findings_label(Some(2), UiLanguage::En),
        "2 drift finding(s)"
    );
    let unknown = super::monitor_intelligence::drift_findings_label(None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let zh = super::monitor_intelligence::drift_findings_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence::evidence_counts_label(
            Some(7),
            Some(0),
            Some(1),
            UiLanguage::En
        ),
        "7 total · 0 failed · 1 disagreed"
    );
    let absent =
        super::monitor_intelligence::evidence_counts_label(None, Some(0), Some(0), UiLanguage::En);
    assert!(absent.contains("unknown"), "{absent}");
    assert!(!absent.contains('7'), "{absent}");
}

#[test]
fn reconcile_line_keeps_an_unobserved_pending_count_unknown() {
    let label = super::monitor_intelligence::pending_tasks_label(None, UiLanguage::En);
    assert!(label.contains("unknown"), "{label}");
    assert!(!label.contains('0'), "{label}");
    let zh = super::monitor_intelligence::pending_tasks_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence::pending_tasks_label(Some(0), UiLanguage::En),
        "0 pending task(s)"
    );
    assert_eq!(
        super::monitor_intelligence::pending_tasks_label(Some(2), UiLanguage::En),
        "2 pending task(s)"
    );
}

#[test]
fn verification_blockers_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence::blockers_label(Some(2), UiLanguage::En),
        "2 blocker(s)"
    );
    assert_eq!(
        super::monitor_intelligence::blockers_label(Some(0), UiLanguage::En),
        "0 blocker(s)"
    );
    let unknown = super::monitor_intelligence::blockers_label(None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let zh = super::monitor_intelligence::blockers_label(None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
}

#[test]
fn graph_counts_stay_unknown_until_observed() {
    assert_eq!(
        super::monitor_intelligence::graph_label(Some(12), Some(30), UiLanguage::En),
        "12 node(s) · 30 edge(s)"
    );
    let unknown = super::monitor_intelligence::graph_label(None, None, UiLanguage::En);
    assert!(unknown.contains("unknown"), "{unknown}");
    assert!(!unknown.contains('0'), "{unknown}");
    let half = super::monitor_intelligence::graph_label(Some(12), None, UiLanguage::En);
    assert!(half.contains("unknown"), "{half}");
    let zh = super::monitor_intelligence::graph_label(None, None, UiLanguage::ZhCn);
    assert!(zh.contains("未知"), "{zh}");
    assert_eq!(
        super::monitor_intelligence::graph_diff_label(
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
    let diff_unknown = super::monitor_intelligence::graph_diff_label(
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
        super::monitor_intelligence::observed_count(7, true, language),
        "7"
    );
    let unobserved = super::monitor_intelligence::observed_count(0, false, language);
    assert_eq!(unobserved, "unknown");
    let zh = super::monitor_intelligence::observed_count(0, false, UiLanguage::ZhCn);
    assert_eq!(zh, "未知");
}
