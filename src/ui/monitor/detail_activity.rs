use super::*;

pub(in crate::monitor) fn render_workspace_activity(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    ui: &DashboardState,
) {
    let workspaces = ordered_workspaces(config, snapshot);
    let total = workspaces.len();
    let visible = workspace_column_count(area.width, total);
    let offset = workspace_window(ui.workspace_focus, ui.workspace_offset, total, visible);
    let end = (offset + visible).min(total);
    let range = if total == 0 {
        "0 / 0".to_owned()
    } else {
        format!("{}–{} / {}", offset + 1, end, total)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(SURFACE_RAISED))
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
        let approvals = ui
            .pending_authorizations
            .iter()
            .filter(|request| request.workspace == *id)
            .count();
        let last_activity = workspace_recent_activity(snapshot, id);
        let recent = !active && !queued && approvals == 0 && last_activity.is_some();
        let recent_failure = recent && workspace_recent_failure(snapshot, id);
        let focused = workspace_index == ui.workspace_focus;
        let status_color = if active {
            ACCENT
        } else if queued || approvals > 0 {
            WARNING
        } else if recent_failure {
            DANGER
        } else if recent {
            LINK
        } else {
            TEXT_DIM
        };
        let border_color = if focused {
            LINK
        } else if active {
            ACCENT
        } else if queued || approvals > 0 {
            WARNING
        } else if recent_failure {
            DANGER
        } else {
            OUTLINE
        };
        let summary = if active || queued {
            let approval_suffix = if approvals > 0 {
                format!(" · {approvals} auth")
            } else {
                String::new()
            };
            format!(
                "{} run · {} wait{approval_suffix}",
                stats.active, stats.queued
            )
        } else if approvals > 0 {
            format!("{approvals} auth")
        } else if let Some(last_activity) = last_activity {
            let label = if recent_failure { "failed" } else { "recent" };
            format!("{label} · {}", last_seen_text(Some(last_activity)))
        } else {
            "idle".to_owned()
        };
        let title_width = column.width.saturating_sub(10) as usize;
        let card = Block::default()
            .borders(Borders::ALL)
            .border_type(if focused {
                BorderType::Double
            } else {
                BorderType::Rounded
            })
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
                    Style::default().fg(if *is_default { ACCENT } else { TEXT_DIM }),
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
    if capacity == 0 {
        return Vec::new();
    }
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.workspace == workspace)
        .collect::<Vec<_>>();
    // Only fully sort the visible window. For large task histories this avoids
    // O(n log n) work every frame when the card only shows a handful of rows.
    if tasks.len() > capacity {
        tasks.select_nth_unstable_by(capacity - 1, |a, b| {
            activity_rank(a.status)
                .cmp(&activity_rank(b.status))
                .then_with(|| task_time(b).cmp(&task_time(a)))
        });
        tasks.truncate(capacity);
    }
    tasks.sort_by(|a, b| {
        activity_rank(a.status)
            .cmp(&activity_rank(b.status))
            .then_with(|| task_time(b).cmp(&task_time(a)))
    });
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
