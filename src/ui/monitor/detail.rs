use super::*;

pub(super) fn tunnel_status_text(snapshot: &MonitorSnapshot) -> &'static str {
    match snapshot.tunnel_running {
        Some(true) => "● RUNNING",
        Some(false) => "× EXITED",
        None if snapshot.public_endpoint.as_deref() == Some("pending") => "◌ CONNECTING",
        None => "○ EXTERNAL / LOCAL",
    }
}

pub(super) fn tunnel_status_color(snapshot: &MonitorSnapshot) -> Color {
    match snapshot.tunnel_running {
        Some(true) => SUCCESS,
        Some(false) => DANGER,
        None => TEXT_MUTED,
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
    let mcp_url = config.mcp_url();
    let lifecycle = if is_quick_tunnel(&mcp_url) {
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
            _ => snapshot
                .tunnel_activity
                .as_deref()
                .map(|activity| truncate_end(activity, 28))
                .unwrap_or_else(|| "waiting".to_owned()),
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
        .border_style(Style::default().fg(WARNING))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("SETUP")),
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                format!(" {lifecycle} "),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 78 || inner.height < 7 {
        let mut lines = vec![
            setup_step(1, language.tr("Open this wcode setup page")),
            setup_step(2, language.tr("Add the MCP URL and choose OAuth")),
            Line::from(vec![
                Span::styled("  VERIFY CODE ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    &config.pairing_code,
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "   ·   Streamable HTTP and legacy SSE",
                    Style::default().fg(TEXT_MUTED),
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
        let link_width = inner.width.saturating_sub(8) as usize;
        if snapshot.tunnels.is_empty() {
            lines.extend(endpoint_link_lines(snapshot, config, link_width));
        } else {
            lines.extend(tunnel_lines(snapshot, link_width));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);
    let left = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(OUTLINE))
        .padding(Padding::new(0, 2, 0, 0));
    frame.render_widget(
        Paragraph::new(
            vec![
                Line::from(Span::styled(
                    "GET CONNECTED",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                )),
                setup_step(1, language.tr("Open this setup page · press O")),
                setup_step(2, language.tr("Choose a compatible MCP client")),
                setup_step(3, language.tr("Add MCP URL · Auth: OAuth")),
                Line::from(vec![
                    Span::styled("  VERIFY CODE  ", Style::default().fg(TEXT_DIM)),
                    Span::styled(
                        &config.pairing_code,
                        Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "   press O to reopen setup",
                        Style::default().fg(TEXT_MUTED),
                    ),
                ]),
            ]
            .into_iter()
            .chain(if snapshot.tunnels.is_empty() {
                endpoint_link_lines(
                    snapshot,
                    config,
                    columns[0].width.saturating_sub(10) as usize,
                )
            } else {
                tunnel_lines(snapshot, columns[0].width.saturating_sub(10) as usize)
            })
            .collect::<Vec<_>>(),
        )
        .block(left),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "CONNECTION",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
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
        ]),
        columns[1],
    );
}

fn tunnel_lines(snapshot: &MonitorSnapshot, width: usize) -> Vec<Line<'static>> {
    snapshot
        .tunnels
        .iter()
        .map(|(provider, url)| {
            // Host + /mcp without the repeated https:// prefix keeps rows
            // readable when four providers are listed at once.
            let host = url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/');
            Line::from(vec![
                Span::styled(format!("  {provider:<13}"), Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate_middle(&format!("{host}/mcp"), width),
                    Style::default().fg(LINK),
                ),
            ])
        })
        .collect()
}

/// Fallback link row shown while tunnels are still connecting or when the
/// endpoint is fixed (external URL) or local-only.
fn endpoint_link_lines(
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    width: usize,
) -> Vec<Line<'static>> {
    match snapshot.public_endpoint.as_deref() {
        Some("pending") => vec![Line::from(Span::styled(
            "  TUNNELS      connecting…",
            Style::default().fg(TEXT_MUTED),
        ))],
        Some("local-only") => vec![Line::from(vec![
            Span::styled("  LOCAL        ", Style::default().fg(TEXT_DIM)),
            Span::styled(
                truncate_middle(&config.mcp_url(), width),
                Style::default().fg(LINK),
            ),
        ])],
        _ => vec![Line::from(vec![
            Span::styled("  MCP          ", Style::default().fg(TEXT_DIM)),
            Span::styled(
                truncate_middle(&config.mcp_url(), width),
                Style::default().fg(LINK),
            ),
        ])],
    }
}

fn setup_step(number: u8, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {number} "),
            Style::default()
                .fg(BACKGROUND)
                .bg(SECONDARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {label}"), Style::default().fg(TEXT)),
    ])
}

fn setup_state(done: bool, label: &str, detail: &str) -> Line<'static> {
    let color = if done { SUCCESS } else { TEXT_DIM };
    Line::from(vec![
        Span::styled(
            if done { "  ● " } else { "  ○ " },
            Style::default().fg(color),
        ),
        Span::styled(format!("{label:<14}"), Style::default().fg(TEXT)),
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
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", ui.language.tr("WORKSPACE ACTIVITY")),
            Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(vec![
                Span::styled("VIEW  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    range,
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ← → ", Style::default().fg(TEXT_DIM)),
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
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        ui.language
                            .tr("restart wcode with one or more --workspace paths"),
                        Style::default().fg(TEXT_MUTED),
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
            ACCENT
        } else if queued {
            WARNING
        } else if stats.failed > 0 {
            DANGER
        } else {
            TEXT_DIM
        };
        let border_color = if focused {
            LINK
        } else if active {
            ACCENT
        } else {
            OUTLINE
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
            .style(Style::default().bg(if focused {
                SURFACE_SELECTED
            } else {
                SURFACE_RAISED
            }))
            .padding(Padding::horizontal(1))
            .title(Line::from(vec![
                Span::styled(
                    if focused { " ▸ " } else { "   " },
                    Style::default().fg(LINK),
                ),
                Span::styled("● ", Style::default().fg(status_color)),
                Span::styled(
                    truncate_end(id, title_width),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]))
            .title_bottom(Line::from(Span::styled(
                format!(" {summary} "),
                Style::default().fg(status_color),
            )))
            .title_bottom(
                Line::from(Span::styled(
                    if *is_default { " DEFAULT " } else { " " },
                    Style::default().fg(if *is_default { SECONDARY } else { TEXT_DIM }),
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
                    Line::from(Span::styled("quiet", Style::default().fg(TEXT_MUTED))),
                    Line::from(Span::styled(
                        truncate_middle(path, card_inner.width as usize),
                        Style::default().fg(TEXT_DIM),
                    )),
                ]
            } else {
                vec![Line::from(Span::styled(
                    "quiet",
                    Style::default().fg(TEXT_MUTED),
                ))]
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
        TaskStatus::Queued => ("◌".to_owned(), WARNING, true),
        TaskStatus::Running => (
            spinner_frame(tick.wrapping_add(task.id as usize)).to_owned(),
            ACCENT,
            true,
        ),
        TaskStatus::Completed => ("✓".to_owned(), SUCCESS, false),
        TaskStatus::Failed => ("×".to_owned(), DANGER, true),
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
                .fg(if highlighted { TEXT } else { TEXT_MUTED })
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
            Style::default().fg(TEXT_DIM),
        ));
    }
    spans.push(Span::styled(
        format!(" {elapsed:>elapsed_width$}"),
        Style::default().fg(color),
    ));

    ListItem::new(Line::from(spans)).style(Style::default().bg(if highlighted {
        SURFACE_SELECTED
    } else {
        SURFACE_RAISED
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
    crate::tunnel::is_quick_tunnel_url(mcp_url)
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

pub(super) fn render_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND)),
        area,
    );
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

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);
    if area.width >= 124 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "  wcode  ",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    project.to_owned(),
                    Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED),
                ),
                Span::styled("  by  ", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    config.author_handle.clone(),
                    Style::default()
                        .fg(SECONDARY)
                        .add_modifier(Modifier::UNDERLINED),
                ),
            ])),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                keycap("←/→"),
                Span::styled(
                    format!(" {}  ", language.tr("workspace")),
                    Style::default().fg(TEXT_MUTED),
                ),
                keycap("O"),
                Span::styled(
                    format!(" {}  ", language.tr("setup")),
                    Style::default().fg(TEXT_MUTED),
                ),
                keycap("W"),
                Span::styled(
                    format!(" {}  ", language.tr("web")),
                    Style::default().fg(TEXT_MUTED),
                ),
                keycap("I"),
                Span::raw(" "),
                keycap("C"),
                Span::raw(" "),
                keycap("L"),
                Span::styled(
                    format!(" {}  ", language.name()),
                    Style::default().fg(TEXT_MUTED),
                ),
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
                        WARNING
                    } else {
                        TEXT_MUTED
                    }),
                ),
                keycap("?"),
                Span::raw(" "),
                keycap("^C"),
            ]))
            .alignment(ratatui::layout::Alignment::Right),
            rows[1],
        );
        return;
    }

    let line = if area.width >= 78 {
        Line::from(vec![
            Span::raw(" "),
            keycap("←/→"),
            Span::styled(
                format!(" {}  ", language.tr("workspace")),
                Style::default().fg(TEXT_MUTED),
            ),
            keycap("O"),
            Span::styled(
                format!(" {}  ", language.tr("setup")),
                Style::default().fg(TEXT_MUTED),
            ),
            keycap("W"),
            Span::styled(
                format!(" {}  ", language.tr("web")),
                Style::default().fg(TEXT_MUTED),
            ),
            keycap("I"),
            Span::raw(" "),
            keycap("C"),
            Span::raw(" "),
            keycap("L"),
            Span::styled(
                format!(" {}  ", language.name()),
                Style::default().fg(TEXT_MUTED),
            ),
            keycap("Y/N"),
            Span::styled(
                if pending_authorizations > 0 {
                    format!(" {pending_authorizations}  ")
                } else {
                    "  ".to_owned()
                },
                Style::default().fg(if pending_authorizations > 0 {
                    WARNING
                } else {
                    TEXT_MUTED
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
            keycap("I"),
            Span::raw("  "),
            keycap("C"),
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
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "  wcode  ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", language.tr("Pairing code")),
                Style::default().fg(TEXT_DIM),
            ),
            Span::styled(
                config.pairing_code.clone(),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[0],
    );
    frame.render_widget(Paragraph::new(line), rows[1]);
}

pub(super) fn keycap(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(TEXT)
            .bg(SURFACE_RAISED)
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
            total.agent_context_calls = total
                .agent_context_calls
                .saturating_add(stats.agent_context_calls);
            total.agent_context_model_bytes = total
                .agent_context_model_bytes
                .saturating_add(stats.agent_context_model_bytes);
            total.agent_context_bytes_avoided = total
                .agent_context_bytes_avoided
                .saturating_add(stats.agent_context_bytes_avoided);
            total.agent_repo_map_cache_hits = total
                .agent_repo_map_cache_hits
                .saturating_add(stats.agent_repo_map_cache_hits);
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
