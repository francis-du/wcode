use super::*;

impl TaskMonitor {
    pub(crate) fn record_agent_context_metrics(
        &self,
        workspace: &str,
        metrics: AgentContextMetrics,
    ) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let stats = state.workspaces.entry(workspace.to_owned()).or_default();
        stats.agent_context_calls = stats.agent_context_calls.saturating_add(1);
        stats.agent_context_model_bytes = stats
            .agent_context_model_bytes
            .saturating_add(metrics.model_bytes);
        stats.agent_context_model_tokens = stats
            .agent_context_model_tokens
            .saturating_add(metrics.model_tokens);
        stats.agent_context_budget_tokens = stats
            .agent_context_budget_tokens
            .saturating_add(metrics.budget_tokens);
        stats.agent_context_bytes_avoided = stats
            .agent_context_bytes_avoided
            .saturating_add(metrics.context_bytes_avoided);
        stats.agent_repo_map_cache_hits = stats
            .agent_repo_map_cache_hits
            .saturating_add(u64::from(metrics.repo_map_cache_hit));
        stats.agent_repo_map_candidates = stats
            .agent_repo_map_candidates
            .saturating_add(metrics.repo_map_candidates);
        stats.agent_repo_map_delivered = stats
            .agent_repo_map_delivered
            .saturating_add(metrics.repo_map_delivered);
        stats.agent_context_build_ms = stats
            .agent_context_build_ms
            .saturating_add(metrics.build_ms);
    }
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
    let bins = request_bins(snapshot, 10, Duration::from_secs(3));
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
        .constraints(if inner.width < 100 {
            [Constraint::Percentage(100), Constraint::Percentage(0)]
        } else {
            [Constraint::Percentage(58), Constraint::Percentage(42)]
        })
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
    let agent_context = if totals.agent_context_calls > 0 {
        let budget_use = if totals.agent_context_budget_tokens == 0 {
            0
        } else {
            totals
                .agent_context_model_tokens
                .saturating_mul(100)
                .saturating_div(totals.agent_context_budget_tokens)
                .min(100)
        };
        format!(
            "CTX {}/{} · BUD {}% · HIT {}/{} · {}ms",
            totals.agent_repo_map_delivered,
            totals.agent_repo_map_candidates,
            budget_use,
            totals.agent_repo_map_cache_hits,
            totals.agent_context_calls,
            totals.agent_context_build_ms / totals.agent_context_calls,
        )
    } else {
        "SLOT UTILIZATION".to_owned()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(agent_context, Style::default().fg(TEXT_DIM))).right_aligned(),
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

pub(super) fn process_queue_text(resources: &crate::resource::ResourceSnapshot) -> String {
    let waiting = resources
        .child_queue
        .waiting
        .saturating_add(resources.probe_queue.waiting);
    let max_wait_ms = resources
        .child_queue
        .max_wait_ms
        .max(resources.probe_queue.max_wait_ms);
    let wait = if max_wait_ms > 0 {
        format!(" · WAIT {max_wait_ms}ms")
    } else {
        String::new()
    };
    format!(
        "PROC {}/{} · GIT {}/{} · Q {}{}",
        resources.child_queue.active,
        resources.child_queue.limit,
        resources.probe_queue.active,
        resources.probe_queue.limit,
        waiting,
        wait,
    )
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
                let index = u128::from(*value) * (LEVELS.len() - 1) as u128 / u128::from(maximum);
                LEVELS[index as usize]
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

pub(super) fn truncate_end(value: &str, max_columns: usize) -> String {
    if max_columns == 0 {
        return String::new();
    }
    let span = Span::raw(value);
    if span.width() <= max_columns {
        return value.to_owned();
    }
    let mut output = String::new();
    let mut remaining = max_columns - 1;
    for grapheme in span.styled_graphemes(Style::default()) {
        let width = Span::raw(grapheme.symbol).width();
        if width > remaining {
            break;
        }
        output.push_str(grapheme.symbol);
        remaining -= width;
    }
    output.push('…');
    output
}

pub(super) fn truncate_middle(value: &str, max_columns: usize) -> String {
    if max_columns == 0 {
        return String::new();
    }
    let span = Span::raw(value);
    if span.width() <= max_columns {
        return value.to_owned();
    }
    let graphemes = span
        .styled_graphemes(Style::default())
        .map(|grapheme| (grapheme.symbol, Span::raw(grapheme.symbol).width()))
        .collect::<Vec<_>>();
    let mut start = String::new();
    let mut left = (max_columns - 1) / 2;
    for (symbol, width) in &graphemes {
        if *width > left {
            break;
        }
        start.push_str(symbol);
        left -= width;
    }
    let mut right = max_columns - 1 - Span::raw(start.as_str()).width();
    let mut suffix = graphemes.len();
    while suffix > 0 && graphemes[suffix - 1].1 <= right {
        suffix -= 1;
        right -= graphemes[suffix].1;
    }
    start.push('…');
    for (symbol, _) in &graphemes[suffix..] {
        start.push_str(symbol);
    }
    start
}
