use super::*;
use std::collections::{BTreeSet, HashSet};

#[derive(Clone)]
struct CommandEntry {
    program: String,
    allowed: bool,
}

pub(super) fn render_commands_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    workspaces: &Workspaces,
    workspace_id: &str,
    offset: usize,
    language: UiLanguage,
) {
    if area.width < 40 || area.height < 14 {
        return;
    }
    let width = area.width.saturating_sub(2).min(112);
    let height = area.height.saturating_sub(2).min(26);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SECONDARY))
        .style(Style::default().bg(SURFACE_SELECTED))
        .padding(Padding::uniform(1))
        .title(Line::from(vec![
            Span::styled(
                " C ",
                Style::default()
                    .fg(BACKGROUND)
                    .bg(SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", language.tr("SUPPORTED COMMANDS")),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {workspace_id} "), Style::default().fg(ACCENT)),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" C / Esc  {} ", language.tr("close")),
                Style::default().fg(TEXT_DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let (root, entries) = match command_entries(workspaces, workspace_id) {
        Ok(result) => result,
        Err(error) => {
            frame.render_widget(
                Paragraph::new(format!(
                    "{}: {error}",
                    language.tr("unable to load commands")
                )),
                inner,
            );
            return;
        }
    };
    let enabled = entries.iter().filter(|entry| entry.allowed).count();
    let columns = command_columns(inner.width);
    let rows = command_rows(inner.height);
    let capacity = rows.saturating_mul(columns);
    let start = offset.min(entries.len().saturating_sub(capacity));
    let end = start.saturating_add(capacity).min(entries.len());
    let column_width = usize::from(inner.width) / columns.max(1);
    let mut lines = Vec::with_capacity(rows + 7);
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<12}", language.tr("ROOT")),
            Style::default().fg(TEXT_DIM),
        ),
        Span::styled(
            truncate_middle(&root, inner.width.saturating_sub(12) as usize),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} {}", entries.len(), language.tr("supported")),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ·  {enabled} {}", language.tr("enabled")),
            Style::default().fg(SUCCESS),
        ),
    ]));
    lines.push(Line::from(""));

    for row in 0..rows {
        let mut spans = Vec::with_capacity(columns);
        for column in 0..columns {
            let index = start + row + column.saturating_mul(rows);
            let Some(entry) = entries.get(index) else {
                break;
            };
            let marker = if entry.allowed { "●" } else { "○" };
            let value = format!("{marker} {}", entry.program);
            spans.push(Span::styled(
                format!("{value:<column_width$}"),
                Style::default()
                    .fg(if entry.allowed { SUCCESS } else { WARNING })
                    .add_modifier(if entry.allowed {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("● ", Style::default().fg(SUCCESS)),
        Span::styled(language.tr("enabled"), Style::default().fg(TEXT_MUTED)),
        Span::styled("    ○ ", Style::default().fg(WARNING)),
        Span::styled(
            language.tr("approval required"),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            language.tr("Executable access"),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ≠ ", Style::default().fg(TEXT_DIM)),
        Span::styled(
            language.tr("Exact repository operation"),
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", language.tr("risky arguments need exact approval")),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "↑/↓  PgUp/PgDn  ·  {}–{} / {}",
            start.saturating_add(1).min(entries.len()),
            end,
            entries.len()
        ),
        Style::default().fg(TEXT_DIM),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn command_page_size(area: Rect) -> usize {
    let width = area.width.saturating_sub(2).min(112).saturating_sub(4);
    let height = area.height.saturating_sub(2).min(26).saturating_sub(4);
    command_columns(width).saturating_mul(command_rows(height))
}

pub(super) fn command_count(workspaces: &Workspaces, workspace_id: &str) -> usize {
    command_entries(workspaces, workspace_id)
        .map(|(_, entries)| entries.len())
        .unwrap_or(0)
}

fn command_entries(
    workspaces: &Workspaces,
    workspace_id: &str,
) -> anyhow::Result<(String, Vec<CommandEntry>)> {
    let access = workspaces.workspace_access(Some(workspace_id))?;
    let allowed = string_set(&access["allowed_commands"]);
    let mut supported = allowed.iter().cloned().collect::<BTreeSet<_>>();
    supported.extend(string_set(&access["available_commands"]));
    let entries = supported
        .into_iter()
        .map(|program| CommandEntry {
            allowed: allowed.contains(&program),
            program,
        })
        .collect();
    let root = access["root"].as_str().unwrap_or(".").to_owned();
    Ok((root, entries))
}

fn string_set(value: &Value) -> HashSet<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn command_columns(width: u16) -> usize {
    if width >= 90 {
        3
    } else if width >= 58 {
        2
    } else {
        1
    }
}

fn command_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(7).max(1))
}
