use super::*;

pub(super) fn intelligence_url_for_workspace(base: &str, workspace: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(workspace.as_bytes()).collect::<String>();
    let separator = if base.contains('#') { '&' } else { '#' };
    format!("{base}{separator}workspace={encoded}")
}

pub(super) fn pending_authorizations(config: &MonitorConfig) -> Vec<AuthorizationRequest> {
    config
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| request.status == AuthorizationStatus::Pending)
        .collect()
}

pub(super) fn configured_workspaces(config: &MonitorConfig) -> Vec<(String, String, bool)> {
    config
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| {
            let is_default = id == config.workspaces.default_id();
            (id, root.display().to_string(), is_default)
        })
        .collect()
}

pub(super) fn focused_workspace_id(config: &MonitorConfig, focus: usize) -> Option<String> {
    let workspaces = configured_workspaces(config);
    workspaces
        .get(focus.min(workspaces.len().saturating_sub(1)))
        .map(|workspace| workspace.0.clone())
}

pub(super) fn run_dashboard(
    monitor: TaskMonitor,
    config: MonitorConfig,
    stop_rx: watch::Receiver<bool>,
    interrupt_tx: watch::Sender<bool>,
) -> io::Result<()> {
    let mut session = TerminalSession::enter()?;
    let mut tick = 0usize;
    let mut ui = DashboardState::default();

    loop {
        if *stop_rx.borrow() {
            break;
        }

        let size = session.terminal.size()?;
        let workspace_count = config.workspaces.roots().len();
        let visible = workspace_column_count(size.width, workspace_count);
        ui.clamp(workspace_count, visible);
        ui.clamp_authorizations(pending_authorizations(&config).len());
        let snapshot = monitor.snapshot();
        session
            .terminal
            .draw(|frame| draw_dashboard(frame, &snapshot, &config, tick, &ui))?;
        tick = tick.wrapping_add(1);

        let refresh_interval = dashboard_refresh_interval(&snapshot);
        if event::poll(refresh_interval)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                    {
                        let _ = interrupt_tx.send(true);
                        break;
                    }
                    if ui.workspace_input.is_some() {
                        match key.code {
                            KeyCode::Esc => {
                                ui.workspace_input = None;
                                ui.workspace_message =
                                    Some(ui.language.tr("workspace add cancelled").to_owned());
                            }
                            KeyCode::Enter => {
                                let path = ui.workspace_input.take().unwrap_or_default();
                                if path.trim().is_empty() {
                                    ui.workspace_message = Some(
                                        ui.language.tr("workspace path cannot be empty").to_owned(),
                                    );
                                } else {
                                    match config.workspaces.add_workspace(path.trim()) {
                                        Ok((id, root)) => {
                                            monitor.register_workspace(id.clone());
                                            ui.workspace_message = Some(format!(
                                                "authorized workspace {id}: {}",
                                                root.display()
                                            ));
                                            let count = config.workspaces.roots().len();
                                            ui.workspace_focus = count.saturating_sub(1);
                                            ui.clamp(
                                                count,
                                                workspace_column_count(size.width, count),
                                            );
                                        }
                                        Err(error) => {
                                            ui.workspace_message =
                                                Some(format!("workspace rejected: {error}"));
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let Some(input) = ui.workspace_input.as_mut() {
                                    input.pop();
                                }
                            }
                            KeyCode::Char(character)
                                if !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                if let Some(input) = ui.workspace_input.as_mut() {
                                    if input.chars().count() < 1024 {
                                        input.push(character);
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                            ui.workspace_message = None;
                        }
                        KeyCode::Char('?') if key.kind == KeyEventKind::Press => {
                            ui.help_open = true;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                        }
                        KeyCode::Char('i') | KeyCode::Char('I')
                            if key.kind == KeyEventKind::Press =>
                        {
                            ui.intelligence_open = !ui.intelligence_open;
                            ui.help_open = false;
                            ui.commands_open = false;
                            if ui.intelligence_open {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, ui.workspace_focus)
                                {
                                    request_intelligence_refresh(&monitor, &config, workspace_id);
                                }
                            }
                        }
                        KeyCode::Char('r') | KeyCode::Char('R')
                            if key.kind == KeyEventKind::Press && ui.intelligence_open =>
                        {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, ui.workspace_focus)
                            {
                                request_intelligence_refresh(&monitor, &config, workspace_id);
                            }
                        }
                        KeyCode::Char('c') | KeyCode::Char('C')
                            if key.kind == KeyEventKind::Press =>
                        {
                            ui.commands_open = !ui.commands_open;
                            ui.command_offset = 0;
                            ui.help_open = false;
                            ui.intelligence_open = false;
                        }
                        KeyCode::Char('l') | KeyCode::Char('L')
                            if key.kind == KeyEventKind::Press =>
                        {
                            ui.language = ui.language.toggle();
                            ui.workspace_message = Some(format!(
                                "{}: {}",
                                ui.language.tr("LANGUAGE"),
                                ui.language.name()
                            ));
                        }
                        KeyCode::Char('o') | KeyCode::Char('O')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.setup_url());
                        }
                        KeyCode::Char('w') | KeyCode::Char('W')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let workspaces = configured_workspaces(&config);
                            let url = workspaces
                                .get(ui.workspace_focus.min(workspaces.len().saturating_sub(1)))
                                .map(|workspace| {
                                    intelligence_url_for_workspace(
                                        &config.intelligence_url,
                                        &workspace.0,
                                    )
                                })
                                .unwrap_or_else(|| config.intelligence_url.clone());
                            let _ = open_external_url(&url);
                        }
                        KeyCode::Char('g') | KeyCode::Char('G')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.project_url);
                        }
                        KeyCode::Char('a') | KeyCode::Char('A')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.author_url);
                        }
                        KeyCode::Char('+') if key.kind == KeyEventKind::Press => {
                            ui.workspace_input = Some(String::new());
                            ui.workspace_message = None;
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let pending = pending_authorizations(&config);
                            if let Some(request) = pending.get(ui.authorization_focus) {
                                let approved =
                                    config.workspaces.approve_authorization_session(&request.id);
                                ui.workspace_message = Some(if approved {
                                    format!(
                                        "{} {} · {}",
                                        ui.language.tr("approved"),
                                        request.id,
                                        ui.language.tr("retry the tool")
                                    )
                                } else {
                                    format!(
                                        "{} {}",
                                        request.id,
                                        ui.language.tr("authorization is no longer pending")
                                    )
                                });
                                ui.clamp_authorizations(pending_authorizations(&config).len());
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Char('N')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let pending = pending_authorizations(&config);
                            if let Some(request) = pending.get(ui.authorization_focus) {
                                let denied = config.workspaces.deny_authorization(&request.id);
                                ui.workspace_message = Some(if denied {
                                    format!("{} {}", ui.language.tr("denied"), request.id)
                                } else {
                                    format!(
                                        "{} {}",
                                        request.id,
                                        ui.language.tr("authorization is no longer pending")
                                    )
                                });
                                ui.clamp_authorizations(pending_authorizations(&config).len());
                            }
                        }
                        KeyCode::Up if key.kind == KeyEventKind::Press && ui.commands_open => {
                            ui.command_offset = ui.command_offset.saturating_sub(1);
                        }
                        KeyCode::Down if key.kind == KeyEventKind::Press && ui.commands_open => {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, ui.workspace_focus)
                            {
                                let page =
                                    command_page_size(Rect::new(0, 0, size.width, size.height));
                                let total = command_count(&config.workspaces, &workspace_id);
                                ui.command_offset = ui
                                    .command_offset
                                    .saturating_add(1)
                                    .min(total.saturating_sub(page));
                            }
                        }
                        KeyCode::PageUp if key.kind == KeyEventKind::Press && ui.commands_open => {
                            let page = command_page_size(Rect::new(0, 0, size.width, size.height));
                            ui.command_offset = ui.command_offset.saturating_sub(page);
                        }
                        KeyCode::PageDown
                            if key.kind == KeyEventKind::Press && ui.commands_open =>
                        {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, ui.workspace_focus)
                            {
                                let page =
                                    command_page_size(Rect::new(0, 0, size.width, size.height));
                                let total = command_count(&config.workspaces, &workspace_id);
                                ui.command_offset = ui
                                    .command_offset
                                    .saturating_add(page)
                                    .min(total.saturating_sub(page));
                            }
                        }
                        KeyCode::Up
                            if key.kind == KeyEventKind::Press
                                && !ui.help_open
                                && !ui.intelligence_open
                                && !ui.commands_open =>
                        {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus = ui.authorization_focus.saturating_sub(1);
                            }
                        }
                        KeyCode::Down
                            if key.kind == KeyEventKind::Press
                                && !ui.help_open
                                && !ui.intelligence_open
                                && !ui.commands_open =>
                        {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus =
                                    ui.authorization_focus.saturating_add(1).min(total - 1);
                            }
                        }
                        KeyCode::Left => {
                            let previous = ui.workspace_focus;
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            ui.workspace_focus = ui.workspace_focus.saturating_sub(step);
                            ui.command_offset = 0;
                            ui.clamp(config.workspaces.roots().len(), visible);
                            if ui.intelligence_open && ui.workspace_focus != previous {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, ui.workspace_focus)
                                {
                                    request_intelligence_refresh(&monitor, &config, workspace_id);
                                }
                            }
                        }
                        KeyCode::Right => {
                            let previous = ui.workspace_focus;
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            let count = config.workspaces.roots().len();
                            ui.workspace_focus = ui
                                .workspace_focus
                                .saturating_add(step)
                                .min(count.saturating_sub(1));
                            ui.command_offset = 0;
                            ui.clamp(count, visible);
                            if ui.intelligence_open && ui.workspace_focus != previous {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, ui.workspace_focus)
                                {
                                    request_intelligence_refresh(&monitor, &config, workspace_id);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(url) =
                        dashboard_link_at(&mouse, size.width, size.height, &ui, &config)
                    {
                        let _ = open_external_url(&url);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

pub(super) fn dashboard_link_at(
    mouse: &MouseEvent,
    width: u16,
    height: u16,
    ui: &DashboardState,
    config: &MonitorConfig,
) -> Option<String> {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) || width == 0 || height == 0 {
        return None;
    }
    let point = (mouse.column, mouse.row);
    let area = Rect::new(0, 0, width, height);

    if ui.help_open {
        if let Some(url) = help_link_at(point, area, config) {
            return Some(url);
        }
    }

    if width >= 124 && mouse.row >= height.saturating_sub(2) {
        let links_row = Rect::new(0, height.saturating_sub(2), width, 1);
        let controls_row = Rect::new(0, height.saturating_sub(1), width, 1);
        let project = config
            .project_url
            .strip_prefix("https://")
            .unwrap_or(&config.project_url)
            .trim_end_matches('/');
        let project_x = links_row.x.saturating_add("  wcode  ".len() as u16);
        let project_rect = Rect::new(project_x, links_row.y, project.len() as u16, 1);
        if point_in_rect(point, project_rect) {
            return Some(config.project_url.clone());
        }
        let author_x = project_rect
            .x
            .saturating_add(project_rect.width)
            .saturating_add("  by  ".len() as u16);
        let author_rect = Rect::new(author_x, links_row.y, config.author_handle.len() as u16, 1);
        if point_in_rect(point, author_rect) {
            return Some(config.author_url.clone());
        }

        let pending_authorizations = config
            .workspaces
            .authorization_requests(256)
            .iter()
            .filter(|request| request.status == AuthorizationStatus::Pending)
            .count();
        let key_width = |key: &str| key.chars().count() as u16 + 2;
        let label_width = |label: &str| label.chars().count() as u16 + 3;
        let pending_width = if pending_authorizations > 0 {
            pending_authorizations.to_string().chars().count() as u16 + 2
        } else {
            1
        };
        let shortcuts_width = key_width("←/→")
            + label_width(ui.language.tr("workspace"))
            + key_width("O")
            + label_width(ui.language.tr("setup"))
            + key_width("W")
            + label_width(ui.language.tr("web"))
            + key_width("I")
            + 1
            + key_width("C")
            + 1
            + key_width("L")
            + label_width(ui.language.name())
            + key_width("+")
            + 1
            + key_width("Y/N")
            + pending_width
            + key_width("?")
            + 1
            + key_width("^C");
        let shortcuts_x = controls_row
            .x
            .saturating_add(controls_row.width.saturating_sub(shortcuts_width));
        let setup_prefix_width = key_width("←/→") + label_width(ui.language.tr("workspace"));
        let setup_x = shortcuts_x.saturating_add(setup_prefix_width);
        let setup_rect = Rect::new(
            setup_x,
            controls_row.y,
            key_width("O") + label_width(ui.language.tr("setup")),
            1,
        );
        if point_in_rect(point, setup_rect) {
            return Some(config.setup_url());
        }
    }
    None
}

fn point_in_rect((x, y): (u16, u16), rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(super) fn dashboard_refresh_interval(snapshot: &MonitorSnapshot) -> Duration {
    if snapshot
        .tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Queued | TaskStatus::Running))
    {
        ACTIVE_REFRESH_INTERVAL
    } else {
        IDLE_REFRESH_INTERVAL
    }
}
