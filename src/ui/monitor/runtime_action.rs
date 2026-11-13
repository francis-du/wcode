use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DashboardAction {
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

pub(super) fn dashboard_action(
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
