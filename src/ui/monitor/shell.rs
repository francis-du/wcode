use super::*;

pub(super) fn draw_dashboard(
    frame: &mut Frame<'_>,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    ui: &DashboardState,
) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND)),
        area,
    );

    let compact = area.width < 92;
    let dense = area.height < 28;
    let header_height = 8;
    // The base heights fit one tunnel link row; every additional live tunnel
    // needs its own row or the last provider gets clipped.
    let extra_tunnel_rows =
        u16::try_from(snapshot.tunnels.len().saturating_sub(1)).unwrap_or(u16::MAX - 32);
    let setup_height = if snapshot.chatgpt_connected {
        0
    } else if compact || dense {
        7 + extra_tunnel_rows
    } else {
        8 + extra_tunnel_rows
    };
    let overview_height = 4;
    let minimum_activity_height = 4;
    let fixed_height = header_height + setup_height + overview_height + minimum_activity_height + 2;

    if area.width < 40 || area.height < fixed_height {
        render_too_small(frame, area, config, ui.language);
        return;
    }

    let throughput_height = if area.height >= fixed_height.saturating_add(4) {
        4
    } else {
        0
    };
    let mut constraints = vec![Constraint::Length(header_height)];
    if setup_height > 0 {
        constraints.push(Constraint::Length(setup_height));
    }
    constraints.push(Constraint::Length(overview_height));
    constraints.push(Constraint::Min(minimum_activity_height));
    if throughput_height > 0 {
        constraints.push(Constraint::Length(throughput_height));
    }
    constraints.push(Constraint::Length(2));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut row = 0usize;
    render_header(
        frame,
        rows[row],
        snapshot,
        config,
        tick,
        compact,
        ui.language,
    );
    row += 1;
    if setup_height > 0 {
        render_setup(frame, rows[row], snapshot, config, compact, ui.language);
        row += 1;
    }
    render_overview(frame, rows[row], snapshot, config, compact, ui.language);
    row += 1;
    render_workspace_activity(frame, rows[row], snapshot, config, tick, ui);
    row += 1;
    if throughput_height > 0 {
        render_throughput(frame, rows[row], snapshot, config, ui.language);
        row += 1;
    }
    render_footer(frame, rows[row], config, ui.language);

    if ui.commands_open {
        if let Some(workspace_id) = focused_workspace_id(config, ui.workspace_focus) {
            render_commands_overlay(
                frame,
                area,
                &config.workspaces,
                &workspace_id,
                ui.command_offset,
                ui.language,
            );
        }
    } else if ui.intelligence_open {
        render_intelligence_overlay(
            frame,
            area,
            snapshot,
            config,
            ui.workspace_focus,
            ui.language,
        );
    } else if ui.help_open {
        render_help_overlay(frame, area, config, ui.language);
    }
    if !ui.help_open && !ui.intelligence_open && !ui.commands_open && ui.workspace_input.is_none() {
        let pending = pending_authorizations(config);
        if !pending.is_empty() {
            render_authorization_overlay(
                frame,
                area,
                &pending,
                ui.authorization_focus,
                ui.language,
            );
        }
    }
    if let Some(input) = ui.workspace_input.as_deref() {
        render_workspace_input_overlay(frame, area, input, ui.language);
    }
    if let Some(message) = ui.workspace_message.as_deref() {
        render_status_message(frame, area, message, ui.language);
    }
}

fn render_too_small(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::uniform(1))
        .title(Line::from(vec![
            Span::styled(
                " WC ",
                Style::default()
                    .fg(BACKGROUND)
                    .bg(SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " wcode ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" INSTANCE {} ", truncate_end(&config.instance_id, 8)),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("Terminal needs a little more room"),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("current size  {} × {}", area.width, area.height),
                Style::default().fg(TEXT_MUTED),
            )),
            Line::from(Span::styled(
                language.tr("resize the window to restore the live dashboard"),
                Style::default().fg(TEXT_DIM),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(block),
        area,
    );
}

fn render_header(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    compact: bool,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let idle = snapshot
        .last_mcp_seen
        .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300));
    let (icon, state, detail, color) = if snapshot.tunnel_running == Some(false) {
        (
            "×",
            language.tr("TUNNEL PROCESS EXITED"),
            snapshot
                .tunnel_error
                .as_deref()
                .map(|error| truncate_end(error, 54))
                .unwrap_or_else(|| {
                    language
                        .tr("tunnel process is no longer running")
                        .to_owned()
                }),
            DANGER,
        )
    } else if snapshot.public_url_healthy == Some(false) {
        (
            "×",
            language.tr("PUBLIC URL UNAVAILABLE"),
            format!(
                "{} consecutive health checks failed",
                snapshot.public_url_consecutive_failures
            ),
            DANGER,
        )
    } else if snapshot.chatgpt_connected && idle {
        (
            "◐",
            language.tr("MCP client idle"),
            format!(
                "last seen {} · HTTP/SSE",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            WARNING,
        )
    } else if snapshot.chatgpt_connected {
        (
            "●",
            language.tr("MCP client connected"),
            format!(
                "last seen {} · HTTP/SSE",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            SUCCESS,
        )
    } else if snapshot.oauth_authorized {
        (
            "◐",
            language.tr("OAuth authorized"),
            language.tr("waiting for MCP handshake").to_owned(),
            WARNING,
        )
    } else {
        (
            "○",
            language.tr("Setup required"),
            language.tr("press O to open Connector setup").to_owned(),
            TEXT_MUTED,
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if snapshot.chatgpt_connected {
            color
        } else {
            OUTLINE
        }))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " WC ",
                Style::default()
                    .fg(BACKGROUND)
                    .bg(SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" wcode {} ", config.version),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" INSTANCE {} ", truncate_end(&config.instance_id, 8)),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let local_home = config.local_health_url.trim_end_matches("/healthz");
    if compact || inner.width < 76 {
        let lines = vec![
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    state,
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {detail}"), Style::default().fg(TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("MCP     ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate_middle(&config.mcp_url(), inner.width.saturating_sub(8) as usize),
                    Style::default().fg(LINK),
                ),
            ]),
            Line::from(vec![
                Span::styled("PUBLIC  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    public_url_health_text(snapshot),
                    Style::default().fg(public_url_health_color(snapshot)),
                ),
                Span::styled("   AUTH  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    auth_session_text(snapshot),
                    Style::default().fg(auth_session_color(snapshot)),
                ),
            ]),
            Line::from(vec![
                Span::styled("TUNNEL  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    tunnel_status_text(snapshot),
                    Style::default().fg(tunnel_status_color(snapshot)),
                ),
                Span::styled("   INIT  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    initialize_status_text(snapshot),
                    Style::default().fg(SECONDARY),
                ),
            ]),
            Line::from(vec![
                Span::styled("SLOTS ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    format!("{} / {}", totals.active, config.max_parallel),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   PEAK ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    snapshot.peak_active.to_string(),
                    Style::default().fg(SECONDARY),
                ),
            ]),
            Line::from(vec![
                Span::styled("VERIFY CODE ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    config.pairing_code.clone(),
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    state,
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {detail}"), Style::default().fg(TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("MCP     ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate_middle(
                        &config.mcp_url(),
                        columns[0].width.saturating_sub(9) as usize,
                    ),
                    Style::default().fg(LINK),
                ),
            ]),
            Line::from(vec![
                Span::styled("WEB     ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate_middle(local_home, columns[0].width.saturating_sub(9) as usize),
                    Style::default().fg(LINK),
                ),
            ]),
            Line::from(vec![
                Span::styled("PUBLIC  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    public_url_health_text(snapshot),
                    Style::default().fg(public_url_health_color(snapshot)),
                ),
            ]),
            Line::from(vec![
                Span::styled("AUTH    ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    auth_session_text(snapshot),
                    Style::default().fg(auth_session_color(snapshot)),
                ),
            ]),
        ]),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(spinner_frame(tick), Style::default().fg(ACCENT)),
                Span::styled(
                    " LIVE",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  UPTIME {}", short_duration(snapshot.started_at.elapsed())),
                    Style::default().fg(TEXT_MUTED),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("VERIFY CODE ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    config.pairing_code.clone(),
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("SLOTS ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    format!("{} / {}", totals.active, config.max_parallel),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   PEAK ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    snapshot.peak_active.to_string(),
                    Style::default().fg(SECONDARY),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("TUNNEL  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    tunnel_status_text(snapshot),
                    Style::default().fg(tunnel_status_color(snapshot)),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("INIT  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    initialize_status_text(snapshot),
                    Style::default().fg(SECONDARY),
                ),
            ])
            .right_aligned(),
            Line::from(Span::styled(
                if snapshot.public_url_healthy == Some(false)
                    || snapshot.tunnel_running == Some(false)
                {
                    "Restart wcode, then update the Connector URL"
                } else {
                    "PUBLIC URL MONITORING ACTIVE"
                },
                Style::default().fg(
                    if snapshot.public_url_healthy == Some(false)
                        || snapshot.tunnel_running == Some(false)
                    {
                        DANGER
                    } else {
                        TEXT_DIM
                    },
                ),
            ))
            .right_aligned(),
        ]),
        columns[1],
    );
}

fn auth_session_text(snapshot: &MonitorSnapshot) -> &'static str {
    if snapshot.oauth_authorized {
        "● SESSION LASTS FOR THIS RUN"
    } else {
        "○ WAITING FOR OAUTH"
    }
}

fn auth_session_color(snapshot: &MonitorSnapshot) -> Color {
    if snapshot.oauth_authorized {
        SUCCESS
    } else {
        TEXT_MUTED
    }
}

pub(super) fn public_url_health_text(snapshot: &MonitorSnapshot) -> String {
    let checked = snapshot
        .public_url_last_checked
        .map(|seen| last_seen_text(Some(seen)))
        .unwrap_or_else(|| "not checked yet".to_owned());
    match snapshot.public_url_healthy {
        Some(true) => format!("● HEALTHY · checked {checked}"),
        Some(false) => format!(
            "× UNAVAILABLE · {} failures · {}",
            snapshot.public_url_consecutive_failures,
            snapshot
                .public_url_error
                .as_deref()
                .map(|error| truncate_end(error, 28))
                .unwrap_or(checked)
        ),
        None if snapshot.public_url_consecutive_failures > 0 => format!(
            "◐ CHECKING · {} failure(s) · checked {checked}",
            snapshot.public_url_consecutive_failures
        ),
        None => format!("○ PENDING · {checked}"),
    }
}

fn public_url_health_color(snapshot: &MonitorSnapshot) -> Color {
    match snapshot.public_url_healthy {
        Some(true) => SUCCESS,
        Some(false) => DANGER,
        None if snapshot.public_url_consecutive_failures > 0 => WARNING,
        None => TEXT_MUTED,
    }
}
