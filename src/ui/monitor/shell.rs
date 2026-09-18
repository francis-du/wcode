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

    render_dashboard_body(frame, area, snapshot, config, tick, ui);

    if ui.commands_open {
        if let Some(workspace_id) = focused_workspace_id(config, ui.workspace_focus) {
            render_commands_overlay(
                frame,
                area,
                &config.workspaces,
                &workspace_id,
                ui.command_offset,
                ui.workspace_message.as_deref(),
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
    let authorization_visible = ui.authorization_visible(area);
    // Authorization feedback lives inside the authorization surface so it cannot
    // be painted underneath the very controls that triggered it.
    if let Some(message) = ui.workspace_message.as_deref() {
        if !authorization_visible && !ui.commands_open && !ui.help_open && !ui.intelligence_open {
            render_status_message(frame, area, message, ui.language);
        }
    }
    if ui.full_access_visible(area) {
        render_full_access_overlay(frame, area, ui.language);
    } else if authorization_visible {
        render_authorization_overlay(
            frame,
            area,
            &ui.pending_authorizations,
            ui.authorization_focus,
            ui.authorization_scroll,
            ui.workspace_message.as_deref(),
            ui.language,
        );
    }
    if ui.workspace_input_visible(area) {
        if let Some(input) = ui.workspace_input.as_deref() {
            render_workspace_input_overlay(frame, area, input, ui.language);
        }
    }
}

fn render_dashboard_body(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    ui: &DashboardState,
) {
    let compact = area.width < 92;
    let dense = area.height < 28;
    let header_height = if dense { 6 } else { 8 };
    // The base heights fit one tunnel link row; every additional live tunnel
    // needs its own row or the last provider gets clipped.
    let extra_tunnel_rows =
        u16::try_from(snapshot.tunnels.len().saturating_sub(1)).unwrap_or(u16::MAX - 32);
    let setup_height = if snapshot.chatgpt_connected {
        0
    } else if dense {
        4 + extra_tunnel_rows
    } else if compact {
        7 + extra_tunnel_rows
    } else {
        8 + extra_tunnel_rows
    };
    // Outer and inner card borders consume four rows; reserve actual task rows too.
    let minimum_activity_height = 6;
    let base_fixed_height = header_height + setup_height + minimum_activity_height + 2;
    let engineering_pulse_height =
        if !compact && !dense && area.height >= base_fixed_height.saturating_add(6) {
            5
        } else {
            0
        };
    let fixed_height = base_fixed_height.saturating_add(engineering_pulse_height);

    if area.width < 40 || area.height < fixed_height {
        render_too_small(frame, area, config, ui.language);
        return;
    }

    let recent_requests = window_totals(snapshot, Duration::from_secs(30)).0;
    let throughput_height =
        if !dense && recent_requests > 0 && area.height >= fixed_height.saturating_add(8) {
            4
        } else {
            0
        };
    let mut constraints = vec![Constraint::Length(header_height)];
    if setup_height > 0 {
        constraints.push(Constraint::Length(setup_height));
    }
    if engineering_pulse_height > 0 {
        constraints.push(Constraint::Length(engineering_pulse_height));
    }
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
    if engineering_pulse_height > 0 {
        render_engineering_pulse(
            frame,
            rows[row],
            snapshot,
            config,
            ui.workspace_focus,
            ui.language,
        );
        row += 1;
    }
    render_workspace_activity(frame, rows[row], snapshot, config, tick, ui);
    row += 1;
    if throughput_height > 0 {
        render_throughput(frame, rows[row], snapshot, config, ui.language);
        row += 1;
    }
    render_footer(frame, rows[row], config, ui.language);
}

fn render_engineering_pulse(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    focus: usize,
    language: UiLanguage,
) {
    let workspace_id =
        focused_workspace_id(config, focus).unwrap_or_else(|| "workspace".to_owned());
    let stats = snapshot.intelligence.get(&workspace_id);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", language.tr("ENGINEERING PULSE")),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {workspace_id} "), Style::default().fg(TEXT)),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" {} ", language.tr("engineering console")),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(stats) = stats.filter(|stats| stats.updated_at.is_some()) else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    language.tr("Loading project engineering model…"),
                    Style::default().fg(TEXT_MUTED),
                )),
                Line::from(Span::styled(
                    language.tr("Runtime activity stays live while architecture evidence loads."),
                    Style::default().fg(TEXT_DIM),
                )),
            ]),
            inner,
        );
        return;
    };

    let design_ok = stats.design_state.as_deref() == Some("valid");
    let architecture = format!(
        "{} · impl {} · refs {}",
        stats.design_state.as_deref().unwrap_or("unknown"),
        stats
            .implementation_coverage
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "—".to_owned()),
        stats
            .verification_coverage
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "—".to_owned())
    );
    let drift = format!(
        "{} · {} drift",
        stats.risk_level.as_deref().unwrap_or("unassessed"),
        stats.drift_findings
    );
    let proof = format!(
        "{} · {} failed · {} disagreed",
        stats
            .verification_ready
            .map(|ready| if ready { "ready" } else { "blocked" })
            .unwrap_or("unplanned"),
        stats.evidence_failed,
        stats.evidence_disagreed
    );
    let model = format!(
        "{} · {} nodes / {} edges",
        stats.graph_precision.as_deref().unwrap_or("syntax"),
        stats.graph_nodes,
        stats.graph_edges
    );
    let line_width = inner.width.saturating_sub(1) as usize;
    let first = format!(
        "{}  {}    {}  {}",
        language.tr("ARCH"),
        architecture,
        language.tr("DRIFT"),
        drift
    );
    let second = format!(
        "{}  {}    {}  {}",
        language.tr("PROOF"),
        proof,
        language.tr("MODEL"),
        model
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                truncate_end(&first, line_width),
                Style::default().fg(if design_ok { TEXT_MUTED } else { WARNING }),
            )),
            Line::from(Span::styled(
                truncate_end(&second, line_width),
                Style::default().fg(if stats.evidence_failed > 0 {
                    DANGER
                } else {
                    TEXT_MUTED
                }),
            )),
            Line::from(vec![
                Span::styled(
                    format!("{}  ", language.tr("POLICY")),
                    Style::default().fg(TEXT_DIM),
                ),
                Span::styled(
                    format!(
                        "{} errors · {} warnings",
                        stats.policy_errors, stats.policy_warnings
                    ),
                    Style::default().fg(if stats.policy_errors > 0 {
                        DANGER
                    } else if stats.policy_warnings > 0 {
                        WARNING
                    } else {
                        SUCCESS
                    }),
                ),
                Span::styled(
                    format!(
                        "    {} · {}",
                        language.tr("updated"),
                        last_seen_text(stats.updated_at)
                    ),
                    Style::default().fg(TEXT_DIM),
                ),
            ]),
        ]),
        inner,
    );
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
                " wcode ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" engineering control plane ", Style::default().fg(ACCENT)),
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
            Line::from(vec![
                Span::styled(
                    format!("{}  ", language.tr("Pairing code")),
                    Style::default().fg(TEXT_DIM),
                ),
                Span::styled(
                    config.pairing_code.clone(),
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                keycap("?"),
                Span::styled(
                    format!(" {}   ", language.tr("help")),
                    Style::default().fg(TEXT_MUTED),
                ),
                keycap("^C"),
                Span::styled(
                    format!(" {}", language.tr("stop")),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
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
    let resources = crate::resource::snapshot();
    let process_text = process_queue_text(&resources);
    let cpu_text = match (resources.cpu_percent, resources.sustained_cpu_percent) {
        (Some(cpu), Some(sustained)) => format!(
            "{cpu:.0}% · AVG {sustained:.0}/{:.0}%",
            resources.interactive_cpu_percent
        ),
        (Some(cpu), None) => format!("{cpu:.0}%"),
        _ => format!("— · AVG —/{:.0}%", resources.interactive_cpu_percent),
    };
    let memory_text = resources.resident_memory_bytes.map_or_else(
        || format!("—/{}M", resources.max_memory_bytes / (1024 * 1024)),
        |resident| {
            format!(
                "{}/{}M",
                resident / (1024 * 1024),
                resources.max_memory_bytes / (1024 * 1024)
            )
        },
    );
    let cpu_color = if resources.cpu_pressure {
        DANGER
    } else if resources
        .sustained_cpu_percent
        .is_some_and(|cpu| cpu > resources.interactive_cpu_percent)
    {
        WARNING
    } else {
        SUCCESS
    };
    let memory_color = match resources.memory_pressure {
        crate::resource::MemoryPressure::Normal => SUCCESS,
        crate::resource::MemoryPressure::Elevated => WARNING,
        crate::resource::MemoryPressure::Critical | crate::resource::MemoryPressure::OverLimit => {
            DANGER
        }
    };
    let idle = snapshot
        .last_mcp_seen
        .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300));
    let (icon, state, detail, color) = if snapshot.tunnel_running == Some(false) {
        (
            "ERR",
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
            "ERR",
            language.tr("PUBLIC URL UNAVAILABLE"),
            format!(
                "{} consecutive health checks failed",
                snapshot.public_url_consecutive_failures
            ),
            DANGER,
        )
    } else if snapshot.chatgpt_connected && idle {
        (
            "IDLE",
            language.tr("MCP client idle"),
            format!(
                "last seen {} · HTTP/SSE",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            WARNING,
        )
    } else if snapshot.chatgpt_connected {
        (
            "LIVE",
            language.tr("MCP client connected"),
            format!(
                "last seen {} · HTTP/SSE",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            SUCCESS,
        )
    } else if snapshot.oauth_authorized {
        (
            "AUTH",
            language.tr("OAuth authorized"),
            language.tr("waiting for MCP handshake").to_owned(),
            WARNING,
        )
    } else {
        (
            "SETUP",
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
                " wcode ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {} ", config.version), Style::default().fg(ACCENT)),
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
    if inner.height < 6 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(format!("{icon} "), Style::default().fg(color)),
                    Span::styled(
                        state,
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("MCP  ", Style::default().fg(TEXT_DIM)),
                    Span::styled(
                        truncate_middle(&config.mcp_url(), inner.width.saturating_sub(5) as usize),
                        Style::default().fg(LINK),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("SLOTS {} / {}", totals.active, config.max_parallel),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  WAIT {}", totals.queued),
                        Style::default().fg(WARNING),
                    ),
                    Span::styled(
                        format!("  FAIL {}", totals.failed),
                        Style::default().fg(if totals.failed > 0 { DANGER } else { TEXT_DIM }),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("CPU ", Style::default().fg(TEXT_DIM)),
                    Span::styled(cpu_text.clone(), Style::default().fg(cpu_color)),
                    Span::styled(
                        format!("  MEM {memory_text}"),
                        Style::default().fg(memory_color),
                    ),
                ]),
            ]),
            inner,
        );
        return;
    }
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
                Span::styled("   CPU ", Style::default().fg(TEXT_DIM)),
                Span::styled(cpu_text.clone(), Style::default().fg(cpu_color)),
            ]),
            Line::from(vec![
                Span::styled("WAIT ", Style::default().fg(TEXT_DIM)),
                Span::styled(totals.queued.to_string(), Style::default().fg(WARNING)),
                Span::styled("   FAIL ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    totals.failed.to_string(),
                    Style::default().fg(if totals.failed > 0 { DANGER } else { TEXT_DIM }),
                ),
                Span::styled("   MEM ", Style::default().fg(TEXT_DIM)),
                Span::styled(memory_text.clone(), Style::default().fg(memory_color)),
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
                Span::styled(
                    if snapshot.observed_active > 0 || snapshot.observed_queued > 0 {
                        spinner_frame(tick)
                    } else {
                        "●"
                    },
                    Style::default().fg(ACCENT),
                ),
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
                Span::styled("WAIT ", Style::default().fg(TEXT_DIM)),
                Span::styled(totals.queued.to_string(), Style::default().fg(WARNING)),
                Span::styled("   FAIL ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    totals.failed.to_string(),
                    Style::default().fg(if totals.failed > 0 { DANGER } else { TEXT_DIM }),
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
                Span::styled("   CPU ", Style::default().fg(TEXT_DIM)),
                Span::styled(cpu_text, Style::default().fg(cpu_color)),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("TUNNEL  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    tunnel_status_text(snapshot),
                    Style::default().fg(tunnel_status_color(snapshot)),
                ),
                Span::styled("   MEM ", Style::default().fg(TEXT_DIM)),
                Span::styled(memory_text, Style::default().fg(memory_color)),
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
                    "Endpoint unavailable · local MCP still available"
                } else {
                    &process_text
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
        "● AUTHORIZED"
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
