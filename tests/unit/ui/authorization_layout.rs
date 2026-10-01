use super::*;

#[test]
fn audit_narrow_authorization_keeps_approve_and_deny_visible() {
    for language in [UiLanguage::En, UiLanguage::ZhCn] {
        for (width, height, count) in [(40, 10, 1), (40, 10, 3), (55, 14, 3), (100, 24, 3)] {
            let requests = (0..count)
                .map(|index| AuthorizationRequest {
                    id: format!("AUTH-{:08}", index + 1),
                    ..monitor_test_request()
                })
                .collect::<Vec<_>>();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render_authorization_overlay(
                        frame,
                        frame.area(),
                        &requests,
                        0,
                        0,
                        None,
                        language,
                    )
                })
                .unwrap();
            let rows = terminal
                .backend()
                .buffer()
                .content
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>();
            assert!(
                rows.iter().any(|row| row.contains('A')),
                "all-command shortcut missing at {width}x{height}"
            );
            let controls = rows
                .iter()
                .find(|row| row.contains('Y') && row.contains('N'))
                .expect("approve and deny shortcuts must remain visible together");
            // Wide glyphs occupy a second buffer cell containing a blank.
            let visible = controls.split_whitespace().collect::<String>();
            let deny = if language == UiLanguage::ZhCn {
                "拒绝"
            } else {
                "deny"
            };
            assert!(
                visible.contains(deny),
                "deny missing at {width}x{height}: {controls}"
            );
        }
    }
}
#[test]
fn human_decision_overlay_exposes_exact_controls_without_all_command_shortcut() {
    let request = AuthorizationRequest {
        kind: crate::authorization::AuthorizationKind::HumanDecision,
        summary: "Approve VP-bound; code=sha256:current; statement=reviewed".into(),
        ..monitor_test_request()
    };
    for (width, height) in [(40, 10), (100, 24)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_authorization_overlay(
                    frame,
                    frame.area(),
                    std::slice::from_ref(&request),
                    0,
                    0,
                    None,
                    UiLanguage::En,
                )
            })
            .unwrap();
        let rows = terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let controls = rows
            .iter()
            .find(|row| row.contains('Y') && row.contains('N'))
            .unwrap();
        assert!(!controls.contains('A'), "{controls}");
        assert!(
            !rows.iter().any(|row| row.contains("authorize commands")),
            "{rows:?}"
        );
    }
}
