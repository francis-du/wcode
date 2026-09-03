use super::*;
use std::fs;

#[test]
fn full_access_elevates_existing_roots_and_keeps_hard_path_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", dir.path());
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let workspaces = Workspaces::new([&project], true, true).unwrap();

    let (home_id, home) = workspaces.grant_full_user_access().unwrap();
    assert_eq!(home, dir.path().canonicalize().unwrap());
    assert!(workspaces.full_access_enabled());
    assert!(workspaces.select(Some(&home_id)).is_ok());
    let capabilities = workspaces.capabilities();
    assert_eq!(capabilities["security"]["full_access"], true);
    assert_eq!(capabilities["security"]["user_home_workspace"], true);
    assert_eq!(capabilities["security"]["broad_workspace_roots"], false);
    assert!(capabilities["security"]["full_access_scope"]
        .as_str()
        .unwrap()
        .contains("filesystem root"));
    assert!(workspaces
        .select(Some(&home_id))
        .unwrap()
        .1
        .read_file(".ssh/config", 1, None)
        .is_err());

    if let Some(home) = previous_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn model_reads_preserve_source_and_cap_each_request_at_one_thousand_lines() {
    let dir = tempfile::tempdir().unwrap();
    let source = (1..=1_200)
        .map(|line| format!("    line_{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.path().join("source.txt"), &source).unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();

    let default_view = workspace.read_file("source.txt", 1, None).unwrap();
    assert_eq!(default_view.start_line, 1);
    assert_eq!(default_view.end_line, 1_000);
    assert_eq!(default_view.content.lines().count(), 1_000);
    assert!(default_view.content.starts_with("    line_1\n    line_2"));

    let oversized = workspace.read_file("source.txt", 101, Some(1_200)).unwrap();
    assert_eq!(oversized.start_line, 101);
    assert_eq!(oversized.end_line, 1_100);
    assert_eq!(oversized.content.lines().count(), 1_000);
}
