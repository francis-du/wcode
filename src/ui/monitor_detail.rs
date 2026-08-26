use super::*;

pub(super) fn tunnel_status_text(snapshot: &MonitorSnapshot) -> &'static str {
    match snapshot.tunnel_running {
        Some(true) => "● RUNNING",
        Some(false) => "× EXITED",
        None => "○ EXTERNAL / LOCAL",
    }
}

pub(super) fn tunnel_status_color(snapshot: &MonitorSnapshot) -> Color {
    match snapshot.tunnel_running {
        Some(true) => GREEN,
        Some(false) => RED,
        None => GRAY,
    }
}

pub(super) fn initialize_status_text(snapshot: &MonitorSnapshot) -> String {
    let last = snapshot
        .last_initialize
        .map(|seen| last_seen_text(Some(seen)))
        .unwrap_or_else(|| "never".to_owned());
    format!("#{} · last {last}", snapshot.initialize_count)
}

pub(super) fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    compact: bool,
    language: UiLanguage,
) {
    let lifecycle = if is_quick_tunnel(&config.mcp_url) {
        "TEMPORARY URL"
    } else {
        "FIXED ENDPOINT"
    };
    let endpoint_mode = snapshot.public_endpoint.as_deref().unwrap_or("pending");
    let endpoint_ready = match endpoint_mode {
        "quick-tunnel" => {
            snapshot.tunnel_running == Some(true) && snapshot.public_url_healthy != Some(false)
        }
        "external" => snapshot.public_url_healthy == Some(true),
        "local-only" => true,
        "pending" => false,
        _ => false,
    };
    let endpoint_detail = if let Some(error) = snapshot.tunnel_error.as_deref() {
        format!("stopped · {}", truncate_end(error, 28))
    } else {
        match endpoint_mode {
            "quick-tunnel" | "external" => public_url_health_text(snapshot),
            "local-only" => "local only".to_owned(),
            _ => "waiting".to_owned(),
        }
    };
    let mcp_seen = snapshot.last_mcp_seen.is_some();
    let mcp_detail = if mcp_seen {
        format!("last seen {}", last_seen_text(snapshot.last_mcp_seen))
    } else {
        "waiting for request".to_owned()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(YELLOW))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("SETUP")),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                format!(" {lifecycle} "),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 78 || inner.height < 7 {
        let lines = vec![
            setup_step(1, language.tr("Open this wcode setup page")),
            setup_step(2, language.tr("Add the MCP URL and choose OAuth")),
            Line::from(vec![
                Span::styled("  MCP  ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(&config.mcp_url, inner.width.saturating_sub(8) as usize),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("  VERIFY CODE ", Style::default().fg(DIM)),
                Span::styled(
                    &config.pairing_code,
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "   ·   Works with compatible Remote MCP clients",
                    Style::default().fg(GRAY),
                ),
            ]),
            setup_state(
                snapshot.oauth_authorized,
                "OAuth",
                if snapshot.oauth_authorized {
                    "authorized"
                } else {
                    "waiting"
                },
            ),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);
    let left = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER))
        .padding(Padding::new(0, 2, 0, 0));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "GET CONNECTED",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            setup_step(1, language.tr("Open this setup page · press O")),
            setup_step(2, language.tr("Choose a compatible AI client")),
            setup_step(3, language.tr("Add MCP URL · Auth: OAuth")),
            Line::from(vec![
                Span::styled("  MCP   ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(
                        &config.mcp_url,
                        columns[0].width.saturating_sub(10) as usize,
                    ),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("  VERIFY CODE  ", Style::default().fg(DIM)),
                Span::styled(
                    &config.pairing_code,
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   press O to reopen setup", Style::default().fg(GRAY)),
            ]),
        ])
        .block(left),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "CONNECTION",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            setup_state(true, "Local server", "ready"),
            setup_state(endpoint_ready, "Public endpoint", &endpoint_detail),
            setup_state(
                snapshot.oauth_client_registered,
                "OAuth client",
                if snapshot.oauth_client_registered {
                    "registered"
                } else {
                    "waiting"
                },
            ),
            setup_state(
                snapshot.oauth_authorized,
                "OAuth",
                if snapshot.oauth_authorized {
                    "authorized"
                } else {
                    "waiting"
                },
            ),
            setup_state(mcp_seen, "MCP", &mcp_detail),
            setup_state(snapshot.chatgpt_connected, "MCP client", "connected"),
            Line::from(vec![
                Span::styled("  LAST  ", Style::default().fg(DIM)),
                Span::styled(
                    last_seen_text(snapshot.last_mcp_seen),
                    Style::default().fg(GRAY),
                ),
                Span::styled("   ·   Remote MCP", Style::default().fg(PURPLE)),
            ]),
        ]),
        columns[1],
    );
}

fn setup_step(number: u8, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {number} "),
            Style::default()
                .fg(BG)
                .bg(PURPLE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {label}"), Style::default().fg(WHITE)),
    ])
}

fn setup_state(done: bool, label: &str, detail: &str) -> Line<'static> {
    let color = if done { GREEN } else { DIM };
    Line::from(vec![
        Span::styled(
            if done { "  ● " } else { "  ○ " },
            Style::default().fg(color),
        ),
        Span::styled(format!("{label:<14}"), Style::default().fg(WHITE)),
        Span::styled(detail.to_owned(), Style::default().fg(color)),
    ])
}

pub(super) fn render_workspace_activity(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    ui: &DashboardState,
) {
    let workspaces = configured_workspaces(config);
    let total = workspaces.len();
    let visible = workspace_column_count(area.width, total);
    let offset = ui.workspace_offset.min(total.saturating_sub(visible));
    let end = (offset + visible).min(total);
    let range = if total == 0 {
        "0 / 0".to_owned()
    } else {
        format!("{}–{} / {}", offset + 1, end, total)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", ui.language.tr("WORKSPACE ACTIVITY")),
            Style::default().fg(GRAY).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(vec![
                Span::styled("VIEW  ", Style::default().fg(DIM)),
                Span::styled(
                    range,
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ← → ", Style::default().fg(DIM)),
            ])
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if total == 0 || inner.width == 0 || inner.height == 0 {
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        ui.language.tr("No workspaces configured"),
                        Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        ui.language
                            .tr("restart wcode with one or more --workspace paths"),
                        Style::default().fg(GRAY),
                    )),
                ])
                .alignment(ratatui::layout::Alignment::Center),
                inner,
            );
        }
        return;
    }

    let column_count = end.saturating_sub(offset).max(1);
    let columns = split_rects_with_gap(inner, column_count, 1);
    let now = Instant::now();

    for (column, workspace_index) in columns.iter().zip(offset..end) {
        let (id, path, is_default) = &workspaces[workspace_index];
        let stats = snapshot.workspaces.get(id).cloned().unwrap_or_default();
        let active = stats.active > 0;
        let queued = stats.queued > 0;
        let focused = workspace_index == ui.workspace_focus;
        let status_color = if active {
            CYAN
        } else if queued {
            YELLOW
        } else if stats.failed > 0 {
            RED
        } else {
            DIM
        };
        let border_color = if focused {
            BLUE
        } else if active {
            CYAN
        } else {
            BORDER
        };
        let summary = if active || queued {
            format!("{} run · {} wait", stats.active, stats.queued)
        } else {
            "idle".to_owned()
        };
        let title_width = column.width.saturating_sub(10) as usize;
        let card = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(if focused { PANEL_ACTIVE } else { PANEL_ALT }))
            .padding(Padding::horizontal(1))
            .title(Line::from(vec![
                Span::styled(
                    if focused { " ▸ " } else { "   " },
                    Style::default().fg(BLUE),
                ),
                Span::styled("● ", Style::default().fg(status_color)),
                Span::styled(
                    truncate_end(id, title_width),
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
            ]))
            .title_bottom(Line::from(Span::styled(
                format!(" {summary} "),
                Style::default().fg(status_color),
            )))
            .title_bottom(
                Line::from(Span::styled(
                    if *is_default { " DEFAULT " } else { " " },
                    Style::default().fg(if *is_default { PURPLE } else { DIM }),
                ))
                .right_aligned(),
            );
        let card_inner = card.inner(*column);
        frame.render_widget(card, *column);

        let capacity = card_inner.height as usize;
        let tasks = workspace_activity_tasks(snapshot, id, capacity);
        if tasks.is_empty() && capacity > 0 {
            let lines = if card_inner.height >= 2 {
                vec![
                    Line::from(Span::styled("quiet", Style::default().fg(GRAY))),
                    Line::from(Span::styled(
                        truncate_middle(path, card_inner.width as usize),
                        Style::default().fg(DIM),
                    )),
                ]
            } else {
                vec![Line::from(Span::styled("quiet", Style::default().fg(GRAY)))]
            };
            frame.render_widget(
                Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center),
                card_inner,
            );
            continue;
        }

        let items = tasks
            .into_iter()
            .map(|task| activity_item(task, tick, now, card_inner.width as usize))
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), card_inner);
    }
}

pub(super) fn workspace_activity_tasks<'a>(
    snapshot: &'a MonitorSnapshot,
    workspace: &str,
    capacity: usize,
) -> Vec<&'a TaskRecord> {
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.workspace == workspace)
        .collect::<Vec<_>>();
    tasks.sort_by(|a, b| {
        activity_rank(a.status)
            .cmp(&activity_rank(b.status))
            .then_with(|| task_time(b).cmp(&task_time(a)))
    });
    tasks.truncate(capacity);
    tasks
}

pub(super) fn activity_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::Running => 0,
        TaskStatus::Queued => 1,
        TaskStatus::Completed | TaskStatus::Failed => 2,
    }
}

pub(super) fn task_time(task: &TaskRecord) -> Instant {
    task.finished_at
        .or(task.started_at)
        .unwrap_or(task.queued_at)
}

pub(super) fn activity_item(
    task: &TaskRecord,
    tick: usize,
    now: Instant,
    width: usize,
) -> ListItem<'static> {
    let (icon, color, highlighted) = match task.status {
        TaskStatus::Queued => ("◌".to_owned(), YELLOW, true),
        TaskStatus::Running => (
            spinner_frame(tick.wrapping_add(task.id as usize)).to_owned(),
            CYAN,
            true,
        ),
        TaskStatus::Completed => ("✓".to_owned(), GREEN, false),
        TaskStatus::Failed => ("×".to_owned(), RED, true),
    };
    let end = task.finished_at.unwrap_or(now);
    let started = task.started_at.unwrap_or(task.queued_at);
    let elapsed = short_duration(end.saturating_duration_since(started));
    let elapsed_width = 7usize;
    let tool_width = if width >= 52 {
        18
    } else {
        width.saturating_sub(elapsed_width + 4).clamp(8, 22)
    };
    let mut spans = vec![
        Span::styled(format!("{icon} "), Style::default().fg(color)),
        Span::styled(
            format!("{:<tool_width$}", truncate_end(&task.tool, tool_width)),
            Style::default()
                .fg(if highlighted { WHITE } else { GRAY })
                .add_modifier(if highlighted {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
    ];
    if width >= 52 {
        let detail_width = width.saturating_sub(tool_width + elapsed_width + 7);
        let detail = if width >= 72 {
            format!(
                "{} · {}/{}",
                task.detail,
                short_bytes(task.request_bytes),
                short_bytes(task.response_bytes)
            )
        } else {
            task.detail.clone()
        };
        spans.push(Span::styled(
            format!(" · {}", truncate_end(&detail, detail_width)),
            Style::default().fg(DIM),
        ));
    }
    spans.push(Span::styled(
        format!(" {elapsed:>elapsed_width$}"),
        Style::default().fg(color),
    ));

    ListItem::new(Line::from(spans)).style(Style::default().bg(if highlighted {
        PANEL_ACTIVE
    } else {
        PANEL_ALT
    }))
}

pub(super) fn workspace_column_count(width: u16, total: usize) -> usize {
    let inner = width.saturating_sub(4);
    let columns = (inner / 31).max(1) as usize;
    columns.min(total.max(1))
}

pub(super) fn last_seen_text(last_seen: Option<Instant>) -> String {
    let Some(last_seen) = last_seen else {
        return "—".to_owned();
    };
    let elapsed = last_seen.elapsed();
    if elapsed < Duration::from_secs(2) {
        "just now".to_owned()
    } else if elapsed < Duration::from_secs(60) {
        format!("{}s ago", elapsed.as_secs())
    } else if elapsed < Duration::from_secs(3600) {
        format!("{}m ago", elapsed.as_secs() / 60)
    } else {
        format!("{}h ago", elapsed.as_secs() / 3600)
    }
}

pub(super) fn is_quick_tunnel(mcp_url: &str) -> bool {
    mcp_url.contains(".trycloudflare.com/")
}

pub(super) fn open_external_url(url: &str) -> io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = StdCommand::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = StdCommand::new("explorer.exe");
        command.arg(url);
        command
    } else {
        let mut command = StdCommand::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .spawn()
        .map(|_| ())
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
        .border_style(Style::default().fg(PURPLE))
        .style(Style::default().bg(PANEL))
        .padding(Padding::uniform(1))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", language.tr("SOFTWARE INTELLIGENCE")),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {workspace_id} "), Style::default().fg(WHITE)),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" I / Esc  {} ", language.tr("close")),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:<12}", language.tr("ROOT")),
                Style::default().fg(DIM),
            ),
            Span::styled(
                truncate_middle(root, inner.width.saturating_sub(12) as usize),
                Style::default().fg(GRAY),
            ),
        ]),
        Line::from(""),
        intelligence_line(
            language.tr("DESIGN"),
            format!(
                "{design} · {} requirements · {} components",
                stats.requirements, stats.components
            ),
            if design == "valid" { GREEN } else { YELLOW },
        ),
        intelligence_line(
            language.tr("SCOPES"),
            format!(
                "{} mapped / {} source · {} unmapped",
                stats.scope_mapped_files, stats.scope_source_files, stats.scope_unmapped_files
            ),
            if stats.scope_unmapped_files == 0 && stats.scope_source_files > 0 {
                GREEN
            } else if stats.scope_source_files == 0 {
                GRAY
            } else {
                YELLOW
            },
        ),
        intelligence_line(
            language.tr("TRACE"),
            format!("implementation {implementation} · verification {verification}"),
            if stats.implementation_coverage == Some(100)
                && stats.verification_coverage == Some(100)
            {
                GREEN
            } else {
                YELLOW
            },
        ),
        intelligence_line(
            language.tr("GRAPH"),
            format!(
                "{} nodes · {} edges · {precision}",
                stats.graph_nodes, stats.graph_edges
            ),
            BLUE,
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
            PURPLE,
        ),
        intelligence_line(
            language.tr("SEMANTICS"),
            format!(
                "{} confirmed · {} candidates",
                stats.semantic_confirmed, stats.semantic_candidates
            ),
            CYAN,
        ),
        intelligence_line(
            language.tr("RISK"),
            format!("{risk} · {} drift finding(s)", stats.drift_findings),
            match risk {
                "low" => GREEN,
                "medium" | "moderate" => YELLOW,
                "high" | "critical" => RED,
                _ => GRAY,
            },
        ),
        intelligence_line(
            language.tr("EVIDENCE"),
            format!(
                "{} total · {} failed · {} disagreed",
                stats.evidence_total, stats.evidence_failed, stats.evidence_disagreed
            ),
            if stats.evidence_failed > 0 || stats.evidence_disagreed > 0 {
                RED
            } else {
                GREEN
            },
        ),
        intelligence_line(
            language.tr("VERIFY"),
            format!("{ready} · {} blocker(s)", stats.verification_blockers),
            if stats.verification_ready == Some(true) {
                GREEN
            } else {
                YELLOW
            },
        ),
        intelligence_line(
            language.tr("RECONCILE"),
            format!(
                "{reconciliation} · {} pending task(s)",
                stats.reconciliation_pending
            ),
            if stats.reconciliation_converged == Some(true) {
                GREEN
            } else {
                PURPLE
            },
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{:<12}", language.tr("UPDATED")),
                Style::default().fg(DIM),
            ),
            Span::styled(updated, Style::default().fg(GRAY)),
        ]),
        Line::from(Span::styled(
            language.tr("Run intelligence tools to refresh live fields."),
            Style::default().fg(DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn intelligence_line(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(DIM)),
        Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

pub(super) fn render_workspace_input_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    input: &str,
    language: UiLanguage,
) {
    let width = area.width.saturating_sub(8).clamp(36, 88);
    let height = area.height.saturating_sub(2).clamp(3, 7);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BLUE))
        .style(Style::default().bg(PANEL_ACTIVE))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("AUTHORIZE WORKSPACE")),
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                if input.is_empty() {
                    language.tr("Type an absolute or relative project path…")
                } else {
                    input
                },
                Style::default().fg(if input.is_empty() { GRAY } else { WHITE }),
            )),
            Line::from(Span::styled(
                language.tr("Enter authorize · Esc cancel · hard safety boundaries still apply"),
                Style::default().fg(DIM),
            )),
        ]),
        inner,
    );
}

pub(super) fn render_authorization_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    requests: &[AuthorizationRequest],
    focus: usize,
    language: UiLanguage,
) {
    if requests.is_empty() || area.width < 40 || area.height < 10 {
        return;
    }
    let focus = focus.min(requests.len() - 1);
    let visible = requests.len().min(5);
    let start = focus
        .saturating_sub(visible / 2)
        .min(requests.len().saturating_sub(visible));
    let end = (start + visible).min(requests.len());
    let width = area.width.saturating_sub(8).clamp(38, 104);
    let height = (visible as u16 + 4).clamp(6, 10);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height + 3),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(YELLOW))
        .style(Style::default().bg(PANEL_ACTIVE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " ! ",
                Style::default()
                    .fg(BG)
                    .bg(YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("AUTHORIZATION REQUIRED")),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" {} {} ", requests.len(), language.tr("PENDING")),
                Style::default().fg(YELLOW),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::with_capacity(visible + 1);
    for (index, request) in requests[start..end].iter().enumerate() {
        let absolute = start + index;
        let selected = absolute == focus;
        let kind = match request.kind {
            crate::authorization::AuthorizationKind::CommandAccess => language.tr("COMMAND"),
            crate::authorization::AuthorizationKind::RiskyExecution => language.tr("RISKY EXEC"),
            crate::authorization::AuthorizationKind::RuntimeExecutor => language.tr("RUNTIME EXEC"),
            crate::authorization::AuthorizationKind::DestructiveDelete => language.tr("DELETE"),
        };
        let prefix = if selected { "›" } else { " " };
        let summary_width = inner.width.saturating_sub(39) as usize;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{prefix} {} ", request.id),
                Style::default()
                    .fg(if selected { WHITE } else { GRAY })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(
                format!("[{kind}] "),
                Style::default().fg(if selected { YELLOW } else { DIM }),
            ),
            Span::styled(
                format!("{} · ", request.workspace),
                Style::default().fg(CYAN),
            ),
            Span::styled(
                truncate_end(&request.summary, summary_width.max(8)),
                Style::default().fg(if selected { WHITE } else { GRAY }),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        keycap("↑/↓"),
        Span::styled(
            format!(" {}   ", language.tr("select request")),
            Style::default().fg(GRAY),
        ),
        keycap("Y"),
        Span::styled(
            format!(" {}   ", language.tr("approve selected")),
            Style::default().fg(GREEN),
        ),
        keycap("N"),
        Span::styled(
            format!(" {}", language.tr("deny selected")),
            Style::default().fg(RED),
        ),
    ]));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn render_status_message(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &str,
    language: UiLanguage,
) {
    if area.width < 32 || area.height < 5 {
        return;
    }
    let width = area.width.saturating_sub(8).clamp(28, 88);
    let height = 3;
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height + 2),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(truncate_end(message, width.saturating_sub(4) as usize)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BLUE))
                .style(Style::default().bg(PANEL_ACTIVE))
                .title(Span::styled(
                    format!(" {} ", language.tr("STATUS")),
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                )),
        ),
        popup,
    );
}

pub(super) fn render_help_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    let width = area.width.saturating_sub(6).clamp(36, 98);
    let height = area.height.saturating_sub(4).clamp(12, 18);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PURPLE))
        .style(Style::default().bg(PANEL_ACTIVE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " ? ",
                Style::default()
                    .fg(BG)
                    .bg(PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("HELP & LINKS")),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" {} ", language.tr("ESC TO CLOSE")),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if width < 74 || height < 16 {
        frame.render_widget(
            Paragraph::new(vec![
                help_hint_line("←/→", language.tr("move workspace")),
                help_hint_line("⇧←/→", language.tr("move one page")),
                help_hint_line("O", language.tr("open Connector setup")),
                help_hint_line("W", language.tr("open Project Observatory")),
                help_hint_line("L", language.tr("toggle language")),
                help_hint_line(
                    "↑/↓ Y/N",
                    language.tr("select / approve / deny authorization"),
                ),
                help_hint_line("? / Esc", language.tr("open or close help")),
                help_hint_line("^C", language.tr("stop wcode")),
                help_link_line(language.tr("Project"), &config.project_url, inner.width),
                help_link_line(language.tr("Author"), &config.author_url, inner.width),
                help_link_line(language.tr("Setup"), &config.setup_url, inner.width),
                help_link_line(language.tr("Health"), &config.local_health_url, inner.width),
            ]),
            inner,
        );
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("SHORTCUTS"),
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            help_hint_line("← / →", language.tr("move workspace focus")),
            help_hint_line("Shift + ← / →", language.tr("move one workspace page")),
            help_hint_line("O", language.tr("open Connector setup")),
            help_hint_line("W", language.tr("open Project Observatory")),
            help_hint_line("G", language.tr("open project repository")),
            help_hint_line("A", language.tr("open author profile")),
            help_hint_line("L", language.tr("toggle language")),
            help_hint_line("↑ / ↓", language.tr("select authorization")),
            help_hint_line("Y", language.tr("approve selected authorization")),
            help_hint_line("N", language.tr("deny selected authorization")),
            help_hint_line("? / Esc", language.tr("open or close help")),
            help_hint_line("Ctrl-C", language.tr("stop wcode")),
        ])
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(BORDER))
                .padding(Padding::new(0, 2, 0, 0)),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("RUNTIME"),
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("Slots  ", Style::default().fg(WHITE)),
                Span::styled("active child tasks / cap", Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("Peak   ", Style::default().fg(WHITE)),
                Span::styled(
                    "real concurrency high-water mark",
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(vec![
                Span::styled("Fan-out", Style::default().fg(WHITE)),
                Span::styled(
                    "  parallel_tools · review · verify",
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(vec![
                Span::styled("CTX    ", Style::default().fg(WHITE)),
                Span::styled("estimated tool-output tokens", Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("Saved  ", Style::default().fg(WHITE)),
                Span::styled(
                    format!(
                        "AST context avoided · EST at ${:.2}/M",
                        config.input_token_price_per_million_usd
                    ),
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(""),
            help_link_line(
                language.tr("Project"),
                &config.project_url,
                columns[1].width,
            ),
            help_link_line(language.tr("Author"), &config.author_url, columns[1].width),
            help_link_line(language.tr("Setup"), &config.setup_url, columns[1].width),
            help_link_line(
                language.tr("Health"),
                &config.local_health_url,
                columns[1].width,
            ),
        ]),
        columns[1],
    );
}

fn help_hint_line(key: &str, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<15}"),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label.to_owned(), Style::default().fg(GRAY)),
    ])
}

pub(super) fn help_link_line(label: &str, url: &str, width: u16) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(DIM)),
        Span::styled(
            truncate_middle(url, width.saturating_sub(10) as usize),
            Style::default().fg(BLUE).add_modifier(Modifier::UNDERLINED),
        ),
    ])
}

pub(super) fn slot_bar(active: u64, capacity: u64, width: usize) -> (String, String, Color) {
    let capacity = capacity.max(1);
    let ratio = (active as f64 / capacity as f64).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let color = if ratio >= 0.85 {
        RED
    } else if ratio >= 0.6 {
        YELLOW
    } else {
        CYAN
    };
    (
        "━".repeat(filled),
        "·".repeat(width.saturating_sub(filled)),
        color,
    )
}

pub(super) fn render_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let pending_authorizations = config
        .workspaces
        .authorization_requests(256)
        .iter()
        .filter(|request| request.status == AuthorizationStatus::Pending)
        .count();
    let project = config
        .project_url
        .strip_prefix("https://")
        .unwrap_or(&config.project_url)
        .trim_end_matches('/');

    if area.width >= 124 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "  wcode  ",
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    project.to_owned(),
                    Style::default().fg(BLUE).add_modifier(Modifier::UNDERLINED),
                ),
                Span::styled("  by  ", Style::default().fg(DIM)),
                Span::styled(
                    config.author_handle.clone(),
                    Style::default()
                        .fg(PURPLE)
                        .add_modifier(Modifier::UNDERLINED),
                ),
            ])),
            columns[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                keycap("←/→"),
                Span::styled(
                    format!(" {}  ", language.tr("workspace")),
                    Style::default().fg(GRAY),
                ),
                keycap("O"),
                Span::styled(
                    format!(" {}  ", language.tr("setup")),
                    Style::default().fg(GRAY),
                ),
                keycap("W"),
                Span::styled(
                    format!(" {}  ", language.tr("web")),
                    Style::default().fg(GRAY),
                ),
                keycap("L"),
                Span::styled(format!(" {}  ", language.name()), Style::default().fg(GRAY)),
                keycap("+"),
                Span::raw(" "),
                keycap("Y/N"),
                Span::styled(
                    if pending_authorizations > 0 {
                        format!(" {pending_authorizations} ")
                    } else {
                        " ".to_owned()
                    },
                    Style::default().fg(if pending_authorizations > 0 {
                        YELLOW
                    } else {
                        GRAY
                    }),
                ),
                keycap("?"),
                Span::raw(" "),
                keycap("^C"),
            ]))
            .alignment(ratatui::layout::Alignment::Right),
            columns[1],
        );
        return;
    }

    let line = if area.width >= 78 {
        Line::from(vec![
            Span::raw(" "),
            keycap("←/→"),
            Span::styled(
                format!(" {}  ", language.tr("workspace")),
                Style::default().fg(GRAY),
            ),
            keycap("O"),
            Span::styled(
                format!(" {}  ", language.tr("setup")),
                Style::default().fg(GRAY),
            ),
            keycap("W"),
            Span::styled(
                format!(" {}  ", language.tr("web")),
                Style::default().fg(GRAY),
            ),
            keycap("L"),
            Span::styled(format!(" {}  ", language.name()), Style::default().fg(GRAY)),
            keycap("Y/N"),
            Span::styled(
                if pending_authorizations > 0 {
                    format!(" {pending_authorizations}  ")
                } else {
                    "  ".to_owned()
                },
                Style::default().fg(if pending_authorizations > 0 {
                    YELLOW
                } else {
                    GRAY
                }),
            ),
            keycap("?"),
            Span::raw("  "),
            keycap("^C"),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            keycap("←/→"),
            Span::raw("  "),
            keycap("O"),
            Span::raw("  "),
            keycap("W"),
            Span::raw("  "),
            keycap("L"),
            Span::raw("  "),
            keycap("Y/N"),
            Span::raw(if pending_authorizations > 0 {
                " ! "
            } else {
                "  "
            }),
            keycap("?"),
            Span::raw("  "),
            keycap("^C"),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

pub(super) fn keycap(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(WHITE)
            .bg(PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn totals(snapshot: &MonitorSnapshot) -> WorkspaceStats {
    snapshot
        .workspaces
        .values()
        .fold(WorkspaceStats::default(), |mut total, stats| {
            total.queued = total.queued.saturating_add(stats.queued);
            total.active = total.active.saturating_add(stats.active);
            total.completed = total.completed.saturating_add(stats.completed);
            total.failed = total.failed.saturating_add(stats.failed);
            total.calls = total.calls.saturating_add(stats.calls);
            total.request_bytes = total.request_bytes.saturating_add(stats.request_bytes);
            total.response_bytes = total.response_bytes.saturating_add(stats.response_bytes);
            total.context_bytes_avoided = total
                .context_bytes_avoided
                .saturating_add(stats.context_bytes_avoided);
            total
        })
}

pub(super) fn success_rate(completed: u64, failed: u64) -> f64 {
    let finished = completed.saturating_add(failed);
    if finished == 0 {
        100.0
    } else {
        completed as f64 * 100.0 / finished as f64
    }
}

pub(super) fn window_totals(snapshot: &MonitorSnapshot, window: Duration) -> (u64, u64, u64) {
    let now = Instant::now();
    snapshot
        .traffic
        .iter()
        .filter(|event| now.saturating_duration_since(event.at) <= window)
        .fold((0, 0, 0), |(requests, rx, tx), event| {
            (
                requests.saturating_add(event.requests),
                rx.saturating_add(event.request_bytes),
                tx.saturating_add(event.response_bytes),
            )
        })
}

pub(super) fn window_context_avoided(snapshot: &MonitorSnapshot, window: Duration) -> u64 {
    let now = Instant::now();
    snapshot
        .traffic
        .iter()
        .filter(|event| now.saturating_duration_since(event.at) <= window)
        .fold(0u64, |total, event| {
            total.saturating_add(event.context_bytes_avoided)
        })
}

pub(super) fn request_bins(snapshot: &MonitorSnapshot, count: usize, width: Duration) -> Vec<u64> {
    let now = Instant::now();
    let mut bins = vec![0u64; count];
    for event in &snapshot.traffic {
        let age = now.saturating_duration_since(event.at);
        let index_from_end = (age.as_secs_f64() / width.as_secs_f64()) as usize;
        if index_from_end < count {
            let index = count - 1 - index_from_end;
            bins[index] = bins[index].saturating_add(event.requests);
        }
    }
    bins
}
