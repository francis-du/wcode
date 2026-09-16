use super::*;
use event::KeyEvent;

#[test]
fn invisible_command_panel_cannot_toggle_permissions() {
    let ui = DashboardState {
        commands_open: true,
        ..Default::default()
    };
    for (width, height) in [(39, 40), (100, 13), (1, 1)] {
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(
            dashboard_action(key, &ui, Rect::new(0, 0, width, height)),
            None,
            "F must not grant permissions when the command view cannot be drawn"
        );
    }
}

fn request() -> AuthorizationRequest {
    AuthorizationRequest {
        id: "AUTH-keys".to_owned(),
        workspace: "fixture".to_owned(),
        kind: crate::authorization::AuthorizationKind::CommandAccess,
        summary: "fixture command".to_owned(),
        program: Some("cargo".to_owned()),
        fingerprint: "fixture".to_owned(),
        status: AuthorizationStatus::Pending,
        created_at_ms: 1,
        decided_at_ms: None,
    }
}

fn action(c: char, ui: &DashboardState) -> Option<DashboardAction> {
    dashboard_action(
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        ui,
        Rect::new(0, 0, 100, 30),
    )
}

#[test]
fn author_and_bulk_authorization_never_share_a_key() {
    let mut ui = DashboardState::default();
    assert_eq!(action('a', &ui), None);
    assert_eq!(action('b', &ui), Some(DashboardAction::Author));
    assert_eq!(
        action(AUTHOR_SHORTCUT.chars().next().unwrap(), &ui),
        Some(DashboardAction::Author)
    );
    ui.pending_authorizations.push(request());
    assert_eq!(action('A', &ui), Some(DashboardAction::GrantAllCommands));
    assert_eq!(action('B', &ui), Some(DashboardAction::Author));
    ui.help_open = true;
    assert_eq!(action('a', &ui), None);
    assert_eq!(action('y', &ui), None);
    assert_eq!(action('n', &ui), None);
}

#[test]
fn modified_keys_cannot_trigger_unmodified_permissions_or_navigation() {
    let mut ui = DashboardState::default();
    ui.pending_authorizations.push(request());
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        for c in ['a', 'A', 'y', 'n', 'p', 'f', 'w', 'o', 'b', 'l', '+', '?'] {
            let key = KeyEvent::new(KeyCode::Char(c), modifiers);
            assert_eq!(
                dashboard_action(key, &ui, Rect::new(0, 0, 100, 30)),
                None,
                "{key:?}"
            );
        }
    }
    assert_eq!(
        dashboard_action(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &ui,
            Rect::new(0, 0, 100, 30)
        ),
        Some(DashboardAction::Interrupt)
    );
    assert_eq!(action('C', &ui), Some(DashboardAction::Commands));
}

#[test]
fn input_and_full_access_confirmation_are_exclusive_contexts() {
    let mut ui = DashboardState {
        workspace_input: Some(String::new()),
        ..Default::default()
    };
    for c in ['a', 'Y', 'P', 'w', '?', '+', '界'] {
        assert_eq!(action(c, &ui), Some(DashboardAction::TypeInput(c)));
    }
    ui.full_access_confirm = true;
    assert_eq!(action('y', &ui), Some(DashboardAction::ConfirmFullAccess));
    assert_eq!(action('n', &ui), Some(DashboardAction::CancelFullAccess));
    assert_eq!(action('w', &ui), None);
    assert_eq!(action('a', &ui), None);
    assert_eq!(
        dashboard_action(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &ui,
            Rect::new(0, 0, 39, 9)
        ),
        None
    );
}

#[test]
fn repeated_keys_never_repeat_grants_toggles_or_external_links() {
    let mut ui = DashboardState::default();
    ui.pending_authorizations.push(request());
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        for c in ['a', 'y', 'n', 'p', 'f', 'w', 'o', 'b', 'c', '?', '+'] {
            let key = KeyEvent::new_with_kind(KeyCode::Char(c), KeyModifiers::NONE, kind);
            assert_eq!(dashboard_action(key, &ui, Rect::new(0, 0, 100, 30)), None);
        }
    }
    let repeat = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert_eq!(
        dashboard_action(repeat, &ui, Rect::new(0, 0, 100, 30)),
        Some(DashboardAction::AuthorizationDown)
    );
    ui.workspace_input = Some(String::new());
    let enter = KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert_eq!(dashboard_action(enter, &ui, Rect::new(0, 0, 100, 30)), None);
}

#[test]
fn navigation_routes_only_to_the_visible_context() {
    let area = Rect::new(0, 0, 100, 30);
    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT);
    let mut ui = DashboardState::default();
    assert_eq!(dashboard_action(down, &ui, area), None);
    assert_eq!(
        dashboard_action(left, &ui, area),
        Some(DashboardAction::WorkspaceLeft)
    );
    ui.pending_authorizations.push(request());
    assert_eq!(
        dashboard_action(down, &ui, area),
        Some(DashboardAction::AuthorizationDown)
    );
    assert_eq!(dashboard_action(left, &ui, area), None);
    assert_eq!(dashboard_action(down, &ui, Rect::new(0, 0, 39, 9)), None);
    ui.commands_open = true;
    assert_eq!(
        dashboard_action(down, &ui, area),
        Some(DashboardAction::CommandsDown)
    );
    assert_eq!(action('f', &ui), Some(DashboardAction::ToggleCommandTrust));
    ui.commands_open = false;
    ui.help_open = true;
    assert_eq!(dashboard_action(down, &ui, area), None);
    assert_eq!(dashboard_action(left, &ui, area), None);
    assert_eq!(action('f', &ui), None);
}
