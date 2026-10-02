use super::*;

pub(super) fn render_too_small(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    minimum_height: u16,
    language: UiLanguage,
) {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " wcode ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("project status")),
                Style::default().fg(ACCENT),
            ),
        ]));
    if area.width >= 64 {
        block = block.title(
            Line::from(Span::styled(
                format!(" INSTANCE {} ", truncate_end(&config.instance_id, 8)),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    // Reserve the recovery controls before allocating explanatory text. Tiny
    // windows can clip the body, but still retain help and stop shortcuts.
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            keycap("?"),
            Span::raw(" "),
            keycap("^C"),
            Span::styled(
                format!("  {} / {}", language.tr("help"), language.tr("stop")),
                Style::default().fg(TEXT_MUTED),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("Terminal needs a little more room"),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(
                    "{} {} × {} · {} 40 × {}",
                    language.tr("Current"),
                    area.width,
                    area.height,
                    language.tr("Minimum"),
                    minimum_height,
                ),
                Style::default().fg(TEXT_MUTED),
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
            Line::from(Span::styled(
                language.tr("Resize to continue"),
                Style::default().fg(TEXT_DIM),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center),
        rows[0],
    );
}
