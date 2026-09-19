use super::*;

#[path = "observatory.rs"]
mod observatory;

#[test]
fn project_observatory_page_is_architecture_first_and_has_no_ball_graph() {
    assert!(INTELLIGENCE_APP_PAGE.contains("Engineering Observatory"));
    assert!(INTELLIGENCE_APP_PAGE.contains("/intelligence/logo.svg"));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"brand-logo\""));
    assert!(INTELLIGENCE_LOGO_SVG.contains("#665cff"));
    assert!(INTELLIGENCE_LOGO_SVG.contains("#f43f8f"));
    assert!(INTELLIGENCE_APP_PAGE.contains("Engineering architecture"));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-i18n=\"System map\">System map"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("id=\"architectureMetrics\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"architectureBlueprint\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"architecture-workbench stage-surface\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"projectNavigator\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"navigatorResults\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"engineeringFlow\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"changeStory\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"engineeringTimeline\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"runtimeTopology\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-back=\"blueprint\""));
    assert!(INTELLIGENCE_JS.contains("data-inspector-components"));
    assert!(INTELLIGENCE_JS.contains("data-inspector-dependencies"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"architectureGraph\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"componentInspector\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"overlay\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"design\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-mode=\"actual\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("Requirements"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"allCommandsToggle\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"allCommandsStatus\""));
    assert!(INTELLIGENCE_JS.contains("/intelligence/command-trust"));
    assert!(INTELLIGENCE_JS.contains("toggleAllCommandsFromUi"));
    assert!(INTELLIGENCE_JS.contains("all_commands_authorized"));
    assert!(INTELLIGENCE_JS.contains("本次运行已全部授权"));
    assert!(INTELLIGENCE_CSS.contains(".all-command-trust-row"));
    assert!(INTELLIGENCE_JS.contains("observed_drift_percent"));
    assert!(INTELLIGENCE_JS.contains("evidence_coverage_percent"));
    assert!(INTELLIGENCE_JS.contains("implementation_coverage_percent"));
    assert!(INTELLIGENCE_JS.contains("acceptance.fresh"));
    assert!(INTELLIGENCE_JS.contains("current_verification_plans"));
    assert!(INTELLIGENCE_JS.contains("current_verification_ready"));
    assert!(INTELLIGENCE_JS.contains("selectedEvidenceKey"));
    assert!(INTELLIGENCE_JS.contains("evidence-ledger-row"));
    assert!(INTELLIGENCE_JS.contains("renderArchitectureBlueprint"));
    assert!(INTELLIGENCE_JS.contains("systemMapTiers"));
    assert!(INTELLIGENCE_JS.contains("projectNavigatorItems"));
    assert!(INTELLIGENCE_JS.contains("activateProjectNavigatorItem"));
    assert!(INTELLIGENCE_JS.contains("renderProjectNavigator"));
    assert!(INTELLIGENCE_JS.contains("function systemMapTiers"));
    assert!(INTELLIGENCE_JS.contains("function renderSubsystemInspector"));
    assert!(INTELLIGENCE_JS.contains("function subsystemDependencyIds"));
    assert!(INTELLIGENCE_JS.contains("function setSystemMapFull"));
    assert!(INTELLIGENCE_JS.contains("renderEngineeringFlow"));
    assert!(INTELLIGENCE_JS.contains("renderChangeStory"));
    assert!(INTELLIGENCE_JS.contains("renderEngineeringTimeline"));
    assert!(INTELLIGENCE_JS.contains("renderRuntimeTopology"));
    assert!(INTELLIGENCE_JS.contains("state.tunnelSnapshot"));
    assert!(INTELLIGENCE_JS.contains("void refreshTunnels()"));
    assert!(INTELLIGENCE_JS.contains("finished_ago_ms"));
    assert!(INTELLIGENCE_JS.contains("engineering_journal"));
    assert!(INTELLIGENCE_JS.contains("Persistent milestone"));
    assert!(INTELLIGENCE_JS.contains("milestone.checks_run"));
    assert!(INTELLIGENCE_APP_PAGE.contains("Files → components → requirements"));
    assert!(INTELLIGENCE_JS.contains("renderArchitectureGraph"));
    assert!(INTELLIGENCE_JS.contains("renderComponentInspector"));
    assert!(INTELLIGENCE_JS.contains("Verification mapping, not execution proof"));
    assert!(INTELLIGENCE_JS.contains("Design vs actual"));
    assert!(INTELLIGENCE_JS.contains("stable requirements"));
    assert!(INTELLIGENCE_JS.contains("data-inspector-proof"));
    assert!(INTELLIGENCE_JS.contains("architectureEdgeTone"));
    assert!(INTELLIGENCE_JS.contains("Strong observed drift"));
    assert!(INTELLIGENCE_JS.contains("工程观测台"));
    assert!(INTELLIGENCE_JS.contains("工程架构"));
    assert!(INTELLIGENCE_JS.contains("架构蓝图"));
    assert!(INTELLIGENCE_JS.contains("实时工程流"));
    assert!(INTELLIGENCE_JS.contains("Vibe Coding 变更链"));
    assert!(INTELLIGENCE_JS.contains("强证据架构偏离"));
    assert!(INTELLIGENCE_JS.contains("language: initialLanguage(savedLanguage)"));
    assert!(INTELLIGENCE_JS.contains("globalThis.navigator?.languages"));
    assert!(INTELLIGENCE_JS.contains("fragment.get(\"workspace\") || \"\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("graphCanvas"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("Interactive Software Graph"));
    assert!(INTELLIGENCE_CSS.contains(".architecture-workbench"));
    assert!(INTELLIGENCE_CSS.contains(".project-navigator"));
    assert!(INTELLIGENCE_CSS.contains(".navigator-results"));
    assert!(INTELLIGENCE_CSS.contains(".system-map"));
    assert!(INTELLIGENCE_CSS.contains(".system-map-root"));
    assert!(INTELLIGENCE_CSS.contains(".system-tier"));
    assert!(INTELLIGENCE_CSS.contains(".subsystem-card"));
    assert!(INTELLIGENCE_CSS.contains(".dependency-ledger"));
    assert!(INTELLIGENCE_CSS.contains(".dependency-row"));
    assert!(INTELLIGENCE_CSS.contains(".engineering-cycle-main"));
    assert!(INTELLIGENCE_CSS.contains(".cycle-support-grid"));
    assert!(INTELLIGENCE_CSS.contains(".trace-flow"));
    assert!(INTELLIGENCE_CSS.contains(".trace-convergence"));
    assert!(INTELLIGENCE_CSS.contains(".change-convergence-flow"));
    assert!(INTELLIGENCE_CSS.contains(".impact-signal-strip"));
    assert!(INTELLIGENCE_CSS.contains(".root-branch"));
    assert!(INTELLIGENCE_CSS.contains(".tier-flow-bridge"));
    assert!(INTELLIGENCE_CSS.contains(".change-impact-grid"));
    assert!(INTELLIGENCE_CSS.contains(".engineering-timeline"));
    assert!(INTELLIGENCE_CSS.contains(".engineering-event.blocked"));
    assert!(INTELLIGENCE_CSS.contains(".runtime-status-grid"));
    assert!(!INTELLIGENCE_CSS.contains(".blueprint-edge"));
    assert!(!INTELLIGENCE_CSS.contains(".arch-edge"));
    assert!(!INTELLIGENCE_CSS.contains(".flow-arrow"));
    assert!(!INTELLIGENCE_CSS.contains(".story-arrow"));
    assert!(!INTELLIGENCE_CSS.contains(".runtime-edge"));
    assert!(INTELLIGENCE_CSS.contains(".inspector-health"));
    assert!(INTELLIGENCE_CSS.contains(".inspector-compare"));
    assert!(INTELLIGENCE_CSS.contains(".inspector-proof-note"));
    assert!(INTELLIGENCE_CSS.contains(
        ".agent-context-efficiency .proof-counts{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));}"
    ));
    assert!(INTELLIGENCE_CSS
        .contains(".agent-context-efficiency .proof-counts>*:last-child{grid-column:1/-1;}"));
    assert!(INTELLIGENCE_CSS.contains("@media (max-width:520px){"));
    assert!(INTELLIGENCE_CSS
        .contains(".runtime-status-grid,.change-impact-grid{grid-template-columns:1fr;}"));
    assert!(INTELLIGENCE_CSS
        .contains(".agent-context-efficiency .proof-counts{grid-template-columns:1fr;}"));
}

#[test]
fn observatory_visual_contract_stays_compact_brand_aligned_and_blueprint_first() {
    for forbidden in ["#b69761", "#9e7b49", "#8c8374", "#151512"] {
        assert!(
            !INTELLIGENCE_CSS.contains(forbidden),
            "high-saturation legacy primary token leaked into Observatory CSS: {forbidden}"
        );
    }
    assert!(INTELLIGENCE_CSS.contains("--bg:#0b0812"));
    assert!(INTELLIGENCE_CSS.contains("--accent:#8b7cff"));
    assert!(INTELLIGENCE_CSS.contains("--accent-strong:#665cff"));
    assert!(INTELLIGENCE_CSS.contains("--accent-secondary:#f05aa6"));
    assert!(INTELLIGENCE_CSS.contains("--radius-xl:34px"));
    assert!(INTELLIGENCE_CSS.contains("--glass:"));
    assert!(INTELLIGENCE_CSS
        .contains("--font-sans:\"Inter\",\"Noto Sans SC\",\"Noto Sans\",sans-serif"));
    assert!(
        INTELLIGENCE_CSS.contains("--font-mono:\"JetBrains Mono\",\"Noto Sans Mono\",monospace")
    );
    for proprietary in [
        "Segoe UI",
        "SFMono-Regular",
        "Menlo",
        "Consolas",
        "-apple-system",
        "system-ui",
    ] {
        assert!(
            !INTELLIGENCE_CSS.contains(proprietary),
            "proprietary platform font leaked into Observatory CSS: {proprietary}"
        );
    }
    assert!(INTELLIGENCE_CSS.contains("backdrop-filter:blur(24px)"));
    assert!(INTELLIGENCE_CSS.contains(".component-inspector.open"));
    assert!(INTELLIGENCE_CSS.contains(".component-inspector{position:sticky"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("observatory-rail"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("utility-deck"));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"global-bar\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"workspace-hero\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"workspace-switchboard\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"workspace-tabs\""));
    assert_eq!(
        INTELLIGENCE_APP_PAGE.matches("data-workspace-tab=").count(),
        7
    );
    for tab in [
        "overview",
        "architecture",
        "activity",
        "proof",
        "changes",
        "requirements",
        "files",
    ] {
        assert!(INTELLIGENCE_APP_PAGE.contains(&format!("data-workspace-tab=\"{tab}\"")));
    }
    assert!(!INTELLIGENCE_APP_PAGE.contains("data-workspace-tab=\"engineering\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("data-workspace-tab=\"diagnostics\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("data-workspace-tab=\"quality\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-workspace-panel=\"requirements\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("context-strip"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("sticky-chrome"));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"architecture-layout\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"architecture-workbench stage-surface\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"dependency-ledger\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"drawer-stack\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"global-controls\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"theme\" class=\"compact-action theme-toggle\""));
    assert!(
        INTELLIGENCE_APP_PAGE.contains("id=\"language\" class=\"compact-action language-toggle\"")
    );
    assert!(
        INTELLIGENCE_APP_PAGE.contains("id=\"autoRefresh\" class=\"compact-action live-toggle\"")
    );
    assert!(!INTELLIGENCE_APP_PAGE.contains("<select id=\"language\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("<select id=\"theme\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("type=\"checkbox\" checked"));
    assert!(INTELLIGENCE_JS.contains("architectureView: \"blueprint\""));
    assert!(INTELLIGENCE_JS.contains("function systemMapTiers"));
    assert!(INTELLIGENCE_JS.contains("function renderSubsystemInspector"));
    assert!(INTELLIGENCE_JS.contains("function focusSelectedSubsystemCard"));
    assert!(INTELLIGENCE_JS.contains("function focusSelectedComponentCard"));
    assert!(INTELLIGENCE_JS.contains("function focusSelectedRequirement"));
    assert!(INTELLIGENCE_JS.contains("function renderArchitectureInspector"));
    assert!(INTELLIGENCE_JS.contains("function subsystemDependencyIds"));
    assert!(INTELLIGENCE_JS.contains("Orchestration"));
    assert!(INTELLIGENCE_JS.contains("Composition"));
    assert!(INTELLIGENCE_JS.contains("Consumers"));
    assert!(INTELLIGENCE_JS.contains("Foundation"));
    assert!(INTELLIGENCE_JS.contains("function clampSystemMapScale"));
    assert!(INTELLIGENCE_JS.contains("function setSystemMapFull"));
    assert!(INTELLIGENCE_JS.contains("function fitSystemMap"));
    assert!(INTELLIGENCE_JS.contains("systemMapFit: true"));
    assert!(INTELLIGENCE_JS.contains("state.systemMapFit = fit"));
    assert!(INTELLIGENCE_JS.contains("scale = Math.min(1, availableWidth / Math.max(1, rawWidth))"));
    assert!(!INTELLIGENCE_JS.contains("availableHeight = Math.max(420"));
    assert!(INTELLIGENCE_JS.contains("els.systemMapFit?.addEventListener(\"click\", fitSystemMap)"));
    assert!(INTELLIGENCE_JS.contains("els.systemMapFit.setAttribute(\"aria-pressed\""));
    assert!(INTELLIGENCE_JS.contains("els.systemMapFull.setAttribute(\"aria-pressed\""));
    assert!(INTELLIGENCE_CSS.contains(".system-map-controls button[aria-pressed=\"true\"]"));
    assert!(INTELLIGENCE_JS.contains("dependency-row"));
    assert!(INTELLIGENCE_JS.contains("function renderEngineeringFlow"));
    assert!(INTELLIGENCE_JS.contains("engineering-cycle-main"));
    assert!(INTELLIGENCE_JS.contains("cycle-support-grid"));
    assert!(INTELLIGENCE_JS.contains("function renderTraceabilityMap"));
    assert!(INTELLIGENCE_JS.contains("trace-flow"));
    assert!(INTELLIGENCE_JS.contains("Mapped"));
    assert!(INTELLIGENCE_JS.contains("Executed"));
    assert!(INTELLIGENCE_JS.contains("Passed"));
    assert!(INTELLIGENCE_JS.contains("Fresh"));
    assert!(INTELLIGENCE_JS.contains("function renderChangeConvergenceMap"));
    assert!(INTELLIGENCE_JS.contains("change-convergence-flow"));
    assert!(INTELLIGENCE_JS.contains("affected_components"));
    assert!(INTELLIGENCE_JS.contains("impact-signal-strip"));
    assert!(INTELLIGENCE_JS.contains("change-impact-grid"));
    assert!(INTELLIGENCE_JS.contains("runtime-status-grid"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("data-arch-view="));
    assert!(!INTELLIGENCE_JS.contains("data-arch-view"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("architectureMetrics"));
    assert!(!INTELLIGENCE_JS.contains("architectureMetrics"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("projectIdentity"));
    assert!(!INTELLIGENCE_JS.contains("projectIdentity"));
    assert!(INTELLIGENCE_JS.contains("architectureView: \"blueprint\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-arch-back=\"blueprint\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("id=\"architectureZoomOut\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("id=\"architectureFit\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("id=\"architectureRelationControls\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"systemMapFit\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"systemMapZoomOut\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"systemMapZoomIn\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"systemMapFull\""));
    assert!(
        INTELLIGENCE_APP_PAGE.contains("id=\"precisionProviders\" class=\"context-provider-list\"")
    );
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"workspace-hero-motto\""));
    assert!(INTELLIGENCE_APP_PAGE
        .contains("id=\"engineeringFlow\" class=\"engineering-cycle-diagram\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"traceabilityMap\" class=\"traceability-map\""));
    assert!(INTELLIGENCE_APP_PAGE
        .contains("id=\"changeConvergenceMap\" class=\"change-convergence-map\""));
    assert!(INTELLIGENCE_CSS.contains(".global-bar"));
    assert!(INTELLIGENCE_CSS.contains("position:sticky"));
    assert!(
        INTELLIGENCE_CSS.contains("grid-template-columns:326px minmax(360px,394px) minmax(0,1fr)")
    );
    assert!(
        INTELLIGENCE_CSS.contains("height:auto;min-height:96px"),
        "The hero must grow when workspace names wrap."
    );
    assert!(INTELLIGENCE_CSS.contains("min-height:38px;padding:7px 22px"));
    assert!(INTELLIGENCE_APP_PAGE.contains("<kbd>⌘ K</kbd>"));
    assert!(!INTELLIGENCE_CSS.contains(".sticky-chrome"));
    assert!(INTELLIGENCE_CSS.contains("--page-gutter-x:clamp(32px,2.6vw,44px)"));
    assert!(INTELLIGENCE_CSS.contains("--page-gutter-top:24px"));
    assert!(INTELLIGENCE_CSS.contains("padding-inline:var(--page-gutter-x)"));
    assert!(INTELLIGENCE_CSS.contains(".workspace-hero"));
    assert!(INTELLIGENCE_APP_PAGE
        .contains("id=\"diagnosticsSection\" class=\"secondary-workspace-section"));
    assert!(
        INTELLIGENCE_APP_PAGE.contains("id=\"qualitySection\" class=\"secondary-workspace-section")
    );
    for id in [
        "activitySection",
        "proofSection",
        "requirementsSection",
        "changesSection",
        "filesSection",
        "diagnosticsSection",
        "qualitySection",
    ] {
        assert!(!INTELLIGENCE_APP_PAGE.contains(&format!("<details id=\"{id}\"")));
    }
    assert!(!INTELLIGENCE_JS.contains("details[data-workspace-panel]"));
    assert!(!INTELLIGENCE_JS.contains("tagName === \"DETAILS\""));
    assert!(!INTELLIGENCE_CSS.contains(".work-section,.disclosure"));
    assert!(INTELLIGENCE_CSS.contains(".secondary-section-head"));
    assert!(INTELLIGENCE_CSS.contains(".quality-table-wrap .table td:first-child{position:sticky"));
    assert!(INTELLIGENCE_CSS.contains(".activity-ledger-head,.resource-telemetry-head"));
    assert!(INTELLIGENCE_CSS.contains(".workspace-tabs{display:flex"));
    assert!(INTELLIGENCE_CSS.contains("overscroll-behavior-inline:contain"));
    assert!(INTELLIGENCE_CSS.contains("@media (max-width:1240px)"));
    assert!(INTELLIGENCE_CSS.contains("@media (max-width:1680px)"));
    assert!(INTELLIGENCE_CSS.contains(".global-controls{grid-column:2;grid-row:1"));
    assert!(INTELLIGENCE_CSS.contains(".project-navigator{grid-column:1/-1;grid-row:2"));
    assert!(INTELLIGENCE_CSS
        .contains(".workspace-switcher{display:grid;grid-template-columns:auto minmax(0,1fr)"));
    assert!(INTELLIGENCE_JS.contains("visibleSources = sourceItems.length <= 2"));
    assert!(INTELLIGENCE_JS.contains("sourceItems = [...providers, \"wcode-design\"]"));
    assert!(INTELLIGENCE_CSS.contains(".system-map"));
    assert!(INTELLIGENCE_CSS.contains(".system-tier-grid"));
    assert!(INTELLIGENCE_CSS.contains("grid-template-columns:168px minmax(0,1fr)"));
    assert!(INTELLIGENCE_CSS.contains("font-size:15px;font-weight:700;line-height:1.25"));
    assert!(INTELLIGENCE_CSS.contains(".system-tier-head strong{font-size:15px"));
    assert!(INTELLIGENCE_CSS.contains(".subsystem-inspector-quick-stats"));
    assert!(INTELLIGENCE_CSS
        .contains(".architecture-layout.full-map .stage-surface{min-height:calc(100dvh - 292px)"));
    assert!(INTELLIGENCE_JS.contains("subsystem-inspector-quick-stats"));
    assert!(!INTELLIGENCE_JS.contains("localized(\"Coverage\", \"覆盖\")"));
    assert!(INTELLIGENCE_CSS.contains(".architecture-canvas{padding:16px;overflow:auto"));
    assert!(INTELLIGENCE_CSS.contains(".root-branch,.tier-flow-bridge{position:relative;width:1px"));
    assert!(!INTELLIGENCE_CSS.contains("padding-left:180px;padding-right:10px"));
    assert!(INTELLIGENCE_CSS.contains("-webkit-line-clamp:2"));
    assert!(INTELLIGENCE_CSS.contains(".subsystem-card"));
    assert!(INTELLIGENCE_CSS.contains(".dependency-ledger"));
    assert!(INTELLIGENCE_CSS.contains(".subsystem-inspector-section"));
    assert!(INTELLIGENCE_CSS.contains(
        ".architecture-layout{display:grid;grid-template-columns:minmax(0,1fr) minmax(420px,31%)"
    ));
    assert!(INTELLIGENCE_CSS.contains(
        ".proof-main-grid{display:grid;grid-template-columns:minmax(0,1fr) minmax(500px,37%)"
    ));
    assert!(INTELLIGENCE_CSS
        .contains(".proof-metric-strip{display:grid;grid-template-columns:repeat(4"));
    assert!(INTELLIGENCE_CSS.contains(".evidence-ledger-card"));
    assert!(INTELLIGENCE_CSS.contains(".evidence-inspector-card"));
    assert!(
        INTELLIGENCE_CSS.contains(".access-card:nth-child(2n){border-left:1px solid var(--line);}")
    );
    assert!(INTELLIGENCE_CSS.contains(".large-file.over-limit{box-shadow:inset 2px 0 0 var(--bad)"));
    assert!(INTELLIGENCE_CSS.contains(
        "grid-template-columns:78px 118px minmax(150px,1.05fr) 90px 92px 128px minmax(110px,.78fr)"
    ));
    assert!(INTELLIGENCE_APP_PAGE.contains("class=\"compact-action theme-toggle\""));
    assert!(!INTELLIGENCE_APP_PAGE.contains("data-theme-value="));
    assert!(INTELLIGENCE_CSS.contains("--font-body:16px"));
    assert!(INTELLIGENCE_CSS.contains("--font-caption:12px"));
    assert!(INTELLIGENCE_CSS.contains("--shadow-soft:0 10px 34px rgba(0,0,0,.11)"));
    assert!(!INTELLIGENCE_CSS.contains("#55e6a5"));
    assert!(INTELLIGENCE_CSS.contains(".table th{position:sticky;top:0;z-index:2"));
    assert!(INTELLIGENCE_CSS.contains(".empty{position:relative;padding:20px 14px"));
    assert!(
        INTELLIGENCE_CSS.contains(".convergence-flow{display:grid;grid-template-columns:repeat(5")
    );
    assert!(INTELLIGENCE_CSS.contains(".arch-node-group-label"));
    assert!(INTELLIGENCE_JS.contains("arch-node-group"));
    assert!(!INTELLIGENCE_CSS.contains(".blueprint-edge"));
    assert!(!INTELLIGENCE_CSS.contains(".arch-edge"));
    assert!(!INTELLIGENCE_CSS.contains(".flow-arrow"));
    assert!(!INTELLIGENCE_CSS.contains(".story-arrow"));
    assert!(!INTELLIGENCE_CSS.contains(".runtime-edge"));
    assert!(INTELLIGENCE_APP_PAGE.contains("role=\"tablist\""));
    assert!(INTELLIGENCE_APP_PAGE
        .contains("class=\"global-status\" role=\"status\" aria-live=\"polite\""));
    let controls = INTELLIGENCE_APP_PAGE
        .find("<div class=\"global-controls\"")
        .unwrap();
    let sync = INTELLIGENCE_APP_PAGE
        .find("<section class=\"global-status\"")
        .unwrap();
    let workspace = INTELLIGENCE_APP_PAGE
        .find("<label class=\"workspace-switcher\"")
        .unwrap();
    assert!(controls < sync && sync < workspace);
    assert!(!INTELLIGENCE_CSS.contains(".global-status{display:none"));
    assert!(!INTELLIGENCE_CSS.contains(".global-status{position:absolute!important"));
    assert!(!INTELLIGENCE_CSS.contains("clip-path:inset(50%)"));
    assert!(INTELLIGENCE_JS.contains("parentElement?.setAttribute(\"aria-label\", label)"));
    assert!(INTELLIGENCE_JS.contains("Agent context efficiency"));
    assert!(INTELLIGENCE_JS.contains("RepoMap delivered / candidates"));
    assert!(INTELLIGENCE_JS.contains("Context budget utilization"));
    assert!(INTELLIGENCE_JS.contains("budget_tokens"));
    assert!(INTELLIGENCE_JS.contains("not a quality score"));
    assert!(INTELLIGENCE_APP_PAGE.contains("role=\"combobox\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("aria-autocomplete=\"list\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("aria-labelledby=\"tabArchitecture\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("aria-selected=\"true\""));
    assert!(INTELLIGENCE_JS.contains("function activateWorkspaceTab"));
    assert!(INTELLIGENCE_JS.contains("function renderWorkspaceHero"));
    assert!(INTELLIGENCE_JS.contains("evidence-ledger-row"));
    assert!(INTELLIGENCE_JS.contains("selectedEvidenceKey"));
    assert!(INTELLIGENCE_JS.contains("evidenceInspectorOpen"));
    assert!(INTELLIGENCE_JS.contains("data-evidence-prev"));
    assert!(INTELLIGENCE_JS.contains("data-evidence-next"));
    assert!(INTELLIGENCE_JS.contains("data-evidence-close"));
    assert!(INTELLIGENCE_JS.contains("aria-pressed=\"${active}\""));
    assert!(INTELLIGENCE_JS.contains("[\"ArrowDown\", \"ArrowUp\", \"Home\", \"End\"]"));
    assert!(INTELLIGENCE_JS
        .contains("results.find(item => item.getAttribute(\"aria-selected\") === \"true\")"));
    assert!(INTELLIGENCE_JS.contains("aria-activedescendant"));
    assert!(INTELLIGENCE_JS.contains("activeTabButton?.scrollIntoView"));
    assert!(INTELLIGENCE_JS.contains("[\"ArrowDown\", \"ArrowUp\", \"Home\", \"End\"]"));
    assert!(INTELLIGENCE_JS.contains("navigatorKindLabel"));
    assert!(INTELLIGENCE_JS.contains("activate(buttons[nextIndex], { focus: true })"));
    assert!(!INTELLIGENCE_JS.contains("\"△\""));
    for glyph in ["⌕", "⌄", "⌃", "‹", "›", "×", "＋", "−"] {
        assert!(
            !INTELLIGENCE_APP_PAGE.contains(glyph) && !INTELLIGENCE_JS.contains(glyph),
            "font-dependent control glyph leaked into Observatory UI: {glyph}"
        );
    }
    assert!(!INTELLIGENCE_CSS.contains(".drawer-stack>.work-section.workspace-panel>summary"));
    for micro in [
        "font-size:7px",
        "font-size:8px",
        "font-size:9px",
        "font-size:10px",
        "font-size:11px",
        "font-size: 7px",
        "font-size: 8px",
        "font-size: 9px",
        "font-size: 10px",
        "font-size: 11px",
        "font:7px",
        "font:8px",
        "font:9px",
        "font:10px",
        "font:11px",
        "font: 7px",
        "font: 8px",
        "font: 9px",
        "font: 10px",
        "font: 11px",
    ] {
        assert!(
            !INTELLIGENCE_CSS.contains(micro),
            "micro typography leaked into Observatory CSS: {micro}"
        );
    }
    assert!(!INTELLIGENCE_CSS.contains(".metric-progress"));
}

#[test]
fn observatory_assets_support_incremental_refresh_precision_and_light_mode() {
    assert!(INTELLIGENCE_APP_PAGE.contains("/intelligence/app.css"));
    assert!(INTELLIGENCE_APP_PAGE.contains("/intelligence/app.js"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"theme\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"refreshSemantic\""));
    assert!(INTELLIGENCE_CSS.contains("prefers-color-scheme:light"));
    assert!(INTELLIGENCE_CSS.contains("html[data-theme=\"light\"]"));
    assert!(INTELLIGENCE_CSS.contains("--accent:#8b7cff"));
    assert!(INTELLIGENCE_CSS.contains("--accent-secondary:#f05aa6"));
    assert!(INTELLIGENCE_APP_PAGE.contains("theme-color\" content=\"#0b0812"));
    assert!(INTELLIGENCE_JS.contains("/intelligence/semantic-refresh"));
    assert!(INTELLIGENCE_JS.contains("/intelligence/revision"));
    assert!(INTELLIGENCE_JS.contains("projectCache"));
    assert!(INTELLIGENCE_JS.contains("X-Wcode-Prefer-Cached"));
    assert!(INTELLIGENCE_JS.contains("snapshot_pending"));
    assert!(INTELLIGENCE_JS.contains("preserveDom: hasCachedSnapshot"));
    assert!(INTELLIGENCE_JS.contains("graph_precision"));
    assert!(INTELLIGENCE_JS.contains("retry_in_seconds"));
    assert!(INTELLIGENCE_JS.contains("death_count"));
    assert!(INTELLIGENCE_JS.contains("tunnel.role || tunnel.state"));
    assert!(INTELLIGENCE_JS.contains("const dashboardUrl = tunnelDashboardUrl(tunnel)"));
    assert!(INTELLIGENCE_JS.contains("if (dashboardUrl)"));
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
fn observatory_exposes_execution_state_without_chat_history() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"executionStatus\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("DURABLE EXECUTION"));
    assert!(INTELLIGENCE_APP_PAGE
        .contains("Read-only projection of objective, progress, plan and proof state."));
    assert!(INTELLIGENCE_JS.contains("function renderExecutionStatus()"));
    assert!(INTELLIGENCE_JS.contains("state.project?.execution"));
    assert!(INTELLIGENCE_JS.contains("checkpoint.reconciliation_plan_id"));
    assert!(INTELLIGENCE_JS.contains("checkpoint.verification_plan_id"));
    assert!(INTELLIGENCE_JS.contains("checkpoint.repository_revision"));
    assert!(INTELLIGENCE_CSS.contains(".execution-observatory-panel"));
    assert!(INTELLIGENCE_CSS.contains(".execution-shell::before"));
    assert!(!INTELLIGENCE_APP_PAGE.contains("executionTranscript"));
    assert!(!INTELLIGENCE_JS.contains("executionTranscript"));
}

#[test]
fn observatory_exposes_execution_steering_without_transcript_state() {
    assert!(INTELLIGENCE_JS.contains("execution.pending_directive"));
    assert!(INTELLIGENCE_JS.contains("execution.replan_required"));
    assert!(INTELLIGENCE_JS.contains("execution.verification_floor"));
    assert!(INTELLIGENCE_JS.contains("execution.lineage"));
    assert!(INTELLIGENCE_JS.contains("Steering / handoff"));
    assert!(INTELLIGENCE_JS.contains("Verification floor"));
    assert!(INTELLIGENCE_JS.contains("Handoff lineage"));
    assert!(INTELLIGENCE_CSS.contains(".execution-steering-card"));
    assert!(INTELLIGENCE_CSS.contains(".execution-steering-facts"));
    assert!(!INTELLIGENCE_JS.contains("execution.messages"));
    assert!(!INTELLIGENCE_JS.contains("execution.transcript"));
}

#[test]
fn observatory_exposes_engineering_digital_twin_code_graph() {
    assert_eq!(
        INTELLIGENCE_APP_PAGE.matches("data-workspace-tab=").count(),
        7
    );
    assert!(!INTELLIGENCE_APP_PAGE.contains("id=\"tabCodeGraph\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("data-architecture-view=\"codegraph\""));
    for marker in [
        "id=\"codeGraphSection\"",
        "id=\"codeGraphSearch\"",
        "id=\"codeGraphMap\"",
        "id=\"codeGraphInspector\"",
    ] {
        assert!(INTELLIGENCE_APP_PAGE.contains(marker), "missing {marker}");
    }
    assert!(INTELLIGENCE_JS.contains("/intelligence/code-graph"));
    assert!(INTELLIGENCE_JS.contains("renderCodeGraphInspector"));
    assert!(INTELLIGENCE_JS.contains("Why this node is related"));
    assert!(INTELLIGENCE_JS.contains("precision_counts"));
    assert!(INTELLIGENCE_JS.contains("edge.provenance?.provider"));
    assert!(INTELLIGENCE_JS.contains("edge.provenance?.precision"));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"codeGraphSnapshot\""));
    assert!(INTELLIGENCE_JS.contains("snapshot_id"));
    assert!(INTELLIGENCE_JS.contains("Latest graph"));
    assert!(INTELLIGENCE_CSS.contains(".code-graph-diagram"));
    assert!(INTELLIGENCE_CSS.contains(".code-graph-edge-path"));
    assert!(INTELLIGENCE_CSS.contains(".code-graph-svg-node"));
    assert!(INTELLIGENCE_CSS.contains(".code-graph-why"));
}

#[test]
fn observatory_visual_grammar_keeps_state_proof_change_and_runtime_scannable() {
    for selector in [
        ".status-summary::before",
        ".attention-item{",
        ".activity-row::before",
        ".runtime-status-card::after",
        ".evidence-inspector-section h4::before",
        ".req.selected",
        ".change-table tbody tr:hover",
        ".code-graph-svg-node.selected rect",
    ] {
        assert!(
            INTELLIGENCE_CSS.contains(selector),
            "missing observability selector: {selector}"
        );
    }
    for micro in [
        "font-size:10px",
        "font-size:11px",
        "font:10px",
        "font:11px",
        "font: 10px",
        "font: 11px",
    ] {
        assert!(
            !INTELLIGENCE_CSS.contains(micro),
            "micro observability typography leaked into production CSS: {micro}"
        );
    }
}

#[test]
fn observatory_narrow_layout_stacks_global_bar_and_page_scrolls_proof_inspector() {
    for rule in [
        ".global-bar{grid-template-columns:minmax(0,1fr);align-items:stretch;gap:8px;}",
        ".project-navigator{grid-column:1;grid-row:2;width:100%;min-width:0;max-width:100%;}",
        ".global-controls{grid-column:1;grid-row:3;grid-template-columns:minmax(0,1fr) 58px repeat(3,44px);width:100%;min-width:0;max-width:100%;gap:4px;}",
        ".evidence-inspector-card{position:static;top:auto;width:100%;min-width:0;max-width:100%;max-height:none;overflow:visible;scrollbar-gutter:auto;overscroll-behavior:auto;align-self:start;}",
    ] {
        assert!(INTELLIGENCE_CSS.contains(rule), "missing narrow responsive contract: {rule}");
    }
}

#[test]
fn observatory_exposes_file_structure_and_largest_files() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"fileTree\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"largeFiles\""));
    assert!(INTELLIGENCE_JS.contains("renderProjectStructure"));
    assert!(INTELLIGENCE_JS.contains("structure.entries"));
    assert!(INTELLIGENCE_JS.contains("line_limit"));
    assert!(INTELLIGENCE_JS.contains("file.generated"));
    assert!(INTELLIGENCE_JS.contains("generated · line limit exempt"));
    assert!(INTELLIGENCE_CSS.contains(".file-tree"));
}

#[test]
fn observatory_explains_verification_selection_provenance() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"verificationImpact\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("Verification impact"));
    assert!(INTELLIGENCE_JS.contains("renderVerificationImpact"));
    assert!(INTELLIGENCE_JS.contains("verification_impact"));
    assert!(INTELLIGENCE_JS.contains("contract_bridge"));
    assert!(INTELLIGENCE_JS.contains("manifest_dependency"));
    assert!(INTELLIGENCE_JS.contains("broad_fallback"));
    assert!(INTELLIGENCE_JS.contains("reason.provider"));
    assert!(INTELLIGENCE_JS.contains("reason.precision"));
}

#[test]
fn observatory_exposes_adaptive_verification_as_a_non_executing_preview() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"adaptiveVerification\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("Adaptive verification"));
    assert!(INTELLIGENCE_JS.contains("renderAdaptiveVerification"));
    assert!(INTELLIGENCE_JS.contains("adaptive_verification"));
    assert!(INTELLIGENCE_JS.contains("focused_test"));
    assert!(INTELLIGENCE_JS.contains("cost_sentinel"));
    assert!(INTELLIGENCE_JS.contains("Fail-fast frontier"));
    assert!(INTELLIGENCE_JS.contains("marginal_failures"));
    assert!(INTELLIGENCE_JS.contains("estimated_total_savings_ms"));
    assert!(INTELLIGENCE_JS.contains("activation_state"));
    assert!(INTELLIGENCE_JS.contains("activation_reason"));
    assert!(INTELLIGENCE_JS.contains("Cost-model replay"));
    assert!(INTELLIGENCE_JS.contains("cost_backtest_non_positive_net_savings"));
    assert!(INTELLIGENCE_JS.contains("full coverage unchanged"));
    assert!(INTELLIGENCE_JS.contains("Planning preview only"));
    assert!(INTELLIGENCE_JS.contains("no_strong_adaptive_evidence"));
    assert!(INTELLIGENCE_JS.contains("quick_verification_gap"));
}

#[test]
fn observatory_exposes_prompt_free_temporal_verified_learning_metrics() {
    assert!(INTELLIGENCE_APP_PAGE.contains("id=\"verifiedLearning\""));
    assert!(INTELLIGENCE_APP_PAGE.contains("Verified learning"));
    assert!(INTELLIGENCE_JS.contains("renderVerifiedLearning"));
    assert!(INTELLIGENCE_JS.contains("verified_learning"));
    assert!(INTELLIGENCE_JS.contains("global-temporal-ab-v2"));
    assert!(INTELLIGENCE_JS.contains("verified-context-cochange-v3"));
    assert!(INTELLIGENCE_JS.contains("raw-count-v1"));
    assert!(INTELLIGENCE_JS.contains("precision_at_k_percent"));
    assert!(INTELLIGENCE_JS.contains("baseline_precision_at_k_percent"));
    assert!(INTELLIGENCE_JS.contains("precision_at_k_delta_percent_points"));
    assert!(INTELLIGENCE_JS.contains("recall_at_k_percent"));
    assert!(INTELLIGENCE_JS.contains("baseline_recall_at_k_percent"));
    assert!(INTELLIGENCE_JS.contains("stale_path_references"));
    assert!(INTELLIGENCE_JS.contains("prompt / chain-of-thought free"));
}

#[test]
fn observatory_html_page_parses_without_errors() {
    fn first_error<'a>(node: tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        if node.is_error() || node.is_missing() {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(error) = first_error(child) {
                return Some(error);
            }
        }
        None
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(INTELLIGENCE_APP_PAGE, None).unwrap();
    if let Some(error) = first_error(tree.root_node()) {
        panic!(
            "observatory HTML contains a syntax error at {:?}..{:?}: {}",
            error.start_position(),
            error.end_position(),
            error.to_sexp()
        );
    }
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
        "min-height:100dvh",
        "safe-area-inset-bottom",
        "@media (max-width:720px)",
        "@media (max-width:900px) and (pointer:coarse)",
        "scroll-snap-type:x mandatory",
        ".change-table td::before",
        "font-size:16px",
        ".access-panel:not(.hidden)",
        ".workspace-tabs button,.filter,.system-map-controls button,.evidence-inspector-controls button,.inspector-collapse{min-height:44px",
        "overflow-x:auto;scrollbar-width:none;overscroll-behavior-inline:contain",
    ] {
        assert!(INTELLIGENCE_CSS.contains(contract), "missing {contract}");
    }
    assert!(
        INTELLIGENCE_CSS.rfind("@media (max-width:720px)")
            > INTELLIGENCE_CSS.rfind(".structure-panel{")
    );

    for contract in [
        "function setAccessPanel",
        "function accessPanelOpen",
        "function renderWorkspaceHero",
        "systemThemeQuery",
        "scrollIntoView({ behavior: \"smooth\"",
    ] {
        assert!(INTELLIGENCE_JS.contains(contract), "missing {contract}");
    }
}
