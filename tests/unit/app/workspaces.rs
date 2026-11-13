use super::*;
use super::args::{parse_workspace_file, parse_workspace_list};

#[test]
fn workspace_list_splits_on_the_platform_separator_and_newlines() {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let raw = format!("/alpha\n/beta {separator} /gamma\n\n  /delta  \n");
    assert_eq!(
        parse_workspace_list(&raw),
        vec![
            PathBuf::from("/alpha"),
            PathBuf::from("/beta"),
            PathBuf::from("/gamma"),
            PathBuf::from("/delta"),
        ]
    );
}

#[test]
fn workspace_list_drops_empty_entries() {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let raw = format!("{separator}{separator}/only{separator}");
    assert_eq!(parse_workspace_list(&raw), vec![PathBuf::from("/only")]);
    assert!(parse_workspace_list("").is_empty());
    assert!(parse_workspace_list("\n \r\n").is_empty());
}

#[test]
fn workspace_file_skips_comments_and_blank_lines() {
    let contents = "# session roots\n\n/a\n  /b  \n   # another\n";
    assert_eq!(
        parse_workspace_file(contents),
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert!(parse_workspace_file("# only comments\n\n").is_empty());
}

#[test]
fn explicit_workspace_arguments_win_over_the_session() {
    let explicit = vec![PathBuf::from("/explicit")];
    assert_eq!(resolved_workspace_roots(&explicit), explicit);
}

#[test]
fn missing_session_falls_back_to_the_current_directory() {
    // The session variables are process-global, so only assert the fallback
    // when neither is set. A parallel test may set them; both outcomes are
    // still correct behaviour, so accept the inherited list as well.
    let roots = resolved_workspace_roots(&[]);
    assert!(!roots.is_empty());
    let inherited =
        std::env::var("WCODE_WORKSPACES").is_ok() || std::env::var("WCODE_WORKSPACES_FILE").is_ok();
    if !inherited {
        assert_eq!(roots, vec![PathBuf::from(".")]);
    }
}
