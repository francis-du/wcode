use super::*;

pub(super) const AUTHOR_SHORTCUT: &str = "B";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DashboardAction {
    Interrupt,
    ConfirmFullAccess,
    CancelFullAccess,
    CancelInput,
    SubmitInput,
    EraseInput,
    TypeInput(char),
    Dismiss,
    Help,
    Intelligence,
    RefreshIntelligence,
    Commands,
    FullAccess,
    Language,
    Setup,
    Observatory,
    Project,
    Author,
    AddWorkspace,
    GrantAllCommands,
    ToggleCommandTrust,
    ShowAuthorization,
    NoPendingAuthorization,
    Approve,
    Deny,
    CommandsUp,
    CommandsDown,
    CommandsPageUp,
    CommandsPageDown,
    AuthorizationUp,
    AuthorizationDown,
    AuthorizationPageUp,
    AuthorizationPageDown,
    WorkspaceLeft,
    WorkspaceRight,
}

fn dashboard_action(
    key: event::KeyEvent,
    ui: &DashboardState,
    area: Rect,
) -> Option<DashboardAction> {
    use DashboardAction::*;
    if key.kind == KeyEventKind::Release {
        return None;
    }
    // Terminal/OS chords must never fall through to unmodified grants or links.
    if key.modifiers.difference(KeyModifiers::SHIFT) == KeyModifiers::CONTROL {
        return (key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Char('c' | 'C')))
            .then_some(Interrupt);
    }
    if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    let press = key.kind == KeyEventKind::Press;
    let code = match key.code {
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        code => code,
    };
    // Exclusive input/confirmation contexts never fall through to globals.
    if ui.full_access_confirm {
        return match code {
            KeyCode::Char('y') if press && ui.full_access_visible(area) => Some(ConfirmFullAccess),
            KeyCode::Char('n') | KeyCode::Esc if press => Some(CancelFullAccess),
            _ => None,
        };
    }
    if ui.workspace_input.is_some() {
        return match key.code {
            KeyCode::Esc if press => Some(CancelInput),
            KeyCode::Enter if press => Some(SubmitInput),
            KeyCode::Backspace => Some(EraseInput),
            KeyCode::Char(c) if !c.is_control() => Some(TypeInput(c)),
            _ => None,
        };
    }
    let authorization = ui.authorization_visible(area);
    let has_authorization = !ui.pending_authorizations.is_empty();
    let commands = ui.commands_open
        && !ui.help_open
        && !ui.intelligence_open
        && commands_overlay_visible(area);
    if !press
        && !matches!(
            code,
            KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown
        )
    {
        return None;
    }
    match code {
        KeyCode::Esc => Some(Dismiss),
        KeyCode::Char('?') => Some(Help),
        KeyCode::Char('i') => Some(Intelligence),
        KeyCode::Char('r') if ui.intelligence_open => Some(RefreshIntelligence),
        KeyCode::Char('c') => Some(Commands),
        KeyCode::Char('p') => Some(FullAccess),
        KeyCode::Char('l') => Some(Language),
        KeyCode::Char('o') => Some(Setup),
        KeyCode::Char('w') => Some(Observatory),
        KeyCode::Char('g') => Some(Project),
        KeyCode::Char('b') => Some(Author),
        KeyCode::Char('+') => Some(AddWorkspace),
        KeyCode::Char('a') => Some(GrantAllCommands),
        KeyCode::Char('f') if commands => Some(ToggleCommandTrust),
        KeyCode::Char('y') if authorization => Some(Approve),
        KeyCode::Char('n') if authorization => Some(Deny),
        KeyCode::Char('y' | 'n') if has_authorization => Some(ShowAuthorization),
        KeyCode::Char('y' | 'n') => Some(NoPendingAuthorization),
        KeyCode::Up if commands => Some(CommandsUp),
        KeyCode::Down if commands => Some(CommandsDown),
        KeyCode::PageUp if commands => Some(CommandsPageUp),
        KeyCode::PageDown if commands => Some(CommandsPageDown),
        KeyCode::Up if authorization => Some(AuthorizationUp),
        KeyCode::Down if authorization => Some(AuthorizationDown),
        KeyCode::PageUp if authorization => Some(AuthorizationPageUp),
        KeyCode::PageDown if authorization => Some(AuthorizationPageDown),
        KeyCode::Left if !ui.help_open && !authorization => Some(WorkspaceLeft),
        KeyCode::Right if !ui.help_open && !authorization => Some(WorkspaceRight),
        _ => None,
    }
}

pub(super) fn intelligence_url_for_workspace(base: &str, workspace: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(workspace.as_bytes()).collect::<String>();
    let separator = if base.contains('#') { '&' } else { '#' };
    format!("{base}{separator}workspace={encoded}")
}

fn open_dashboard_url(ui: &mut DashboardState, url: &str) -> bool {
    match open_external_url(url) {
        Ok(()) => {
            ui.workspace_message = None;
            true
        }
        Err(error) => {
            ui.workspace_message = Some(format!(
                "{}: {error}",
                ui.language.tr("unable to open link")
            ));
            false
        }
    }
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
    let mut status_snapshot: Option<String> = None;
    let mut status_deadline: Option<Instant> = None;
    // Compact fingerprint (avoids >12-field tuple PartialEq limits).
    let mut last_draw_key: Option<(u64, u64, Option<String>)> = None;
    let initial_snapshot = monitor.snapshot();
    let initial_workspaces = ordered_workspaces(&config, &initial_snapshot);
    ui.sync_workspace_order(&initial_workspaces, initial_workspaces.len().max(1));
    if let Some(workspace_id) = focused_workspace_id(&config, &initial_snapshot, ui.workspace_focus)
    {
        request_intelligence_refresh(&monitor, &config, workspace_id);
    }

    loop {
        if *stop_rx.borrow() {
            break;
        }

        if ui.workspace_message != status_snapshot {
            status_snapshot = ui.workspace_message.clone();
            status_deadline = ui
                .workspace_message
                .as_ref()
                .map(|_| Instant::now() + STATUS_MESSAGE_TTL);
        }
        if status_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            ui.workspace_message = None;
            status_snapshot = None;
            status_deadline = None;
        }

        let size = session.terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        if ui.full_access_confirm && !full_access_overlay_visible(area) {
            ui.full_access_confirm = false;
            ui.workspace_message = Some(
                ui.language
                    .tr("full access dialog requires a larger terminal")
                    .to_owned(),
            );
        }
        if ui.workspace_input.is_some() && !workspace_input_overlay_visible(area) {
            ui.workspace_input = None;
            ui.workspace_message = Some(
                ui.language
                    .tr("workspace input requires a larger terminal")
                    .to_owned(),
            );
        }
        let snapshot = monitor.snapshot();
        // Compute pending authorizations once per frame and reuse for ordering +
        // overlay state so the authorization store is not scanned three times.
        let previous_request = ui
            .pending_authorizations
            .get(ui.authorization_focus)
            .map(|request| request.id.clone());
        ui.pending_authorizations = pending_authorizations(&config);
        ui.clamp_authorizations(ui.pending_authorizations.len());
        if ui
            .pending_authorizations
            .get(ui.authorization_focus)
            .map(|request| &request.id)
            != previous_request.as_ref()
        {
            ui.authorization_scroll = 0;
        }
        let approvals = approval_counts(&ui.pending_authorizations);
        let workspaces = ordered_workspaces_with_approvals(&config, &snapshot, &approvals);
        let workspace_count = workspaces.len();
        let visible = workspace_column_count(size.width, workspace_count);
        ui.sync_workspace_order(&workspaces, visible);
        if ui.commands_open && !commands_overlay_visible(area) {
            ui.commands_open = false;
            ui.command_offset = 0;
            ui.workspace_message = Some(
                ui.language
                    .tr("command view requires a larger terminal")
                    .to_owned(),
            );
        }
        if ui.commands_open {
            if let Some(workspace_id) = focused_workspace_id_from(&workspaces, ui.workspace_focus) {
                let total = command_count(&config.workspaces, &workspace_id);
                ui.command_offset = ui
                    .command_offset
                    .min(total.saturating_sub(command_page_size(area)));
            }
        }
        let busy = dashboard_refresh_interval(&snapshot) == ACTIVE_REFRESH_INTERVAL;
        // Coarse frame key: skip terminal paint when the operator-visible state
        // did not change and no spinner needs to advance.
        let flags = (u64::from(ui.help_open))
            | (u64::from(ui.intelligence_open) << 1)
            | (u64::from(ui.commands_open) << 2)
            | (u64::from(ui.full_access_confirm) << 3)
            | (u64::from(ui.workspace_input.is_some()) << 4);
        let draw_key = (
            (u64::from(size.width) << 48)
                | (u64::from(size.height) << 32)
                | (snapshot
                    .observed_active
                    .saturating_add(snapshot.observed_queued << 16)
                    & 0xffff_ffff),
            ((snapshot.tasks.len() as u64) << 48)
                | ((ui.pending_authorizations.len() as u64) << 32)
                | ((ui.workspace_focus as u64) << 24)
                | ((ui.command_offset as u64) << 16)
                | ((ui.authorization_focus as u64) << 8)
                | ((ui.authorization_scroll as u64) & 0xff)
                | (flags << 56),
            ui.workspace_message.clone(),
        );
        let changed = last_draw_key.as_ref() != Some(&draw_key);
        if busy || changed {
            session
                .terminal
                .draw(|frame| draw_dashboard(frame, &snapshot, &config, tick, &ui))?;
            last_draw_key = Some(draw_key);
            tick = tick.wrapping_add(1);
        }

        let refresh_interval = dashboard_refresh_interval(&snapshot);
        if event::poll(refresh_interval)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    let Some(action) = dashboard_action(key, &ui, area) else {
                        continue;
                    };
                    if action == DashboardAction::Interrupt {
                        let _ = interrupt_tx.send(true);
                        break;
                    }
                    if ui.full_access_confirm {
                        match action {
                            DashboardAction::ConfirmFullAccess => {
                                ui.full_access_confirm = false;
                                match config.workspaces.grant_full_user_access() {
                                    Ok((id, root)) => {
                                        monitor.register_workspace(id.clone());
                                        ui.workspace_message = Some(format!(
                                            "{} {id}: {}",
                                            ui.language.tr("full access granted"),
                                            root.display()
                                        ));
                                        let workspaces = ordered_workspaces(&config, &snapshot);
                                        if let Some(index) = workspaces
                                            .iter()
                                            .position(|workspace| workspace.0 == id)
                                        {
                                            ui.set_workspace_focus(
                                                &workspaces,
                                                index,
                                                workspace_column_count(
                                                    size.width,
                                                    workspaces.len(),
                                                ),
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        ui.workspace_message = Some(format!(
                                            "{}: {error}",
                                            ui.language.tr("full access failed")
                                        ));
                                    }
                                }
                            }
                            DashboardAction::CancelFullAccess => {
                                ui.full_access_confirm = false;
                                ui.workspace_message =
                                    Some(ui.language.tr("full access cancelled").to_owned());
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if ui.workspace_input.is_some() {
                        match action {
                            DashboardAction::CancelInput => {
                                ui.workspace_input = None;
                                ui.workspace_message =
                                    Some(ui.language.tr("workspace add cancelled").to_owned());
                            }
                            DashboardAction::SubmitInput => {
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
                                            let workspaces = ordered_workspaces(&config, &snapshot);
                                            let count = workspaces.len();
                                            if let Some(index) = workspaces
                                                .iter()
                                                .position(|workspace| workspace.0 == id)
                                            {
                                                ui.set_workspace_focus(
                                                    &workspaces,
                                                    index,
                                                    workspace_column_count(size.width, count),
                                                );
                                            } else {
                                                ui.sync_workspace_order(
                                                    &workspaces,
                                                    workspace_column_count(size.width, count),
                                                );
                                            }
                                        }
                                        Err(error) => {
                                            ui.workspace_message =
                                                Some(format!("workspace rejected: {error}"));
                                        }
                                    }
                                }
                            }
                            DashboardAction::EraseInput => {
                                if let Some(input) = ui.workspace_input.as_mut() {
                                    input.pop();
                                }
                            }
                            DashboardAction::TypeInput(character) => {
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
                    match action {
                        DashboardAction::Dismiss => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                            ui.workspace_message = None;
                        }
                        DashboardAction::Help => {
                            ui.help_open = !ui.help_open;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                        }
                        DashboardAction::Intelligence => {
                            ui.intelligence_open = !ui.intelligence_open;
                            ui.help_open = false;
                            ui.commands_open = false;
                            if ui.intelligence_open {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, &snapshot, ui.workspace_focus)
                                {
                                    request_intelligence_refresh(&monitor, &config, workspace_id);
                                }
                            }
                        }
                        DashboardAction::RefreshIntelligence => {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, &snapshot, ui.workspace_focus)
                            {
                                request_intelligence_refresh(&monitor, &config, workspace_id);
                            }
                        }
                        DashboardAction::Commands => {
                            if ui.commands_open {
                                ui.commands_open = false;
                                ui.command_offset = 0;
                            } else if commands_overlay_visible(area) {
                                ui.commands_open = true;
                                ui.command_offset = 0;
                                ui.workspace_message = None;
                            } else {
                                ui.workspace_message = Some(
                                    ui.language
                                        .tr("command view requires a larger terminal")
                                        .to_owned(),
                                );
                            }
                            ui.help_open = false;
                            ui.intelligence_open = false;
                        }
                        DashboardAction::FullAccess => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                            if config.workspaces.full_access_enabled() {
                                ui.workspace_message =
                                    Some(ui.language.tr("full access already enabled").to_owned());
                            } else if !full_access_overlay_visible(area) {
                                ui.workspace_message = Some(
                                    ui.language
                                        .tr("full access dialog requires a larger terminal")
                                        .to_owned(),
                                );
                            } else {
                                ui.full_access_confirm = true;
                                ui.workspace_message = None;
                            }
                        }
                        DashboardAction::Language => {
                            ui.language = ui.language.toggle();
                            if ui.help_open || ui.intelligence_open {
                                ui.workspace_message = None;
                            } else {
                                ui.workspace_message = Some(format!(
                                    "{}: {}",
                                    ui.language.tr("LANGUAGE"),
                                    ui.language.name()
                                ));
                            }
                        }
                        DashboardAction::Setup => {
                            if !open_dashboard_url(&mut ui, &config.setup_url()) {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                                ui.commands_open = false;
                            }
                        }
                        DashboardAction::Observatory => {
                            let workspaces = ordered_workspaces(&config, &snapshot);
                            let url = workspaces
                                .get(ui.workspace_focus.min(workspaces.len().saturating_sub(1)))
                                .map(|workspace| {
                                    intelligence_url_for_workspace(
                                        &config.intelligence_url,
                                        &workspace.0,
                                    )
                                })
                                .unwrap_or_else(|| config.intelligence_url.clone());
                            if !open_dashboard_url(&mut ui, &url) {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                                ui.commands_open = false;
                            }
                        }
                        DashboardAction::Project => {
                            if !open_dashboard_url(&mut ui, &config.project_url) {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                                ui.commands_open = false;
                            }
                        }
                        DashboardAction::GrantAllCommands => {
                            if !ui.commands_open {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                            }
                            let request_target = ui
                                .pending_authorizations
                                .get(ui.authorization_focus)
                                .filter(|_| ui.authorization_visible(area))
                                .map(|request| {
                                    (
                                        request.workspace.clone(),
                                        request.kind
                                            == crate::authorization::AuthorizationKind::DestructiveDelete,
                                    )
                                });
                            if request_target
                                .as_ref()
                                .map(|(_, destructive)| *destructive)
                                .unwrap_or(false)
                            {
                                ui.workspace_message = Some(
                                    ui.language
                                        .tr("all command authorization does not include delete")
                                        .to_owned(),
                                );
                            } else {
                                let workspace_id =
                                    request_target.map(|(workspace, _)| workspace).or_else(|| {
                                        focused_workspace_id(&config, &snapshot, ui.workspace_focus)
                                    });
                                ui.workspace_message = Some(match workspace_id {
                                    Some(workspace_id) => match config
                                        .workspaces
                                        .all_commands_authorized(Some(&workspace_id))
                                    {
                                        Ok(true) => format!(
                                            "{} {}",
                                            ui.language.tr("all commands already authorized"),
                                            workspace_id
                                        ),
                                        _ => match config
                                            .workspaces
                                            .set_all_commands_authorized(Some(&workspace_id), true)
                                        {
                                            Ok(_) => format!(
                                                "{} {}",
                                                ui.language.tr("all commands authorized"),
                                                workspace_id
                                            ),
                                            Err(error) => format!(
                                                "{}: {error}",
                                                ui.language.tr("all command authorization failed")
                                            ),
                                        },
                                    },
                                    None => ui.language.tr("no workspace selected").to_owned(),
                                });
                            }
                            ui.clamp_authorizations(pending_authorizations(&config).len());
                        }
                        DashboardAction::Author => {
                            if !open_dashboard_url(&mut ui, &config.author_url) {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                                ui.commands_open = false;
                            }
                        }
                        DashboardAction::AddWorkspace => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                            if workspace_input_overlay_visible(area) {
                                ui.workspace_input = Some(String::new());
                                ui.workspace_message = None;
                            } else {
                                ui.workspace_message = Some(
                                    ui.language
                                        .tr("workspace input requires a larger terminal")
                                        .to_owned(),
                                );
                            }
                        }
                        DashboardAction::ToggleCommandTrust => {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, &snapshot, ui.workspace_focus)
                            {
                                let enabled = config
                                    .workspaces
                                    .all_commands_authorized(Some(&workspace_id))
                                    .unwrap_or(false);
                                ui.workspace_message = Some(
                                    match config
                                        .workspaces
                                        .set_all_commands_authorized(Some(&workspace_id), !enabled)
                                    {
                                        Ok(_) if enabled => format!(
                                            "{} {workspace_id}",
                                            ui.language.tr("all command authorization disabled")
                                        ),
                                        Ok(_) => format!(
                                            "{} {workspace_id}",
                                            ui.language.tr("all commands authorized")
                                        ),
                                        Err(error) => format!(
                                            "{}: {error}",
                                            ui.language.tr("all command authorization failed")
                                        ),
                                    },
                                );
                            }
                        }
                        DashboardAction::ShowAuthorization => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.commands_open = false;
                            if authorization_overlay_visible(area) {
                                ui.workspace_message = None;
                                ui.authorization_scroll = 0;
                            } else {
                                ui.workspace_message = Some(
                                    ui.language
                                        .tr("authorization view requires a larger terminal")
                                        .to_owned(),
                                );
                            }
                        }
                        DashboardAction::NoPendingAuthorization => {
                            if !ui.commands_open {
                                ui.help_open = false;
                                ui.intelligence_open = false;
                            }
                            ui.workspace_message =
                                Some(ui.language.tr("no pending authorization").to_owned());
                        }
                        DashboardAction::Approve => {
                            if let Some(request) =
                                ui.pending_authorizations.get(ui.authorization_focus)
                            {
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
                        DashboardAction::Deny => {
                            if let Some(request) =
                                ui.pending_authorizations.get(ui.authorization_focus)
                            {
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
                        DashboardAction::CommandsUp => {
                            ui.command_offset = ui.command_offset.saturating_sub(1);
                        }
                        DashboardAction::CommandsDown => {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, &snapshot, ui.workspace_focus)
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
                        DashboardAction::CommandsPageUp => {
                            let page = command_page_size(Rect::new(0, 0, size.width, size.height));
                            ui.command_offset = ui.command_offset.saturating_sub(page);
                        }
                        DashboardAction::CommandsPageDown => {
                            if let Some(workspace_id) =
                                focused_workspace_id(&config, &snapshot, ui.workspace_focus)
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
                        DashboardAction::AuthorizationPageUp => {
                            ui.authorization_scroll = ui.authorization_scroll.saturating_sub(5);
                        }
                        DashboardAction::AuthorizationPageDown => {
                            ui.authorization_scroll = ui.authorization_scroll.saturating_add(5);
                        }
                        DashboardAction::AuthorizationUp => {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus = ui.authorization_focus.saturating_sub(1);
                                ui.authorization_scroll = 0;
                            }
                        }
                        DashboardAction::AuthorizationDown => {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus =
                                    ui.authorization_focus.saturating_add(1).min(total - 1);
                                ui.authorization_scroll = 0;
                            }
                        }
                        DashboardAction::WorkspaceLeft => {
                            let previous = ui.workspace_focus_id.clone();
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            let next = ui.workspace_focus.saturating_sub(step);
                            ui.set_workspace_focus(&workspaces, next, visible);
                            ui.command_offset = 0;
                            if ui.workspace_focus_id != previous {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, &snapshot, ui.workspace_focus)
                                {
                                    request_intelligence_refresh(&monitor, &config, workspace_id);
                                }
                            }
                        }
                        DashboardAction::WorkspaceRight => {
                            let previous = ui.workspace_focus_id.clone();
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            let next = ui
                                .workspace_focus
                                .saturating_add(step)
                                .min(workspaces.len().saturating_sub(1));
                            ui.set_workspace_focus(&workspaces, next, visible);
                            ui.command_offset = 0;
                            if ui.workspace_focus_id != previous {
                                if let Some(workspace_id) =
                                    focused_workspace_id(&config, &snapshot, ui.workspace_focus)
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
                        if !open_dashboard_url(&mut ui, &url) {
                            ui.help_open = false;
                        }
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
    if mouse.kind != MouseEventKind::Down(MouseButton::Left)
        || mouse.column >= width
        || mouse.row >= height
    {
        return None;
    }
    let point = (mouse.column, mouse.row);
    let area = Rect::new(0, 0, width, height);

    if ui.full_access_confirm
        || ui.workspace_input.is_some()
        || ui.commands_open
        || ui.intelligence_open
        || ui.authorization_visible(area)
    {
        return None;
    }
    if ui.help_open {
        return help_link_at(point, area, config);
    }

    if width >= 124 && mouse.row >= height.saturating_sub(2) {
        let links_row = Rect::new(0, height.saturating_sub(2), width, 1);
        let controls_row = Rect::new(0, height.saturating_sub(1), width, 1);
        let project = wide_footer_project_text(config, ui.language, width);
        let project_x = links_row
            .x
            .saturating_add(Span::raw("  wcode  ").width() as u16);
        let project_rect = Rect::new(
            project_x,
            links_row.y,
            Span::raw(&project).width() as u16,
            1,
        );
        if point_in_rect(point, project_rect) {
            return Some(config.project_url.clone());
        }
        let author_x = project_rect
            .x
            .saturating_add(project_rect.width)
            .saturating_add("  by  ".len() as u16);
        let author_rect = Rect::new(
            author_x,
            links_row.y,
            Span::raw(&config.author_handle).width() as u16,
            1,
        );
        if point_in_rect(point, author_rect) {
            return Some(config.author_url.clone());
        }

        let pending_authorizations = config
            .workspaces
            .authorization_requests(256)
            .iter()
            .filter(|request| request.status == AuthorizationStatus::Pending)
            .count();
        let key_width = |key: &str| Span::raw(key).width() as u16 + 2;
        let label_width = |label: &str| Span::raw(label).width() as u16 + 3;
        let pending_width = key_width("Y/N")
            + if pending_authorizations > 0 {
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
            + key_width("A")
            + label_width(ui.language.tr("all"))
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

pub(super) fn point_in_rect((x, y): (u16, u16), rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/keys.rs"]
mod key_tests;

pub(super) fn dashboard_refresh_interval(snapshot: &MonitorSnapshot) -> Duration {
    let tasks_busy = snapshot
        .tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Queued | TaskStatus::Running));
    let intelligence_busy = snapshot.intelligence.values().any(|stats| stats.refreshing);
    let tunnel_busy = matches!(snapshot.tunnel_running, Some(false))
        || snapshot.public_endpoint.as_deref() == Some("pending")
        || snapshot
            .tunnel_runtime
            .iter()
            .any(|tunnel| tunnel.circuit_open || tunnel.state == "connecting");
    if tasks_busy || intelligence_busy || tunnel_busy {
        ACTIVE_REFRESH_INTERVAL
    } else {
        IDLE_REFRESH_INTERVAL
    }
}
