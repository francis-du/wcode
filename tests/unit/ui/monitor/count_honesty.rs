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
