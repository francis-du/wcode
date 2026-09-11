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
                    render_authorization_overlay(frame, frame.area(), &requests, 0, 0, language)
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
                .expect("both shortcuts must be visible on the same row");
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
