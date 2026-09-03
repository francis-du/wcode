use super::*;

const TUI_GRAPH_FILES: usize = 1_500;
const TUI_GRAPH_SYMBOLS: usize = 5_000;
const TUI_STATUS_ITEMS: usize = 500;

pub(super) fn request_intelligence_refresh(
    monitor: &TaskMonitor,
    config: &MonitorConfig,
    workspace_id: String,
) {
    monitor.register_workspace(workspace_id.clone());
    if !monitor.begin_intelligence_refresh(&workspace_id) {
        return;
    }
    let refresh_monitor = monitor.clone();
    let failure_monitor = monitor.clone();
    let harness = config.harness.clone();
    let workspaces = config.workspaces.clone();
    let failure_workspace = workspace_id.clone();
    let spawn = std::thread::Builder::new()
        .name("wcode-intelligence".to_owned())
        .spawn(move || {
            let error =
                refresh_intelligence_now(&refresh_monitor, &harness, &workspaces, &workspace_id)
                    .err()
                    .map(|error| error.to_string());
            refresh_monitor.finish_intelligence_refresh(&workspace_id, error);
        });
    if let Err(error) = spawn {
        failure_monitor.finish_intelligence_refresh(
            &failure_workspace,
            Some(format!("cannot start refresh: {error}")),
        );
    }
}

pub(super) fn refresh_intelligence_now(
    monitor: &TaskMonitor,
    harness: &ToolHarness,
    workspaces: &Workspaces,
    workspace_id: &str,
) -> anyhow::Result<()> {
    monitor.register_workspace(workspace_id.to_owned());
    let (_, workspace) = workspaces.select(Some(workspace_id))?;
    let mut errors = Vec::new();
    record_refresh(
        monitor,
        workspace_id,
        "design_status",
        harness.design_status(workspace_id, &workspace),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "traceability_status",
        harness.traceability_status(workspace_id, &workspace),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "scope_status",
        harness.product_scope_status(&workspace),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "software_graph",
        harness.software_graph(
            workspace_id,
            &workspace,
            ".",
            TUI_GRAPH_FILES,
            TUI_GRAPH_SYMBOLS,
        ),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "semantic_status",
        harness.semantic_status(workspace_id, &workspace, TUI_STATUS_ITEMS),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "semantic_provider_status",
        harness.semantic_provider_status(&workspace),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "semantic_session_status",
        Ok(harness.semantic_session_status(&workspace)),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "graph_provider_status",
        harness.graph_provider_status(&workspace),
        &mut errors,
    );
    record_refresh(
        monitor,
        workspace_id,
        "evidence_status",
        harness.evidence_status(workspace_id, &workspace, None, TUI_STATUS_ITEMS),
        &mut errors,
    );
    refresh_graph_diff(monitor, harness, workspace_id, &workspace, &mut errors);
    refresh_verification(monitor, harness, workspace_id, &workspace, &mut errors);
    refresh_reconciliation(monitor, harness, workspace_id, &workspace, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(errors.join(" · ")))
    }
}

fn record_refresh<T: serde::Serialize>(
    monitor: &TaskMonitor,
    workspace_id: &str,
    tool: &'static str,
    result: anyhow::Result<T>,
    errors: &mut Vec<String>,
) {
    match result.and_then(|value| serde_json::to_value(value).map_err(anyhow::Error::from)) {
        Ok(value) => monitor.record_intelligence_result(workspace_id, tool, &value),
        Err(error) => errors.push(format!("{tool}: {error}")),
    }
}

fn refresh_graph_diff(
    monitor: &TaskMonitor,
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &crate::workspace::Workspace,
    errors: &mut Vec<String>,
) {
    match harness.graph_history(workspace, 2) {
        Ok(history) if history.len() >= 2 => record_refresh(
            monitor,
            workspace_id,
            "graph_diff",
            harness.graph_diff(
                workspace,
                &crate::graph_store::GraphDiffInput {
                    from_snapshot_id: None,
                    to_snapshot_id: None,
                    limit: 200,
                },
            ),
            errors,
        ),
        Ok(_) => {}
        Err(error) => errors.push(format!("graph_history: {error}")),
    }
}

fn refresh_verification(
    monitor: &TaskMonitor,
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &crate::workspace::Workspace,
    errors: &mut Vec<String>,
) {
    match harness.verification_history(workspace_id, workspace, 1) {
        Ok(history) => {
            if let Some(status) = history.first() {
                record_refresh(
                    monitor,
                    workspace_id,
                    "verification_status",
                    Ok(status),
                    errors,
                );
            }
        }
        Err(error) => errors.push(format!("verification_history: {error}")),
    }
}

fn refresh_reconciliation(
    monitor: &TaskMonitor,
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &crate::workspace::Workspace,
    errors: &mut Vec<String>,
) {
    match harness.reconciliation_history(workspace, 1) {
        Ok(history) => {
            if let Some(plan) = history.first() {
                record_refresh(
                    monitor,
                    workspace_id,
                    "reconciliation_execution_status",
                    harness.reconciliation_execution_status(workspace_id, workspace, &plan.id),
                    errors,
                );
            }
        }
        Err(error) => errors.push(format!("reconciliation_history: {error}")),
    }
}

pub(super) fn render_intelligence_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    focus: usize,
    language: UiLanguage,
) {
    let width = area.width.saturating_sub(4).min(104);
    let height = area.height.saturating_sub(4).min(24);
    if width < 36 || height < 12 {
        return;
    }
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let workspaces = configured_workspaces(config);
    let workspace = workspaces.get(focus.min(workspaces.len().saturating_sub(1)));
    let workspace_id = workspace
        .map(|entry| entry.0.as_str())
        .unwrap_or("workspace");
    let root = workspace.map(|entry| entry.1.as_str()).unwrap_or(".");
    let stats = snapshot
        .intelligence
        .get(workspace_id)
        .cloned()
        .unwrap_or_default();
    let design = stats.design_state.as_deref().unwrap_or("unknown");
    let risk = stats.risk_level.as_deref().unwrap_or("unknown");
    let precision = stats.graph_precision.as_deref().unwrap_or("unknown");
    let updated = last_seen_text(stats.updated_at);
    let implementation = stats
        .implementation_coverage
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "—".into());
    let verification = stats
        .verification_coverage
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "—".into());
    let ready = stats
        .verification_ready
        .map(|ready| language.tr(if ready { "ready" } else { "blocked" }))
        .unwrap_or_else(|| language.tr("unknown"));
    let reconciliation = stats
        .reconciliation_converged
        .map(|converged| language.tr(if converged { "converged" } else { "active" }))
        .unwrap_or_else(|| language.tr("unknown"));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SECONDARY))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::uniform(1))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", language.tr("SOFTWARE INTELLIGENCE")),
                Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {workspace_id} "), Style::default().fg(TEXT)),
        ]))
        .title(
            Line::from(Span::styled(
                format!(
                    " R {}  ·  I / Esc {} ",
                    language.tr("refresh"),
                    language.tr("close")
                ),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:<12}", language.tr("ROOT")),
                Style::default().fg(TEXT_DIM),
            ),
            Span::styled(
                truncate_middle(root, inner.width.saturating_sub(12) as usize),
                Style::default().fg(TEXT_MUTED),
            ),
        ]),
        Line::from(""),
        intelligence_line(
            language.tr("DESIGN"),
            format!(
                "{design} · {} requirements · {} components",
                stats.requirements, stats.components
            ),
            if design == "valid" { SUCCESS } else { WARNING },
        ),
        intelligence_line(
            language.tr("SCOPES"),
            format!(
                "{} mapped / {} source · {} unmapped",
                stats.scope_mapped_files, stats.scope_source_files, stats.scope_unmapped_files
            ),
            if stats.scope_unmapped_files == 0 && stats.scope_source_files > 0 {
                SUCCESS
            } else if stats.scope_source_files == 0 {
                TEXT_MUTED
            } else {
                WARNING
            },
        ),
        intelligence_line(
            language.tr("TRACE"),
            format!("implementation {implementation} · verification {verification}"),
            if stats.implementation_coverage == Some(100)
                && stats.verification_coverage == Some(100)
            {
                SUCCESS
            } else {
                WARNING
            },
        ),
        intelligence_line(
            language.tr("GRAPH"),
            format!(
                "{} nodes · {} edges · {precision}",
                stats.graph_nodes, stats.graph_edges
            ),
            LINK,
        ),
        intelligence_line(
            language.tr("GRAPH Δ"),
            format!(
                "nodes +{}/-{}/~{} · edges +{}/-{}/~{}",
                stats.graph_added_nodes,
                stats.graph_removed_nodes,
                stats.graph_changed_nodes,
                stats.graph_added_edges,
                stats.graph_removed_edges,
                stats.graph_changed_edges
            ),
            SECONDARY,
        ),
        intelligence_line(
            language.tr("SEMANTICS"),
            format!(
                "{} confirmed · {} candidates",
                stats.semantic_confirmed, stats.semantic_candidates
            ),
            ACCENT,
        ),
        intelligence_line(
            "LSP",
            if config.semantic_auto {
                format!(
                    "ready {}/{} · live {} · auth {} · missing {} · auto {} · warm {}/{} · fresh {}/{}",
                    stats.lsp_launch_ready,
                    stats.lsp_available,
                    stats.lsp_validated,
                    stats.lsp_authorization_required,
                    stats.lsp_missing,
                    stats.lsp_automatic,
                    stats.lsp_sessions,
                    stats.lsp_documents,
                    stats.lsp_fresh,
                    stats.lsp_stale
                )
            } else {
                "off · Tree-sitter only".to_owned()
            },
            if !config.semantic_auto {
                TEXT_MUTED
            } else if stats.lsp_fresh > 0 {
                SUCCESS
            } else if stats.lsp_runnable > 0 {
                WARNING
            } else {
                TEXT_MUTED
            },
        ),
        intelligence_line(
            language.tr("RISK"),
            format!("{risk} · {} drift finding(s)", stats.drift_findings),
            match risk {
                "low" => SUCCESS,
                "medium" | "moderate" => WARNING,
                "high" | "critical" => DANGER,
                _ => TEXT_MUTED,
            },
        ),
        intelligence_line(
            language.tr("EVIDENCE"),
            format!(
                "{} total · {} failed · {} disagreed",
                stats.evidence_total, stats.evidence_failed, stats.evidence_disagreed
            ),
            if stats.evidence_failed > 0 || stats.evidence_disagreed > 0 {
                DANGER
            } else {
                SUCCESS
            },
        ),
        intelligence_line(
            language.tr("VERIFY"),
            format!("{ready} · {} blocker(s)", stats.verification_blockers),
            if stats.verification_ready == Some(true) {
                SUCCESS
            } else {
                WARNING
            },
        ),
        intelligence_line(
            language.tr("RECONCILE"),
            format!(
                "{reconciliation} · {} pending task(s)",
                stats.reconciliation_pending
            ),
            if stats.reconciliation_converged == Some(true) {
                SUCCESS
            } else {
                SECONDARY
            },
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{:<12}", language.tr("UPDATED")),
                Style::default().fg(TEXT_DIM),
            ),
            Span::styled(updated, Style::default().fg(TEXT_MUTED)),
        ]),
        intelligence_refresh_line(&stats, inner.width, language),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn intelligence_refresh_line(
    stats: &IntelligenceStats,
    width: u16,
    language: UiLanguage,
) -> Line<'static> {
    if stats.refreshing {
        return Line::from(Span::styled(
            language.tr("Loading current Workspace data…"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(error) = stats.refresh_error.as_deref() {
        return Line::from(vec![
            Span::styled(
                format!("{}: ", language.tr("Partial refresh")),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate_end(error, width.saturating_sub(20) as usize),
                Style::default().fg(TEXT_MUTED),
            ),
        ]);
    }
    Line::from(Span::styled(
        language.tr("R refresh · risk checks remain authorization-bound"),
        Style::default().fg(TEXT_DIM),
    ))
}

pub(super) fn intelligence_line(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(TEXT_DIM)),
        Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}
