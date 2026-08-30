use super::*;

pub(super) fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    compact: bool,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let observed_active = snapshot.observed_active.max(totals.active);
    let observed_queued = snapshot.observed_queued.max(totals.queued);
    let success = success_rate(totals.completed, totals.failed);
    let (requests, rx_30s, tx_30s) = window_totals(snapshot, Duration::from_secs(30));
    let rate = requests as f64 / 30.0;
    let context_tokens = estimated_tokens(totals.response_bytes);
    let saved_tokens = estimated_tokens(totals.context_bytes_avoided);
    let estimated_context_cost = estimated_cost_usd(
        totals.response_bytes,
        config.input_token_price_per_million_usd,
    );
    let estimated_savings = estimated_cost_usd(
        totals.context_bytes_avoided,
        config.input_token_price_per_million_usd,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("OVERVIEW")),
            Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                format!(
                    " 30S  {rate:.1} req/s · RX {} · TX {} ",
                    short_bytes(rx_30s),
                    short_bytes(tx_30s)
                ),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 82 || inner.height < 2 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    compact_metric(language.tr("RUN"), observed_active, ACCENT),
                    Span::raw("   "),
                    compact_metric(language.tr("WAIT"), observed_queued, WARNING),
                    Span::raw("   "),
                    compact_metric(language.tr("DONE"), totals.completed, SUCCESS),
                    Span::raw("   "),
                    compact_metric(
                        language.tr("FAIL"),
                        totals.failed,
                        if totals.failed > 0 { DANGER } else { TEXT_DIM },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("SUCCESS ", Style::default().fg(TEXT_DIM)),
                    Span::styled(format!("{success:.1}%"), Style::default().fg(SUCCESS)),
                    Span::styled("   CTX ~", Style::default().fg(TEXT_DIM)),
                    Span::styled(short_tokens(context_tokens), Style::default().fg(LINK)),
                    Span::styled(
                        format!(" · COST {}", short_usd(estimated_context_cost)),
                        Style::default().fg(LINK),
                    ),
                    Span::styled("   SAVED ~", Style::default().fg(TEXT_DIM)),
                    Span::styled(short_tokens(saved_tokens), Style::default().fg(SECONDARY)),
                    Span::styled(
                        format!(" · SAVE {}", short_usd(estimated_savings)),
                        Style::default().fg(SUCCESS),
                    ),
                ]),
            ]),
            inner,
        );
        return;
    }

    let cards = split_rects_with_gap(inner, 4, 1);
    render_metric_card(
        frame,
        cards[0],
        language.tr("ACTIVE"),
        observed_active.to_string(),
        if observed_active > totals.active {
            format!("now {} · peak {}", totals.active, snapshot.peak_active)
        } else {
            format!("peak {}", snapshot.peak_active)
        },
        ACCENT,
    );
    render_metric_card(
        frame,
        cards[1],
        language.tr("QUEUED"),
        observed_queued.to_string(),
        if observed_queued > totals.queued {
            format!("now {} · recent peak", totals.queued)
        } else {
            "waiting".to_owned()
        },
        WARNING,
    );
    render_metric_card(
        frame,
        cards[2],
        language.tr("COMPLETED"),
        totals.completed.to_string(),
        format!("{success:.1}% success"),
        SUCCESS,
    );
    render_metric_card(
        frame,
        cards[3],
        language.tr("FAILED"),
        totals.failed.to_string(),
        if totals.failed == 0 {
            "clean".to_owned()
        } else {
            "inspect".to_owned()
        },
        if totals.failed > 0 { DANGER } else { TEXT_DIM },
    );
}

fn compact_metric(label: &'static str, value: u64, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label} {value}"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn render_metric_card(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: String,
    detail: String,
    color: Color,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE_RAISED))
        .padding(Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                value,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled(label.to_owned(), Style::default().fg(TEXT_DIM)),
                Span::styled(format!("  {detail}"), Style::default().fg(TEXT_MUTED)),
            ]),
        ])
        .block(block),
        area,
    );
}

pub(super) fn split_rects_with_gap(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let gap_total = gap.saturating_mul(count.saturating_sub(1) as u16);
    let usable = area.width.saturating_sub(gap_total);
    let base = usable / count as u16;
    let remainder = usable % count as u16;
    let mut x = area.x;
    let mut rects = Vec::with_capacity(count);
    for index in 0..count {
        let width = base + u16::from((index as u16) < remainder);
        rects.push(Rect::new(x, area.y, width, area.height));
        x = x.saturating_add(width).saturating_add(gap);
    }
    rects
}

pub(super) fn render_throughput(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let bins = request_bins(snapshot, 12, Duration::from_secs(3));
    let sparkline = sparkline(&bins);
    let (requests, rx, tx) = window_totals(snapshot, Duration::from_secs(30));
    let avoided_30s = window_context_avoided(snapshot, Duration::from_secs(30));
    let req_rate = requests as f64 / 30.0;
    let context_tokens_30s = estimated_tokens(tx);
    let saved_tokens_30s = estimated_tokens(avoided_30s);
    let context_cost_30s = estimated_cost_usd(tx, config.input_token_price_per_million_usd);
    let savings_30s = estimated_cost_usd(avoided_30s, config.input_token_price_per_million_usd);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OUTLINE))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("THROUGHPUT")),
            Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(" 30S WINDOW ", Style::default().fg(TEXT_DIM))).right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("REQUESTS  ", Style::default().fg(TEXT_DIM)),
                Span::styled(sparkline, Style::default().fg(ACCENT)),
                Span::styled(format!("  {req_rate:.1}/s"), Style::default().fg(TEXT)),
                Span::styled("   RX ", Style::default().fg(TEXT_DIM)),
                Span::styled(short_bytes(rx), Style::default().fg(LINK)),
                Span::styled("  TX ", Style::default().fg(TEXT_DIM)),
                Span::styled(short_bytes(tx), Style::default().fg(SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("CTX ~", Style::default().fg(TEXT_DIM)),
                Span::styled(short_tokens(context_tokens_30s), Style::default().fg(LINK)),
                Span::styled(
                    format!(" · COST {}", short_usd(context_cost_30s)),
                    Style::default().fg(LINK),
                ),
                Span::styled("   SAVED ~", Style::default().fg(TEXT_DIM)),
                Span::styled(
                    short_tokens(saved_tokens_30s),
                    Style::default().fg(SECONDARY),
                ),
                Span::styled(
                    format!(" · SAVE {}", short_usd(savings_30s)),
                    Style::default().fg(SUCCESS),
                ),
            ]),
        ]),
        columns[0],
    );

    let bar_width = columns[1].width.saturating_sub(23).clamp(6, 18) as usize;
    let (filled, empty, color) = slot_bar(totals.active, config.max_parallel as u64, bar_width);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "SLOT UTILIZATION",
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
            Line::from(vec![
                Span::styled(
                    filled,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(empty, Style::default().fg(OUTLINE)),
                Span::styled(
                    format!(
                        "  {} / {} · peak {}",
                        totals.active, config.max_parallel, snapshot.peak_active
                    ),
                    Style::default().fg(TEXT),
                ),
            ])
            .right_aligned(),
        ]),
        columns[1],
    );
}

pub(super) fn sparkline(values: &[u64]) -> String {
    const LEVELS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let maximum = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|value| {
            if maximum == 0 {
                LEVELS[0]
            } else {
                let index = (*value as usize * (LEVELS.len() - 1)) / maximum as usize;
                LEVELS[index]
            }
        })
        .collect()
}

pub(super) fn spinner_frame(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}

pub(super) fn short_duration(duration: Duration) -> String {
    if duration.as_secs() >= 60 {
        format!(
            "{}m{:02}s",
            duration.as_secs() / 60,
            duration.as_secs() % 60
        )
    } else if duration.as_secs() >= 1 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

pub(super) fn short_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

pub(super) fn estimated_tokens(bytes: u64) -> u64 {
    (bytes as f64 / ESTIMATED_BYTES_PER_TOKEN).ceil() as u64
}

pub(super) fn short_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.1}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(super) fn estimated_cost_usd(context_bytes: u64, price_per_million: f64) -> f64 {
    estimated_tokens(context_bytes) as f64 * price_per_million.max(0.0) / 1_000_000.0
}

pub(super) fn short_usd(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        "$0".to_owned()
    } else if value >= 1_000.0 {
        format!("${:.1}K", value / 1_000.0)
    } else if value >= 1.0 {
        format!("${value:.2}")
    } else if value >= 0.01 {
        format!("${value:.3}")
    } else if value >= 0.000001 {
        format!("${value:.6}")
    } else {
        "<$0.000001".to_owned()
    }
}

pub(super) fn truncate_end(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_owned()
    } else if max_chars <= 1 {
        "…".to_owned()
    } else {
        let mut output = value.chars().take(max_chars - 1).collect::<String>();
        output.push('…');
        output
    }
}

pub(super) fn truncate_middle(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 3 {
        return "…".to_owned();
    }
    let left = (max_chars - 1) / 2;
    let right = max_chars - left - 1;
    let start = value.chars().take(left).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(right)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{start}…{end}")
}
