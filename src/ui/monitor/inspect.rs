use super::*;
use crate::workspace::Workspace;

const INSPECT_PAGE_LINES: usize = 240;
const INSPECT_SYMBOL_LIMIT: usize = 128;
const INSPECT_TARGET_LIMIT: usize = 16;
static INSPECT_INFLIGHT: AtomicBool = AtomicBool::new(false);

struct InspectWorker;
impl Drop for InspectWorker {
    fn drop(&mut self) {
        INSPECT_INFLIGHT.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct InspectTarget {
    path: String,
    symbol: Option<String>,
    pub(super) candidate: bool,
}

/// Targets identify source only; they do not attest that its current bytes passed.
pub(super) fn inspect_target(value: &str) -> Option<InspectTarget> {
    let value = value.strip_prefix("file:").unwrap_or(value);
    let (path, symbol) = value
        .split_once("::")
        .map_or((value, None), |(path, symbol)| (path, Some(symbol)));
    if path.is_empty()
        || path.len() > 1024
        || path
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
        || !std::path::Path::new(path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        || symbol.is_some_and(|name| {
            name.is_empty() || name.len() > 256 || name.chars().any(char::is_control)
        })
    {
        return None;
    }
    Some(InspectTarget {
        path: path.to_owned(),
        symbol: symbol.map(str::to_owned),
        candidate: false,
    })
}

#[derive(Clone, Debug)]
struct InspectSymbol {
    name: String,
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct InspectPage {
    path: String,
    sha256: String,
    content: String,
    start: usize,
    end: usize,
    total: usize,
    redacted: bool,
    symbols: Vec<InspectSymbol>,
    total_symbols: usize,
    outline_notice: String,
    selected_symbol: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InspectRequest {
    workspace: String,
    target: InspectTarget,
    start: usize,
    expected_sha256: Option<String>,
}

pub(super) struct SourceInspection {
    workspace: String,
    record_digest: String,
    targets: Vec<InspectTarget>,
    target_index: usize,
    page: Option<InspectPage>,
    selected_symbol: usize,
    scroll: usize,
    viewport: std::cell::Cell<usize>,
    request: Option<InspectRequest>,
    pending: Option<std::sync::mpsc::Receiver<Result<InspectPage, String>>>,
    error: Option<String>,
}

impl SourceInspection {
    fn queue(&mut self, start: usize, expected_sha256: Option<String>) {
        if self.pending.is_some() || self.request.is_some() {
            return;
        }
        if let Some(target) = self.targets.get(self.target_index).cloned() {
            self.request = Some(InspectRequest {
                workspace: self.workspace.clone(),
                target,
                start,
                expected_sha256,
            });
            self.error = None;
            self.scroll = 0;
        }
    }
}

fn symbol_rows(outline: &Value, total: usize) -> Vec<InspectSymbol> {
    outline["symbols"]
        .as_array()
        .into_iter()
        .flatten()
        .take(INSPECT_SYMBOL_LIMIT)
        .filter_map(|symbol| {
            let start = usize::try_from(symbol["range"]["start_line"].as_u64()?).ok()?;
            let end = usize::try_from(symbol["range"]["end_line"].as_u64()?).ok()?;
            if start == 0 || start > total || end < start {
                return None;
            }
            let name = symbol["qualified_name"]
                .as_str()
                .or_else(|| symbol["name"].as_str())?;
            Some(InspectSymbol {
                name: console_clean(name),
                start,
                end: end.min(total),
            })
        })
        .collect()
}

fn read_inspect_page(
    harness: &ToolHarness,
    workspace: &Workspace,
    request: &InspectRequest,
) -> anyhow::Result<InspectPage> {
    let before = workspace.source_stamp(&request.target.path)?;
    // file_outline is syntax-only and performs no command or LSP launch.
    let outline = harness.file_outline(
        request.workspace.as_str(),
        workspace,
        &request.target.path,
        INSPECT_SYMBOL_LIMIT,
    );
    let initial = workspace.read_file(
        &request.target.path,
        request.start.max(1),
        Some(request.start.max(1).saturating_add(INSPECT_PAGE_LINES - 1)),
    )?;
    if request
        .expected_sha256
        .as_ref()
        .is_some_and(|sha| sha != &initial.sha256)
    {
        anyhow::bail!(
            "Source changed since inspection; R reload before following another page or symbol"
        );
    }
    let (symbols, total_symbols, mut notice) = match outline {
        Ok(outline) => {
            if outline["workspace"] != request.workspace
                || outline["path"] != initial.path
                || outline["sha256"] != initial.sha256
                || outline["provider"] != "tree-sitter"
                || outline["precision"] != "syntax"
            {
                anyhow::bail!("Source outline identity changed; refresh inspection");
            }
            let symbols = symbol_rows(&outline, initial.total_lines);
            let total = outline["total_symbols"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(symbols.len());
            let partial = outline["truncated"].as_bool() == Some(true) || total > symbols.len();
            let notice = if outline["parse_errors"].as_bool() == Some(true) {
                "Syntax parse errors · symbol coverage partial"
            } else if partial {
                "Syntax symbol list truncated"
            } else {
                "tree-sitter · syntax · not semantic or execution proof"
            };
            (symbols, total, notice.to_owned())
        }
        Err(error) => (
            Vec::new(),
            0,
            format!(
                "Symbol outline unavailable: {}",
                console_clean(&error.to_string())
            ),
        ),
    };
    let mut selected_symbol = 0;
    let mut page = initial;
    if let Some(wanted) = request
        .target
        .symbol
        .as_ref()
        .filter(|_| request.start == 0)
    {
        let candidates = symbols
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.name == *wanted || item.name.rsplit("::").next() == Some(wanted.as_str())
            })
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            let (index, symbol) = candidates[0];
            selected_symbol = index;
            let selected = workspace.read_file(
                &request.target.path,
                symbol.start,
                Some(symbol.start.saturating_add(INSPECT_PAGE_LINES - 1)),
            )?;
            if selected.sha256 != page.sha256 {
                anyhow::bail!("Source changed while following symbol; refresh inspection");
            }
            page = selected;
        } else {
            selected_symbol = usize::MAX;
            notice.push_str(" · requested symbol unavailable or ambiguous; no substitute selected");
        }
    }
    if workspace.source_stamp(&request.target.path)? != before {
        anyhow::bail!("Source changed during inspection; refresh before using ranges");
    }
    Ok(InspectPage {
        path: page.path,
        sha256: page.sha256,
        content: page.content,
        start: page.start_line,
        end: page.end_line,
        total: page.total_lines,
        redacted: page.redacted,
        symbols,
        total_symbols,
        outline_notice: notice,
        selected_symbol,
    })
}

pub(super) fn open_source_inspection(
    ui: &mut DashboardState,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) {
    let Some(workspace) = console_workspace(ui, snapshot, config) else {
        return;
    };
    let Some(stats) = snapshot.intelligence.get(&workspace) else {
        return;
    };
    let Some(record) = acceptance_view(stats, &workspace).record else {
        return;
    };
    let targets = acceptance_inspect_targets(stats, &workspace, ui.console_focus)
        .into_iter()
        .take(INSPECT_TARGET_LIMIT)
        .collect::<Vec<_>>();
    let mut inspection = SourceInspection {
        workspace,
        record_digest: record["record_digest"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        targets,
        target_index: 0,
        page: None,
        selected_symbol: 0,
        scroll: 0,
        viewport: std::cell::Cell::new(1),
        request: None,
        pending: None,
        error: None,
    };
    if inspection.targets.is_empty() {
        inspection.error = Some(
            "No file target in exact linked Evidence · unknown is not an empty repository".into(),
        );
    } else {
        inspection.queue(0, None);
    }
    ui.source_inspection = Some(inspection);
}

pub(super) fn refresh_source_inspection(
    ui: &mut DashboardState,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) {
    let Some(inspection) = ui.source_inspection.as_ref() else {
        return;
    };
    if !ui.intelligence_open
        || ui.console_tab != ConsoleTab::Acceptance
        || console_workspace(ui, snapshot, config).as_deref() != Some(&inspection.workspace)
    {
        ui.source_inspection = None;
        return;
    }
    let current_digest = snapshot
        .intelligence
        .get(&inspection.workspace)
        .and_then(|stats| acceptance_view(stats, &inspection.workspace).record)
        .and_then(|record| record["record_digest"].as_str());
    let Some(inspection) = ui.source_inspection.as_mut() else {
        return;
    };
    if current_digest != Some(inspection.record_digest.as_str()) {
        inspection.pending = None;
        inspection.request = None;
        inspection.page = None;
        inspection.error =
            Some("Acceptance changed · Esc back and reopen its exact Evidence target".into());
        return;
    }
    if let Some(receiver) = inspection.pending.as_ref() {
        match receiver.try_recv() {
            Ok(result) => {
                inspection.pending = None;
                match result {
                    Ok(page) => {
                        inspection.selected_symbol = page.selected_symbol;
                        inspection.page = Some(page);
                        inspection.error = None;
                    }
                    Err(error) => {
                        inspection.page = None;
                        inspection.error = Some(console_clean(&error));
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                inspection.pending = None;
                inspection.page = None;
                inspection.error = Some("Source observer disconnected · state unknown".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
    let Some(request) = inspection.request.take() else {
        return;
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        inspection.error = Some("Source observer runtime unavailable · state unknown".into());
        return;
    };
    if INSPECT_INFLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        inspection.request = Some(request);
        return;
    }
    let worker = InspectWorker;
    let workspaces = config.workspaces.clone();
    let harness = config.harness.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    inspection.pending = Some(receiver);
    runtime.spawn_blocking(move || {
        let _worker = worker;
        let result = workspaces
            .select(Some(&request.workspace))
            .and_then(|(id, workspace)| {
                if id != request.workspace {
                    anyhow::bail!("Source workspace identity mismatch");
                }
                read_inspect_page(&harness, &workspace, &request)
            })
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
}

pub(super) fn handle_source_inspection_key(key: event::KeyEvent, ui: &mut DashboardState) -> bool {
    let Some(inspection) = ui.source_inspection.as_mut() else {
        return false;
    };
    if key.kind != KeyEventKind::Press {
        return true;
    }
    let viewport = inspection.viewport.get().max(1);
    if (inspection.pending.is_some() || inspection.request.is_some())
        && matches!(
            key.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Enter | KeyCode::Char('r' | 'R')
        )
    {
        return true;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('f' | 'F') => ui.source_inspection = None,
        KeyCode::Left | KeyCode::Right if !inspection.targets.is_empty() => {
            let index = if key.code == KeyCode::Left {
                inspection.target_index.saturating_sub(1)
            } else {
                (inspection.target_index + 1).min(inspection.targets.len() - 1)
            };
            if index != inspection.target_index {
                inspection.target_index = index;
                inspection.pending = None;
                inspection.request = None;
                inspection.page = None;
                inspection.selected_symbol = 0;
                inspection.queue(0, None);
            }
        }
        KeyCode::Char('r' | 'R') => {
            // Drop the old response channel; it can never replace a new request.
            inspection.pending = None;
            inspection.request = None;
            inspection.page = None;
            inspection.queue(0, None);
        }
        KeyCode::Up | KeyCode::Down => {
            if let Some(page) = &inspection.page {
                let total = page.symbols.len();
                inspection.selected_symbol = if inspection.selected_symbol >= total {
                    0
                } else if key.code == KeyCode::Up {
                    inspection.selected_symbol.saturating_sub(1)
                } else {
                    inspection
                        .selected_symbol
                        .saturating_add(1)
                        .min(total.saturating_sub(1))
                };
            }
        }
        KeyCode::Enter => {
            if let Some((start, sha)) = inspection.page.as_ref().and_then(|page| {
                page.symbols
                    .get(inspection.selected_symbol)
                    .map(|symbol| (symbol.start, page.sha256.clone()))
            }) {
                inspection.queue(start, Some(sha));
            }
        }
        KeyCode::Home => inspection.scroll = 0,
        KeyCode::End => {
            inspection.scroll = inspection.page.as_ref().map_or(0, |page| {
                page.content.lines().count().saturating_sub(viewport)
            });
        }
        KeyCode::PageUp | KeyCode::PageDown => {
            if let Some(page) = &inspection.page {
                let max_scroll = page.content.lines().count().saturating_sub(viewport);
                if key.code == KeyCode::PageDown
                    && inspection.scroll >= max_scroll
                    && page.end < page.total
                {
                    inspection.queue(page.end + 1, Some(page.sha256.clone()));
                } else if key.code == KeyCode::PageUp && inspection.scroll == 0 && page.start > 1 {
                    inspection.queue(
                        page.start.saturating_sub(INSPECT_PAGE_LINES).max(1),
                        Some(page.sha256.clone()),
                    );
                } else if key.code == KeyCode::PageUp {
                    inspection.scroll = inspection.scroll.saturating_sub(viewport);
                } else {
                    inspection.scroll = inspection.scroll.saturating_add(viewport).min(max_scroll);
                }
            }
        }
        // Tab/number/W retain the existing console/browser navigation.
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('1'..='6' | 'w' | 'W' | 'i' | 'I') => {
            return false
        }
        _ => {}
    }
    true
}

pub(super) fn render_source_inspection(
    frame: &mut Frame<'_>,
    area: Rect,
    inspection: &SourceInspection,
) {
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "INSPECT · {} · file target {} / {}",
                console_clean(&inspection.workspace),
                (inspection.target_index + 1).min(inspection.targets.len()),
                inspection.targets.len()
            ),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::raw("Current protected source · read-only · not historical Evidence or approval"),
    ];
    if let Some(target) = inspection.targets.get(inspection.target_index) {
        lines.push(Line::raw(if target.candidate {
            "Candidate changed file · not an Evidence association · up to 16 targets"
        } else {
            "Exact linked Evidence target · current source may differ · up to 16 targets"
        }));
        lines.push(Line::raw(format!(
            "{}{}",
            console_clean(&target.path),
            target
                .symbol
                .as_ref()
                .map(|name| format!("::{}", console_clean(name)))
                .unwrap_or_default()
        )));
    }
    if let Some(error) = &inspection.error {
        lines.push(Line::from(Span::styled(
            console_clean(error),
            Style::default().fg(WARNING),
        )));
    } else if inspection.pending.is_some() || inspection.request.is_some() {
        lines.push(Line::raw("Loading protected source · state unknown"));
    } else if let Some(page) = &inspection.page {
        lines.push(Line::raw(format!(
            "{} · lines {}–{} / {} · SHA {}{}",
            console_clean(&page.path),
            page.start,
            page.end,
            page.total,
            page.sha256,
            if page.redacted { " · redacted" } else { "" }
        )));
        lines.push(Line::raw(console_clean(&page.outline_notice)));
        if let Some(symbol) = page.symbols.get(inspection.selected_symbol) {
            lines.push(Line::raw(format!(
                "Symbol {} / {} ({} observed) · {} · {}–{} · Enter jump",
                inspection.selected_symbol + 1,
                page.symbols.len(),
                page.total_symbols,
                symbol.name,
                symbol.start,
                symbol.end
            )));
        } else {
            lines.push(Line::raw("No symbol selected · source remains inspectable"));
        }
        let viewport = usize::from(area.height.saturating_sub(8).max(1));
        inspection.viewport.set(viewport);
        for (index, line) in page
            .content
            .lines()
            .enumerate()
            .skip(inspection.scroll)
            .take(viewport)
        {
            lines.push(Line::raw(format!(
                "{:>5} {}",
                page.start + index,
                console_clean(line)
            )));
        }
    }
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines), body);
    if area.height > 0 {
        frame.render_widget(Paragraph::new(
            "←→ target · ↑↓ symbol · Enter jump · PgUp/PgDn source · Home/End · R reload · Esc back"
        ).style(Style::default().fg(TEXT_DIM)),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1));
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/inspect.rs"]
mod tests;
