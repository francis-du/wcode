use super::*;

#[test]
fn project_observatory_page_is_architecture_first_and_has_no_ball_graph() {
    assert!(INTELLIGENCE_APP_PAGE.contains("Architecture overview"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"architectureMetrics\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"architectureGraph\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"componentInspector\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"overlay\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"design\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"actual\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("Requirements"));
    assert!(INTELLIGENCE_JS.contains("observed_drift_percent"));
    assert!(INTELLIGENCE_JS.contains("evidence_coverage_percent"));
    assert!(INTELLIGENCE_JS.contains("implementation_coverage_percent"));
    assert!(INTELLIGENCE_JS.contains("renderArchitectureGraph"));
    assert!(INTELLIGENCE_JS.contains("renderComponentInspector"));
    assert!(INTELLIGENCE_JS.contains("architectureEdgeTone"));
    assert!(INTELLIGENCE_JS.contains("Strong observed drift"));
    assert!(INTELLIGENCE_JS.contains("整体架构"));
    assert!(INTELLIGENCE_JS.contains("强证据架构偏离"));
    assert!(INTELLIGENCE_JS.contains("savedLanguage === \"zh-CN\" ? \"zh-CN\" : \"en\""));
    assert!(INTELLIGENCE_JS.contains("fragment.get(\"workspace\") || \"\""));
    assert!(!INTELLIGENCE_JS.contains("navigator.language"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("graphCanvas"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("Interactive Software Graph"));
}

#[test]
fn observatory_assets_support_incremental_refresh_precision_and_light_mode() {
    assert!(INTELLIGENCE_APP_PAGE.contains("/intelligence/app.css"));
    assert!(INTELLIGENCE_APP_PAGE.contains("/intelligence/app.js"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"theme\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"refreshSemantic\""));
    assert!(INTELLIGENCE_CSS.contains("prefers-color-scheme:light"));
    assert!(INTELLIGENCE_CSS.contains("html[data-theme=\"light\"]"));
    assert!(INTELLIGENCE_JS.contains("/intelligence/semantic-refresh"));
    assert!(INTELLIGENCE_JS.contains("/intelligence/revision"));
    assert!(INTELLIGENCE_JS.contains("graph_precision"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("style=\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("<script>"));
    assert!(!INTELLIGENCE_JS.contains("style=\""));
    assert!(INTELLIGENCE_JS.contains("setHtml(\"detail\""));
    assert!(INTELLIGENCE_JS.contains("setTimeout(refreshTick, 8000)"));
    assert!(INTELLIGENCE_JS.contains("document.hidden"));
    assert!(!INTELLIGENCE_JS.contains("setInterval("));
    assert!(!INTELLIGENCE_JS.contains("location.reload"));
}

#[test]
fn observatory_exposes_file_structure_and_largest_files() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"fileTree\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"largeFiles\""));
    assert!(INTELLIGENCE_JS.contains("renderProjectStructure"));
    assert!(INTELLIGENCE_JS.contains("structure.entries"));
    assert!(INTELLIGENCE_JS.contains("line_limit"));
    assert!(INTELLIGENCE_CSS.contains(".file-tree"));
}

#[test]
fn observatory_javascript_bundle_parses_without_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(INTELLIGENCE_JS, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "observatory JavaScript bundle contains a syntax error"
    );
}

#[test]
fn observatory_assets_have_a_mobile_safe_touch_layout() {
    assert!(INTELLIGENCE_APP_PAGE.contains("viewport-fit=cover"));
    assert!(INTELLIGENCE_APP_PAGE.contains("aria-controls=\"accessPanel\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("role=\"dialog\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("enterkeyhint=\"search\""));

    for contract in [
        "min-height: 100dvh",
        "safe-area-inset-bottom",
        "@media (max-width:600px)",
        "@media (max-width:900px) and (pointer:coarse)",
        "scroll-snap-type: x mandatory",
        ".change-table td::before",
        "font-size: 16px",
        ".access-panel:not(.hidden)",
    ] {
        assert!(INTELLIGENCE_CSS.contains(contract), "missing {contract}");
    }
    assert!(
        INTELLIGENCE_CSS.rfind("@media (max-width:600px)")
            > INTELLIGENCE_CSS.rfind(".structure-panel {")
    );

    for contract in [
        "function setAccessPanel",
        "function accessPanelOpen",
        "width=\"${layout.width}\"",
        "systemThemeQuery",
        "scrollIntoView({ behavior: \"smooth\"",
    ] {
        assert!(INTELLIGENCE_JS.contains(contract), "missing {contract}");
    }
}
