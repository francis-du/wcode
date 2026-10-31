use super::*;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn command_overlay_tracks_disabled_execution_and_session_override() {
    assert_eq!(UiLanguage::ZhCn.tr("EXECUTION DISABLED"), "命令执行已禁用");
    for (allow_exec, default_label) in [(false, "EXECUTION DISABLED"), (true, "DEFAULT + SANDBOX")]
    {
        let root = tempfile::tempdir().expect("workspace directory");
        let workspaces =
            Workspaces::new([root.path()], true, allow_exec).expect("workspace registry");
        let workspace_id = workspaces.default_id();
        for (override_enabled, expected_label) in [
            (false, default_label),
            (true, "SESSION OVERRIDE"),
            (false, default_label),
        ] {
            let access = workspaces
                .set_all_commands_authorized(Some(workspace_id), override_enabled)
                .expect("set session override");
            assert_eq!(access["exec_enabled"], allow_exec);
            assert_eq!(access["all_commands_authorized"], override_enabled);
            for language in [UiLanguage::En, UiLanguage::ZhCn] {
                let backend = TestBackend::new(120, 30);
                let mut terminal = Terminal::new(backend).expect("terminal");
                terminal
                    .draw(|frame| {
                        render_commands_overlay(
                            frame,
                            frame.area(),
                            &workspaces,
                            workspace_id,
                            0,
                            None,
                            language,
                        );
                    })
                    .expect("render command controls");
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                // Ignore the empty cells reserved after wide CJK glyphs.
                let text = text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                for label in [
                    "EXECUTION DISABLED",
                    "DEFAULT + SANDBOX",
                    "SESSION OVERRIDE",
                ] {
                    let translated = language
                        .tr(label)
                        .chars()
                        .filter(|character| !character.is_whitespace())
                        .collect::<String>();
                    assert_eq!(
                        text.contains(&translated),
                        label == expected_label,
                        "command state {label:?}: exec={allow_exec}, override={override_enabled}, language={language:?}"
                    );
                }
            }
        }
    }
}
