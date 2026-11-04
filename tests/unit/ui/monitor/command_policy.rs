use super::*;
use crate::workspace::WorkspaceSecurity;
use ratatui::{backend::TestBackend, Terminal};

fn hardened_security() -> WorkspaceSecurity {
    WorkspaceSecurity {
        allow_risky_exec: false,
        allow_unrestricted_commands: false,
        allow_semantic_exec: false,
        allow_destructive_writes: false,
        allow_overlapping_workspaces: false,
        allow_user_home_workspace: false,
        allow_broad_workspace: false,
    }
}

#[test]
fn command_overlay_reports_the_permissive_session_override() {
    assert_eq!(UiLanguage::ZhCn.tr("EXECUTION DISABLED"), "命令执行已禁用");
    for allow_exec in [false, true] {
        let root = tempfile::tempdir().expect("workspace directory");
        let workspaces =
            Workspaces::new_with_security([root.path()], true, allow_exec, hardened_security())
                .expect("workspace registry");
        let workspace_id = workspaces.default_id();
        for override_enabled in [false, true, false] {
            let access = workspaces
                .set_all_commands_authorized(Some(workspace_id), override_enabled)
                .expect("set session override");
            assert_eq!(access["exec_enabled"], allow_exec);
            // The manager auto-grants, so the reported policy stays permissive.
            assert_eq!(access["all_commands_authorized"], true);
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
                        label == "SESSION OVERRIDE",
                        "command state {label:?}: exec={allow_exec}, override={override_enabled}, language={language:?}"
                    );
                }
            }
        }
    }
}
