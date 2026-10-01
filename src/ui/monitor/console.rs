use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ConsoleTab {
    #[default]
    Acceptance,
    Attention,
    Tasks,
    Providers,
    Agents,
    Summary,
}

impl ConsoleTab {
    fn index(self) -> usize {
        match self {
            Self::Acceptance => 0,
            Self::Summary => 5,
            Self::Attention => 1,
            Self::Tasks => 2,
            Self::Providers => 3,
            Self::Agents => 4,
        }
    }

    fn from_index(index: usize) -> Self {
        [
            Self::Acceptance,
            Self::Attention,
            Self::Tasks,
            Self::Providers,
            Self::Agents,
            Self::Summary,
        ][index % 6]
    }
}

struct AttentionSignal {
    severity: String,
    subject: String,
    message: String,
    provenance: String,
    action: String,
    target: ConsoleTab,
    task_id: Option<u64>,
    authorization: bool,
}

pub(super) fn render_console_tabs(frame: &mut Frame<'_>, area: Rect, ui: &DashboardState) {
    let labels = if area.width >= 80 {
        [
            "Acceptance",
            "Attention",
            "Tasks",
            "Providers",
            "Agents",
            "Observations",
        ]
    } else if area.width >= 61 {
        ["Accept", "Issues", "Tasks", "LSP/AI", "Agents", "Observe"]
    } else {
        ["AC", "!", "T", "P", "A", "O"]
    };
    let spans = labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            Span::styled(
                format!(" {} {} ", index + 1, ui.language.tr(label)),
                if ui.console_tab.index() == index {
                    Style::default()
                        .fg(BACKGROUND)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_MUTED)
                },
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    if area.height > 1 {
        let label = [
            "Acceptance",
            "Attention",
            "Tasks",
            "Providers",
            "Agents",
            "Observations",
        ][ui.console_tab.index()];
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{} · Tab / Shift-Tab · 1-6", ui.language.tr(label)),
                Style::default().fg(TEXT_DIM),
            )),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
    }
}

pub(super) fn console_workspace(
    ui: &DashboardState,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) -> Option<String> {
    ui.workspace_focus_id
        .clone()
        .or_else(|| focused_workspace_id(config, snapshot, ui.workspace_focus))
}

pub(super) fn console_tasks<'a>(
    snapshot: &'a MonitorSnapshot,
    workspace: &str,
) -> Vec<&'a TaskRecord> {
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.workspace == workspace)
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| {
        (
            match task.status {
                TaskStatus::Running => 0,
                TaskStatus::Queued => 1,
                TaskStatus::Failed => 2,
                TaskStatus::Completed => 3,
            },
            std::cmp::Reverse(task.id),
        )
    });
    tasks
}

pub(super) fn task_selection(ui: &DashboardState, tasks: &[&TaskRecord]) -> usize {
    ui.console_task_id
        .and_then(|id| tasks.iter().position(|task| task.id == id))
        .unwrap_or(ui.console_focus)
        .min(tasks.len().saturating_sub(1))
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

fn severity_tone(severity: &str) -> Color {
    match severity {
        "critical" | "high" => DANGER,
        "medium" => WARNING,
        "low" => SECONDARY,
        _ => TEXT_MUTED,
    }
}

fn json_text<'a>(value: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    value[key].as_str().unwrap_or(fallback)
}

fn attention_signals(
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
) -> Vec<AttentionSignal> {
    let mut signals = Vec::new();
    let mut local = |severity: &str,
                     subject: &str,
                     message: String,
                     action: &str,
                     target,
                     task_id,
                     authorization| {
        signals.push(AttentionSignal {
            severity: severity.to_owned(),
            subject: subject.to_owned(),
            message,
            provenance: "runtime monitor · observed".to_owned(),
            action: action.to_owned(),
            target,
            task_id,
            authorization,
        });
    };
    let approvals = ui
        .pending_authorizations
        .iter()
        .filter(|request| request.workspace == workspace)
        .count();
    if approvals > 0 {
        local(
            "high",
            "Authorization pending",
            format!("{approvals} operation(s) await an operator decision."),
            "Enter opens the authorization surface; inspect the selected operation.",
            ConsoleTab::Summary,
            None,
            true,
        );
    }
    if snapshot.tunnel_running == Some(false) {
        local(
            "high",
            "Tunnel process exited",
            snapshot
                .tunnel_error
                .clone()
                .unwrap_or_else(|| "The provider process is no longer running.".to_owned()),
            "Inspect provider recovery and health probes.",
            ConsoleTab::Providers,
            None,
            false,
        );
    } else if snapshot.public_url_healthy == Some(false) {
        local(
            "high",
            "Public endpoint unavailable",
            snapshot
                .public_url_error
                .clone()
                .unwrap_or_else(|| "No verified public endpoint is available.".to_owned()),
            "Inspect provider recovery and health probes.",
            ConsoleTab::Providers,
            None,
            false,
        );
    }
    for task in console_tasks(snapshot, workspace)
        .into_iter()
        .filter(|task| task.status == TaskStatus::Failed)
    {
        local(
            "high",
            "Tool call failed",
            format!(
                "#{} {} · {}",
                task.id,
                task.tool,
                truncate_end(&task.detail, 240)
            ),
            "Enter opens retained task details; inspect the tool result before retrying.",
            ConsoleTab::Tasks,
            Some(task.id),
            false,
        );
    }
    if let Some(stats) = snapshot.intelligence.get(workspace) {
        if let Some(error) = stats.refresh_error.as_deref() {
            local(
                "medium",
                "Partial refresh",
                truncate_end(error, 400),
                "R refreshes the current workspace observation.",
                ConsoleTab::Summary,
                None,
                false,
            );
        }
        if stats
            .project_observed_at
            .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300))
        {
            local("medium", "Project snapshot aging", "The project observation is older than five minutes; its revision may differ from the working tree.".to_owned(),
                "R refreshes the current workspace observation.", ConsoleTab::Summary, None, false);
        }
        if stats.lsp_stale > 0 {
            local("medium", "Semantic evidence stale", format!("{} stale provider snapshot(s); syntax facts retain syntax precision.", stats.lsp_stale),
                "Inspect provider freshness; semantic_status and semantic_navigation expose current provenance.", ConsoleTab::Providers, None, false);
        }
        // The shared project projection owns proof/trace/drift findings. Scalar
        // fallback is only used before that bounded snapshot becomes available.
        if stats.project_attention.is_none() {
            if stats.evidence_failed > 0 || stats.evidence_disagreed > 0 {
                local("high", "Proof needs attention", format!("{} failed · {} disagreed evidence item(s).", stats.evidence_failed, stats.evidence_disagreed),
                    "evidence_status shows producer and revision; review_changes then verify_project refreshes checks.", ConsoleTab::Summary, None, false);
            }
            if stats.verification_ready == Some(false) {
                local(
                    "medium",
                    "Verification blocked",
                    format!(
                        "{} blocker(s) in the latest observed verification plan.",
                        stats.verification_blockers
                    ),
                    "verification_status exposes plan blockers and revision.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
            if stats.policy_errors > 0 {
                local(
                    "high",
                    "Policy findings",
                    format!(
                        "{} error(s) · {} warning(s).",
                        stats.policy_errors, stats.policy_warnings
                    ),
                    "convention_status exposes exact policy locations.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
            if stats.drift_findings > 0 {
                local(
                    "medium",
                    "Design drift",
                    format!(
                        "{} observed finding(s) · risk {}.",
                        stats.drift_findings,
                        stats.risk_level.as_deref().unwrap_or("unassessed")
                    ),
                    "drift_status and risk_status expose affected subjects.",
                    ConsoleTab::Summary,
                    None,
                    false,
                );
            }
        }
        if let Some(items) = stats
            .project_attention
            .as_ref()
            .and_then(|attention| attention["items"].as_array())
        {
            for issue in items.iter().take(200) {
                let section = json_text(issue, "section", "overview");
                let subject = json_text(issue, "subject", "project");
                let location = issue["path"]
                    .as_str()
                    .or_else(|| issue["requirement"].as_str());
                signals.push(AttentionSignal {
                    severity: json_text(issue, "severity", "info").to_owned(),
                    subject: subject.to_owned(),
                    message: format!(
                        "{}{}",
                        json_text(issue, "message", "Observed project finding"),
                        location
                            .map(|location| format!(" · {location}"))
                            .unwrap_or_default()
                    ),
                    provenance: format!(
                        "{} · {} · cached project snapshot",
                        json_text(issue, "provider", "project_observatory"),
                        json_text(issue, "precision", "unknown")
                    ),
                    action: format!("W opens the workspace observatory; inspect {section}."),
                    target: ConsoleTab::Summary,
                    task_id: None,
                    authorization: false,
                });
            }
        }
    }
    signals.sort_by_key(|signal| severity_rank(&signal.severity));
    signals
}

pub(super) fn handle_console_key(
    key: event::KeyEvent,
    ui: &mut DashboardState,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) -> bool {
    if !ui.intelligence_open
        || ui.help_open
        || ui.commands_open
        || ui.full_access_confirm
        || ui.workspace_input.is_some()
        || area.width < 40
        || area.height < 16
        || key.kind == KeyEventKind::Release
        || !key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
    {
        return false;
    }
    if handle_source_inspection_key(key, ui) {
        return true;
    }
    if key.kind == KeyEventKind::Press
        && matches!(key.code, KeyCode::Char('f' | 'F'))
        && ui.console_tab == ConsoleTab::Acceptance
    {
        open_source_inspection(ui, snapshot, config);
        return true;
    }
    let press = key.kind == KeyEventKind::Press;
    let tab = match key.code {
        KeyCode::Tab if press => Some(ConsoleTab::from_index(ui.console_tab.index() + 1)),
        KeyCode::BackTab if press => Some(ConsoleTab::from_index(ui.console_tab.index() + 5)),
        KeyCode::Char('1'..='6') if press => {
            if let KeyCode::Char(digit) = key.code {
                Some(ConsoleTab::from_index(digit as usize - '1' as usize))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(tab) = tab {
        ui.console_tab = tab;
        ui.console_focus = 0;
        ui.console_task_id = None;
        ui.console_scroll = 0;
        ui.acceptance_detail_open = false;
        ui.source_inspection = None;
        return true;
    }
    let Some(workspace) = console_workspace(ui, snapshot, config) else {
        return false;
    };
    let signals = attention_signals(snapshot, ui, &workspace);
    let tasks = console_tasks(snapshot, &workspace);
    let selected = if ui.console_tab == ConsoleTab::Tasks {
        task_selection(ui, &tasks)
    } else {
        ui.console_focus
    };
    let total = match ui.console_tab {
        ConsoleTab::Acceptance => snapshot
            .intelligence
            .get(&workspace)
            .map_or(0, |stats| acceptance_entry_count(stats, &workspace)),
        ConsoleTab::Tasks => tasks.len(),
        ConsoleTab::Attention => signals.len(),
        ConsoleTab::Agents => snapshot
            .intelligence
            .get(&workspace)
            .and_then(|stats| stats.project_worklist.as_ref())
            .and_then(|status| status["items"].as_array())
            .map_or(0, Vec::len),
        _ => 0,
    };
    match key.code {
        KeyCode::Up | KeyCode::Down
            if matches!(
                ui.console_tab,
                ConsoleTab::Acceptance
                    | ConsoleTab::Attention
                    | ConsoleTab::Tasks
                    | ConsoleTab::Agents
            ) =>
        {
            ui.console_focus = if key.code == KeyCode::Up {
                selected.saturating_sub(1)
            } else {
                selected.saturating_add(1).min(total.saturating_sub(1))
            };
            ui.console_task_id = (ui.console_tab == ConsoleTab::Tasks)
                .then(|| tasks.get(ui.console_focus).map(|task| task.id))
                .flatten();
            ui.console_scroll = 0;
            ui.acceptance_detail_open = false;
        }
        KeyCode::Up | KeyCode::PageUp => {
            ui.console_scroll = ui
                .console_scroll
                .saturating_sub(if key.code == KeyCode::Up { 1 } else { 5 })
        }
        KeyCode::Down | KeyCode::PageDown => {
            ui.console_scroll = ui
                .console_scroll
                .saturating_add(if key.code == KeyCode::Down { 1 } else { 5 })
                .min(4096)
        }
        KeyCode::Enter if press && ui.console_tab == ConsoleTab::Acceptance => {
            ui.acceptance_detail_open = !ui.acceptance_detail_open;
            ui.console_scroll = 0;
        }
        KeyCode::Enter if press && ui.console_tab == ConsoleTab::Attention => {
            if let Some(signal) = signals.get(ui.console_focus.min(signals.len().saturating_sub(1)))
            {
                if signal.authorization {
                    ui.intelligence_open = false;
                    ui.authorization_focus = ui
                        .pending_authorizations
                        .iter()
                        .position(|request| request.workspace == workspace)
                        .unwrap_or(0);
                    ui.authorization_scroll = 0;
                } else {
                    ui.console_tab = signal.target;
                    ui.console_task_id = signal.task_id;
                    ui.console_focus = 0;
                    ui.console_scroll = 0;
                }
            }
        }
        _ => return false,
    }
    true
}

pub(super) fn console_clean(text: &str) -> String {
    crate::workspace::redact_sensitive_text(text)
        .0
        .chars()
        .map(|character| {
            if character.is_control() && character != '\n' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub(super) fn console_paragraph(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: Vec<Line<'static>>,
    scroll: usize,
) {
    let lines = lines
        .into_iter()
        .flat_map(|line| {
            let style = line.style.patch(
                line.spans
                    .first()
                    .map(|span| span.style)
                    .unwrap_or_default(),
            );
            let mut wrapped =
                authorization_detail_lines(&line.to_string(), usize::from(area.width));
            if wrapped.is_empty() {
                wrapped.push(Line::raw(""));
            }
            wrapped.into_iter().map(move |line| line.style(style))
        })
        .collect::<Vec<_>>();
    let maximum = lines.len().saturating_sub(area.height as usize);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(
        paragraph.scroll((scroll.min(maximum).min(u16::MAX as usize) as u16, 0)),
        area,
    );
}

pub(super) fn render_console_tab(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
    stats: &IntelligenceStats,
) {
    if area.height == 0 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    match ui.console_tab {
        ConsoleTab::Acceptance => render_acceptance_console(frame, rows[0], stats, ui, workspace),
        ConsoleTab::Attention => render_attention(frame, rows[0], snapshot, ui, workspace, stats),
        ConsoleTab::Tasks => render_task_console(frame, rows[0], snapshot, ui, workspace),
        ConsoleTab::Providers => {
            render_provider_console(frame, rows[0], snapshot, ui, workspace, stats)
        }
        ConsoleTab::Agents => render_agent_console(frame, rows[0], stats, ui),
        ConsoleTab::Summary => {}
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            ui.language
                .tr("Tab sections · ↑↓ select · PgUp/PgDn details · Enter inspect")
                .to_owned(),
            Style::default().fg(TEXT_DIM),
        )),
        rows[1],
    );
}

fn project_snapshot_line(stats: &IntelligenceStats) -> Line<'static> {
    let Some(attention) = stats.project_attention.as_ref() else {
        return Line::from(Span::styled(
            "Project snapshot unavailable · R refresh",
            Style::default().fg(WARNING),
        ));
    };
    let revision = stats
        .project_revision
        .as_ref()
        .or_else(|| attention.get("revision"));
    let code = revision
        .and_then(|revision| revision["code"].as_str())
        .unwrap_or("unknown");
    Line::from(Span::styled(
        format!(
            "Project snapshot {} · {} · {} issues{}{}",
            last_seen_text(stats.project_observed_at),
            truncate_end(code, 22),
            attention["total"].as_u64().unwrap_or(0),
            if attention["partial"].as_bool() == Some(true) {
                " · partial"
            } else {
                ""
            },
            if attention["truncated"].as_bool() == Some(true) {
                " · truncated"
            } else {
                ""
            },
        ),
        Style::default().fg(TEXT_DIM),
    ))
}

fn render_attention(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
    stats: &IntelligenceStats,
) {
    let signals = attention_signals(snapshot, ui, workspace);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(if area.height >= 10 { 5 } else { 3 }),
        ])
        .split(area);
    frame.render_widget(Paragraph::new(project_snapshot_line(stats)), rows[0]);
    if signals.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    if stats.project_attention.is_some() {
                        "No issues in the observed snapshot."
                    } else {
                        "No retained runtime issues; project state is unknown."
                    },
                    Style::default().fg(TEXT_MUTED),
                )),
                Line::from("R refresh · W inspect the workspace observatory"),
            ]),
            rows[1],
        );
        return;
    }
    let focus = ui.console_focus.min(signals.len() - 1);
    let page = rows[1].height.max(1) as usize;
    let offset = focus.saturating_sub(page - 1);
    let items = signals
        .iter()
        .enumerate()
        .skip(offset)
        .take(page)
        .map(|(index, signal)| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    if index == focus { "▸ " } else { "  " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!("{:<8}", signal.severity.to_ascii_uppercase()),
                    Style::default().fg(severity_tone(&signal.severity)),
                ),
                Span::styled(
                    truncate_end(
                        &format!("{} · {}", signal.subject, console_clean(&signal.message)),
                        rows[1].width.saturating_sub(10) as usize,
                    ),
                    Style::default().fg(if index == focus { TEXT } else { TEXT_MUTED }),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), rows[1]);
    let signal = &signals[focus];
    console_paragraph(
        frame,
        rows[2],
        vec![
            Line::from(Span::styled(
                format!("{} / {} · {}", focus + 1, signals.len(), signal.provenance),
                Style::default().fg(TEXT_DIM),
            )),
            Line::from(console_clean(&signal.message)),
            Line::from(Span::styled(
                signal.action.clone(),
                Style::default().fg(ACCENT),
            )),
        ],
        ui.console_scroll,
    );
}

fn task_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
    }
}

fn render_task_console(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
) {
    let tasks = console_tasks(snapshot, workspace);
    if tasks.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No retained tasks for this workspace. History is bounded to process memory.",
            ),
            area,
        );
        return;
    }
    let focus = task_selection(ui, &tasks);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(if area.height >= 12 { 5 } else { 2 }),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(
                "{} retained · history: bounded process memory · workspace {workspace}",
                tasks.len()
            ),
            Style::default().fg(TEXT_DIM),
        )),
        rows[0],
    );
    let page = rows[1].height.max(1) as usize;
    let offset = focus.saturating_sub(page - 1);
    let items = tasks
        .iter()
        .enumerate()
        .skip(offset)
        .take(page)
        .map(|(index, task)| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    if index == focus { "▸ " } else { "  " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!("#{:<5} {:<10} ", task.id, task_status(task.status)),
                    Style::default().fg(match task.status {
                        TaskStatus::Failed => DANGER,
                        TaskStatus::Running => ACCENT,
                        TaskStatus::Queued => WARNING,
                        _ => SUCCESS,
                    }),
                ),
                Span::styled(
                    truncate_end(&task.tool, rows[1].width.saturating_sub(20) as usize),
                    Style::default().fg(TEXT),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), rows[1]);
    let task = tasks[focus];
    let end = task.finished_at.unwrap_or_else(Instant::now);
    let wait = task
        .started_at
        .unwrap_or(end)
        .saturating_duration_since(task.queued_at)
        .as_millis();
    let run = task
        .started_at
        .map(|start| format!("{}ms", end.saturating_duration_since(start).as_millis()))
        .unwrap_or_else(|| "not started".to_owned());
    let mut details = vec![
        Line::from(Span::styled(
            format!(
                "#{} {} · {} · workspace {}",
                task.id,
                task.tool,
                task_status(task.status),
                task.workspace
            ),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Wait {wait}ms · Run {run} · {}",
            if task.slot_counted {
                "execution slot"
            } else {
                "orchestration"
            }
        )),
        Line::from(format!(
            "Request {}B · Response {}B · Context avoided {}B",
            task.request_bytes, task.response_bytes, task.context_bytes_avoided
        )),
        Line::from(Span::styled("DETAIL", Style::default().fg(TEXT_DIM))),
        Line::from(console_clean(&task.detail)),
    ];
    details.extend(command_job_lines(task, ui));
    console_paragraph(frame, rows[2], details, ui.console_scroll);
}

fn render_provider_console(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    ui: &DashboardState,
    workspace: &str,
    stats: &IntelligenceStats,
) {
    let mut lines = vec![
        Line::from(Span::styled("SEMANTIC PROVIDERS", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
        Line::from(format!("Available {} · launch ready {} · validated {} · authorization required {} · missing {}", stats.lsp_available, stats.lsp_launch_ready, stats.lsp_validated, stats.lsp_authorization_required, stats.lsp_missing)),
        Line::from(format!("Warm sessions {} · documents {} · starts {} · requests {}", stats.lsp_sessions, stats.lsp_documents, stats.lsp_starts, stats.lsp_requests)),
        Line::from(format!("Fresh snapshots {} · stale {} · confirmed facts {} · candidates {}", stats.lsp_fresh, stats.lsp_stale, stats.semantic_confirmed, stats.semantic_candidates)),
        Line::from(format!("Graph precision {} · observed {}", stats.graph_precision.as_deref().unwrap_or("unknown"), last_seen_text(stats.updated_at))),
        Line::from(""),
        Line::from(Span::styled("DECISION RUNTIME", Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD))),
    ];
    if let Some(workspace_stats) = snapshot.workspaces.get(workspace) {
        if let Some(jev) = workspace_stats.jev_latest.as_ref() {
            lines.push(Line::from(format!(
                "Jev {} · model {} · authority {} · observed {}",
                jev.status,
                jev.model.as_deref().unwrap_or("unknown"),
                jev.authority.as_deref().unwrap_or("unknown"),
                last_seen_text(Some(jev.observed_at))
            )));
            lines.push(Line::from(format!(
                "Checkpoint {} · next {} → {}",
                jev.checkpoint,
                jev.baseline_next_action.as_deref().unwrap_or("unknown"),
                jev.candidate_next_action.as_deref().unwrap_or("unknown")
            )));
            lines.push(Line::from(format!(
                "Observed {} · successful {} · degraded {} · disabled {}",
                workspace_stats.jev_observed,
                workspace_stats.jev_successful,
                workspace_stats.jev_degraded,
                workspace_stats.jev_disabled
            )));
            lines.push(Line::from(format!(
                "Last call {}ms · request {}B · response {}B",
                jev.call_elapsed_ms, jev.call_request_bytes, jev.call_response_bytes
            )));
            lines.push(Line::from(format!(
                "Choice disagreements {} · safety violations {} · shape mismatches {}",
                jev.choice_disagreements, jev.safety_policy_violations, jev.shape_mismatches
            )));
        } else {
            lines.push(Line::from("Jev observation unavailable."));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "PUBLIC ENDPOINT PROVIDERS",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
    if snapshot.tunnel_runtime.is_empty() {
        lines.push(Line::from("No provider observations retained."));
    }
    for tunnel in &snapshot.tunnel_runtime {
        lines.push(Line::from(format!(
            "{} · {} · {} · failures {} · deaths {} · retry {}",
            tunnel.provider,
            tunnel.role,
            tunnel.state,
            tunnel.consecutive_failures,
            tunnel.death_count,
            tunnel
                .retry_in_seconds
                .map(|seconds| format!("{seconds}s"))
                .unwrap_or_else(|| "none".to_owned())
        )));
        if let Some(url) = tunnel.url.as_deref() {
            lines.push(Line::from(format!("  {url}")));
        }
    }
    console_paragraph(frame, area, lines, ui.console_scroll);
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/observatory.rs"]
mod tests;
