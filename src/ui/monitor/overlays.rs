use super::*;

pub(super) fn render_workspace_input_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    input: &str,
    language: UiLanguage,
) {
    let width = area.width.saturating_sub(8).clamp(36, 88).min(area.width);
    let height = area.height.saturating_sub(2).clamp(3, 7).min(area.height);
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
        .border_style(Style::default().fg(LINK))
        .style(Style::default().bg(SURFACE_SELECTED))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("AUTHORIZE WORKSPACE")),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
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
                Style::default().fg(if input.is_empty() { TEXT_MUTED } else { TEXT }),
            )),
            Line::from(Span::styled(
                language.tr("Enter authorize · Esc cancel · hard safety boundaries still apply"),
                Style::default().fg(TEXT_DIM),
            )),
        ]),
        inner,
    );
}

pub(super) fn render_full_access_overlay(frame: &mut Frame<'_>, area: Rect, language: UiLanguage) {
    if area.width < 48 || area.height < 12 {
        return;
    }
    let width = area.width.saturating_sub(10).clamp(46, 96);
    let height = 10;
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
        .border_style(Style::default().fg(DANGER))
        .style(Style::default().bg(SURFACE_SELECTED))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("FULL ACCESS")),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("Grant wcode system-tool access for this session?"),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                language.tr("Adds your Home directory as a Workspace and enables repository-aware execution plus destructive replacements."),
                Style::default().fg(WARNING),
            )),
            Line::from(Span::styled(
                language.tr("Hard boundaries stay enforced: protected credentials, .env, symlinks, hard links, shells, and filesystem-root escape remain blocked."),
                Style::default().fg(TEXT_MUTED),
            )),
            Line::from(""),
            Line::from(vec![
                keycap("Y"),
                Span::styled(
                    format!(" {}   ", language.tr("grant full access")),
                    Style::default().fg(DANGER),
                ),
                keycap("N / Esc"),
                Span::styled(
                    format!(" {}", language.tr("cancel")),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
        ]),
        inner,
    );
}

pub(super) fn render_authorization_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    requests: &[AuthorizationRequest],
    focus: usize,
    detail_scroll: usize,
    language: UiLanguage,
) {
    if requests.is_empty() || area.width < 40 || area.height < 10 {
        return;
    }
    let focus = focus.min(requests.len() - 1);
    let visible = requests.len().min(3);
    let start = focus
        .saturating_sub(visible / 2)
        .min(requests.len().saturating_sub(visible));
    let end = (start + visible).min(requests.len());
    let width = area.width.saturating_sub(8).clamp(38, 104);
    let height = (visible as u16 + 14).min(area.height.saturating_sub(4));
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
        .border_style(Style::default().fg(WARNING))
        .style(Style::default().bg(SURFACE_SELECTED))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " ! ",
                Style::default()
                    .fg(WARNING)
                    .bg(SURFACE_SELECTED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("AUTHORIZATION REQUIRED")),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" {} {} ", requests.len(), language.tr("PENDING")),
                Style::default().fg(WARNING),
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
            crate::authorization::AuthorizationKind::CommandAccess => {
                language.tr("Executable access")
            }
            crate::authorization::AuthorizationKind::RiskyExecution => {
                language.tr("Exact repository operation")
            }
            crate::authorization::AuthorizationKind::DestructiveDelete => language.tr("DELETE"),
        };
        let prefix = if selected { "›" } else { " " };
        let summary_width = inner.width.saturating_sub(39) as usize;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{prefix} {} ", request.id),
                Style::default()
                    .fg(if selected { TEXT } else { TEXT_MUTED })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(
                format!("[{kind}] "),
                Style::default().fg(if selected { WARNING } else { TEXT_DIM }),
            ),
            Span::styled(
                format!("{} · ", request.workspace),
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                truncate_end(&request.summary, summary_width.max(8)),
                Style::default().fg(if selected { TEXT } else { TEXT_MUTED }),
            ),
        ]));
    }
    let wide_controls = inner.width >= 64;
    let control_rows = if wide_controls { 2 } else { 1 };
    let detail_rows = usize::from(inner.height).saturating_sub(visible + 1 + control_rows);
    if detail_rows > 0 {
        lines.push(Line::styled(
            if language == UiLanguage::ZhCn {
                "请求详情 · PgUp/PgDn 翻页"
            } else {
                "Request details · PgUp/PgDn to scroll"
            },
            Style::default().fg(ACCENT),
        ));
        let request = &requests[focus];
        let details = authorization_detail_lines(
            &format!(
                "{} · {}\n{}",
                request.id, request.workspace, request.summary
            ),
            usize::from(inner.width),
        );
        let offset = detail_scroll.min(details.len().saturating_sub(detail_rows));
        lines.extend(details.into_iter().skip(offset).take(detail_rows));
        while lines.len() < usize::from(inner.height).saturating_sub(control_rows) {
            lines.push(Line::raw(""));
        }
    }
    if wide_controls {
        lines.push(Line::from(vec![
            keycap("A"),
            Span::styled(
                format!(" {}", language.tr("authorize all commands")),
                Style::default().fg(WARNING),
            ),
        ]));
        lines.push(Line::from(vec![
            keycap("↑/↓"),
            Span::styled(
                format!(" {}   ", language.tr("select request")),
                Style::default().fg(TEXT_MUTED),
            ),
            keycap("Y"),
            Span::styled(
                format!(" {}   ", language.tr("approve selected")),
                Style::default().fg(SUCCESS),
            ),
            keycap("N"),
            Span::styled(
                format!(" {}", language.tr("deny selected")),
                Style::default().fg(DANGER),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            keycap("A"),
            Span::styled(
                format!(" {}  ", language.tr("all")),
                Style::default().fg(WARNING),
            ),
            keycap("Y"),
            Span::styled(
                format!(" {}  ", language.tr("approve")),
                Style::default().fg(SUCCESS),
            ),
            keycap("N"),
            Span::styled(
                format!(" {}", language.tr("deny")),
                Style::default().fg(DANGER),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn authorization_detail_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let cell_width = Span::raw(character.to_string()).width();
        if character == '\n' || (used + cell_width > width && !current.is_empty()) {
            lines.push(Line::raw(std::mem::take(&mut current)));
            used = 0;
        }
        if character != '\n' {
            current.push(character);
            used += cell_width;
        }
    }
    if !current.is_empty() {
        lines.push(Line::raw(current));
    }
    lines
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
                .border_style(Style::default().fg(LINK))
                .style(Style::default().bg(SURFACE_SELECTED))
                .title(Span::styled(
                    format!(" {} ", language.tr("STATUS")),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
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
    let popup = help_popup(area);
    let width = popup.width;
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE_SELECTED))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " ? ",
                Style::default()
                    .fg(ACCENT)
                    .bg(SURFACE_SELECTED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("HELP & LINKS")),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" {} ", language.tr("ESC TO CLOSE")),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if width < 74 {
        frame.render_widget(
            Paragraph::new(vec![
                help_hint_line("I", language.tr("show repository intelligence")),
                help_hint_line("C", language.tr("show supported commands")),
                help_hint_line("O", language.tr("open Connector setup")),
                help_hint_line("W", language.tr("open Engineering Observatory")),
                help_hint_line("L", language.tr("toggle language")),
                help_hint_line("P", language.tr("grant full user access")),
                help_hint_line(
                    "A / ↑↓ Y/N",
                    language.tr("all / select / approve / deny authorization"),
                ),
                help_hint_line("? / Esc", language.tr("open or close help")),
                help_hint_line("^C", language.tr("stop wcode")),
                pairing_code_line(config, language),
                help_link_line(language.tr("Project"), &config.project_url, inner.width),
                help_link_line(
                    &format!(
                        "{} {}",
                        super::monitor_runtime::AUTHOR_SHORTCUT,
                        language.tr("Author")
                    ),
                    &config.author_url,
                    inner.width,
                ),
                help_link_line(
                    language.tr("Setup"),
                    config.setup_url().as_str(),
                    inner.width,
                ),
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
                Style::default().fg(TEXT_DIM).add_modifier(Modifier::BOLD),
            )),
            help_hint_line("← / →", language.tr("move workspace focus")),
            help_hint_line("Shift + ← / →", language.tr("move one workspace page")),
            help_hint_line("O", language.tr("open Connector setup")),
            help_hint_line("W", language.tr("open Engineering Observatory")),
            help_hint_line("I", language.tr("show repository intelligence")),
            help_hint_line("C", language.tr("show supported commands")),
            help_hint_line("L", language.tr("toggle language")),
            help_hint_line("P", language.tr("grant full user access")),
            help_hint_line(
                "A",
                language.tr("authorize all commands for selected workspace"),
            ),
            help_hint_line(
                "F",
                language.tr("toggle all command authorization in command view"),
            ),
            help_hint_line("↑ / ↓", language.tr("select authorization")),
            help_hint_line("Y", language.tr("approve selected authorization")),
            help_hint_line("N", language.tr("deny selected authorization")),
            help_hint_line("? / Esc", language.tr("open or close help")),
            help_hint_line("Ctrl-C", language.tr("stop wcode")),
        ])
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(OUTLINE))
                .padding(Padding::new(0, 2, 0, 0)),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("RUNTIME"),
                Style::default().fg(TEXT_DIM).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("Slots  ", Style::default().fg(TEXT)),
                Span::styled("active child tasks / cap", Style::default().fg(TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("Peak   ", Style::default().fg(TEXT)),
                Span::styled(
                    "real concurrency high-water mark",
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
            Line::from(vec![
                Span::styled("Fan-out", Style::default().fg(TEXT)),
                Span::styled(
                    "  parallel_tools · review · verify",
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
            Line::from(vec![
                Span::styled("CTX    ", Style::default().fg(TEXT)),
                Span::styled(
                    "estimated tool-output tokens",
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
            Line::from(vec![
                Span::styled("Saved  ", Style::default().fg(TEXT)),
                Span::styled(
                    format!(
                        "AST context avoided · EST at ${:.2}/M",
                        config.input_token_price_per_million_usd
                    ),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
            Line::from(""),
            pairing_code_line(config, language),
            help_link_line(
                language.tr("Project"),
                &config.project_url,
                columns[1].width,
            ),
            help_link_line(
                &format!(
                    "{} {}",
                    super::monitor_runtime::AUTHOR_SHORTCUT,
                    language.tr("Author")
                ),
                &config.author_url,
                columns[1].width,
            ),
            help_link_line(
                language.tr("Setup"),
                config.setup_url().as_str(),
                columns[1].width,
            ),
            help_link_line(
                language.tr("Health"),
                &config.local_health_url,
                columns[1].width,
            ),
        ]),
        columns[1],
    );
}

pub(super) fn help_link_at(
    point: (u16, u16),
    area: Rect,
    config: &MonitorConfig,
) -> Option<String> {
    let popup = help_popup(area);
    let inner = Rect::new(
        popup.x.saturating_add(2),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(2),
    );
    let setup_url = config.setup_url();
    if popup.width < 74 {
        return link_at_rows(
            point,
            inner,
            &[
                (10, config.project_url.as_str()),
                (11, config.author_url.as_str()),
                (12, setup_url.as_str()),
                (13, config.local_health_url.as_str()),
            ],
        );
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(inner);
    link_at_rows(
        point,
        columns[1],
        &[
            (8, config.project_url.as_str()),
            (9, config.author_url.as_str()),
            (10, setup_url.as_str()),
            (11, config.local_health_url.as_str()),
        ],
    )
}

fn help_popup(area: Rect) -> Rect {
    let width = area.width.saturating_sub(2).clamp(36, 98).min(area.width);
    let height = area.height.saturating_sub(2).clamp(12, 20).min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn link_at_rows(point: (u16, u16), area: Rect, links: &[(u16, &str)]) -> Option<String> {
    if !point_in_rect(point, area) {
        return None;
    }
    links.iter().find_map(|(row, url)| {
        let link = Rect::new(area.x, area.y.saturating_add(*row), area.width, 1);
        (point.0 >= link.x
            && point.0 < link.x.saturating_add(link.width)
            && point.1 >= link.y
            && point.1 < link.y.saturating_add(link.height))
        .then(|| (*url).to_owned())
    })
}

fn pairing_code_line(config: &MonitorConfig, language: UiLanguage) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}  ", language.tr("Pairing code")),
            Style::default().fg(TEXT_DIM),
        ),
        Span::styled(
            config.pairing_code.clone(),
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", language.tr("valid for this run")),
            Style::default().fg(TEXT_MUTED),
        ),
    ])
}

fn help_hint_line(key: &str, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<15}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label.to_owned(), Style::default().fg(TEXT_MUTED)),
    ])
}

pub(super) fn help_link_line(label: &str, url: &str, width: u16) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(TEXT_DIM)),
        Span::styled(
            truncate_middle(url, width.saturating_sub(10) as usize),
            Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED),
        ),
    ])
}

pub(super) fn slot_bar(active: u64, capacity: u64, width: usize) -> (String, String, Color) {
    let capacity = capacity.max(1);
    let ratio = (active as f64 / capacity as f64).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let color = if ratio >= 0.85 {
        DANGER
    } else if ratio >= 0.6 {
        WARNING
    } else {
        ACCENT
    };
    (
        "━".repeat(filled),
        "·".repeat(width.saturating_sub(filled)),
        color,
    )
}
