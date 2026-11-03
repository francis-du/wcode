use super::*;
use sha2::{Digest, Sha256};

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn head(root: &Path) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(root: &Path, message: &str) -> String {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            message,
        ],
    );
    head(root)
}

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    fs::write(root.path().join("main.rs"), "fn before() {}\n").unwrap();
    git(root.path(), &["add", "main.rs"]);
    commit(root.path(), "first");
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    (root, workspace)
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[tokio::test]
async fn historical_source_reads_the_exact_revision_and_reports_its_sha() {
    let (root, workspace) = fixture();
    let first = head(root.path());
    fs::write(root.path().join("main.rs"), "fn after() {}\n").unwrap();
    git(root.path(), &["add", "main.rs"]);
    let second = commit(root.path(), "second");

    let source = workspace
        .historical_source(&first, "main.rs", 1, None)
        .await
        .unwrap();
    assert_eq!(source.kind, "source");
    assert_eq!(source.revision, first);
    assert_ne!(source.revision, second);
    assert_eq!(source.content, "fn before() {}");
    assert_eq!(source.sha256, digest("fn before() {}\n"));
    assert_eq!(
        (source.start_line, source.end_line, source.total_lines),
        (1, 1, 1)
    );
    assert!(!source.redacted && !source.truncated);
    assert_eq!(source.line_ending, "lf");

    let current = workspace
        .historical_source(&second, "main.rs", 1, None)
        .await
        .unwrap();
    assert_eq!(current.content, "fn after() {}");
    assert_eq!(current.sha256, digest("fn after() {}\n"));
    assert_eq!(
        fs::read_to_string(root.path().join("main.rs")).unwrap(),
        "fn after() {}\n"
    );
    assert!(!workspace.write_enabled());
}

#[tokio::test]
async fn historical_source_reads_paths_absent_from_the_worktree() {
    let (root, workspace) = fixture();
    let first = head(root.path());
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/old.rs"), "fn deleted_later() {}\n").unwrap();
    git(root.path(), &["add", "src/old.rs"]);
    let nested = commit(root.path(), "add nested");
    fs::remove_dir_all(root.path().join("src")).unwrap();
    git(root.path(), &["add", "-A"]);
    commit(root.path(), "remove nested");

    let source = workspace
        .historical_source(&nested, "src/old.rs", 1, None)
        .await
        .unwrap();
    assert_eq!(source.kind, "source");
    assert_eq!(source.content, "fn deleted_later() {}");
    assert_eq!(source.sha256, digest("fn deleted_later() {}\n"));
    assert!(workspace
        .historical_source(&first, "src/old.rs", 1, None)
        .await
        .is_err());
}

#[tokio::test]
async fn historical_source_reports_unknown_missing_and_restricted_states() {
    let (root, workspace) = fixture();
    let revision = head(root.path());

    let unknown = workspace
        .historical_source(
            "0123456789abcdef0123456789abcdef01234567",
            "main.rs",
            1,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("revision is unknown"), "{unknown}");

    let missing = workspace
        .historical_source(&revision, "absent.rs", 1, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(missing.contains("missing at this revision"), "{missing}");

    let protected = workspace
        .historical_source(&revision, ".git/config", 1, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(protected.contains("protected"), "{protected}");
    for path in [
        ".env",
        "../main.rs",
        "/etc/passwd",
        "main.rs\nother.rs",
        ".",
    ] {
        assert!(
            workspace
                .historical_source(&revision, path, 1, None)
                .await
                .is_err(),
            "{path}"
        );
    }
}

#[tokio::test]
async fn historical_source_rejects_revision_expressions_and_incomplete_oids() {
    let (root, workspace) = fixture();
    let revision = head(root.path());
    let short = revision[..12].to_owned();
    for expression in ["HEAD", "HEAD~1", short.as_str(), "", "not-a-revision"] {
        let error = workspace
            .historical_source(expression, "main.rs", 1, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("complete commit object ID"),
            "{expression}: {error}"
        );
    }
    let denied = Workspace::new(root.path(), false, false).unwrap();
    assert!(denied
        .historical_source(&revision, "main.rs", 1, None)
        .await
        .is_err());
}

#[tokio::test]
async fn historical_source_marks_binary_directory_and_truncated_states() {
    let (root, workspace) = fixture();
    fs::write(root.path().join("blob.bin"), [0u8, 1, 2, 0, 255]).unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/inner.txt"), "inner\n").unwrap();
    git(root.path(), &["add", "-A"]);
    let revision = commit(root.path(), "binary and directory");

    let binary = workspace
        .historical_source(&revision, "blob.bin", 1, None)
        .await
        .unwrap();
    assert_eq!(binary.kind, "binary");
    assert_eq!(binary.content, "");
    assert_eq!(binary.sha256, "");

    let directory = workspace
        .historical_source(&revision, "dir", 1, None)
        .await
        .unwrap();
    assert_eq!(directory.kind, "directory");

    let oversized = "x".repeat(300 * 1024);
    fs::write(root.path().join("large.txt"), format!("{oversized}\n")).unwrap();
    git(root.path(), &["add", "large.txt"]);
    let large_revision = commit(root.path(), "oversized");
    let large = workspace
        .historical_source(&large_revision, "large.txt", 1, None)
        .await
        .unwrap();
    assert_eq!(large.kind, "truncated");
    assert!(large.truncated && !large.redacted);
    assert_eq!(large.content, "");
}

#[tokio::test]
async fn historical_source_bounds_the_window_and_withholds_redacted_output() {
    let (root, workspace) = fixture();
    let mut content = String::new();
    for index in 1..=500 {
        content.push_str(&format!("line {index}\n"));
    }
    fs::write(root.path().join("big.txt"), &content).unwrap();
    git(root.path(), &["add", "big.txt"]);
    let revision = commit(root.path(), "big");

    let first = workspace
        .historical_source(&revision, "big.txt", 1, None)
        .await
        .unwrap();
    assert_eq!(first.kind, "source");
    assert_eq!(first.total_lines, 500);
    assert_eq!((first.start_line, first.end_line), (1, 240));
    assert_eq!(first.content.lines().count(), 240);
    assert!(!first.redacted);
    assert_eq!(first.sha256, digest(&content));

    let later = workspace
        .historical_source(&revision, "big.txt", 481, Some(500))
        .await
        .unwrap();
    assert_eq!((later.start_line, later.end_line), (481, 500));
    assert_eq!(later.content.lines().count(), 20);
    assert!(later.content.starts_with("line 481"));

    // The command layer redacts a credential-shaped line anywhere in the bounded
    // output. Historical source then withholds content rather than publish a
    // SHA that does not identify the bytes an operator would see.
    let mut secret = String::new();
    for index in 1..=20 {
        secret.push_str(&format!("line {index}\n"));
    }
    secret.push_str("API_TOKEN=private-historical-value\n");
    fs::write(root.path().join("secret.txt"), &secret).unwrap();
    git(root.path(), &["add", "secret.txt"]);
    let secret_revision = commit(root.path(), "secret");
    let redacted = workspace
        .historical_source(&secret_revision, "secret.txt", 1, None)
        .await
        .unwrap();
    assert_eq!(redacted.kind, "redacted");
    assert!(redacted.redacted);
    assert_eq!(redacted.content, "");
    assert_eq!(redacted.sha256, "");
    assert!(!redacted.content.contains("private-historical-value"));
}

#[tokio::test]
async fn historical_revisions_lists_bounded_recent_commits_newest_first() {
    let (root, workspace) = fixture();
    let mut revisions = Vec::new();
    for index in 0..4 {
        fs::write(root.path().join("main.rs"), format!("fn v{index}() {{}}\n")).unwrap();
        git(root.path(), &["add", "main.rs"]);
        revisions.push(commit(root.path(), &format!("change {index}")));
    }
    let (listed, incomplete) = workspace.historical_revisions(2).await.unwrap();
    assert!(!incomplete);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].revision, revisions[3]);
    assert_eq!(listed[1].revision, revisions[2]);
    assert_eq!(listed[0].revision.len(), 40);
    assert!(!listed[0].short_revision.is_empty());
    assert_eq!(listed[0].subject, "change 3");

    let (all, _) = workspace
        .historical_revisions(MAX_HISTORICAL_REVISIONS)
        .await
        .unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].revision, revisions[3]);
    let denied = Workspace::new(root.path(), false, false).unwrap();
    assert!(denied.historical_revisions(4).await.is_err());
}

#[test]
fn historical_source_error_classifier_separates_explicit_states() {
    assert_eq!(
        historical_source_error_code("historical source requires a complete commit object ID"),
        Some("invalid_revision")
    );
    assert_eq!(
        historical_source_error_code(HISTORICAL_REVISION_UNKNOWN),
        Some("unknown_revision")
    );
    assert_eq!(
        historical_source_error_code(HISTORICAL_SOURCE_MISSING),
        Some("source_missing")
    );
    assert_eq!(
        historical_source_error_code(
            "protected credential or repository-control path is not accessible: .git"
        ),
        Some("path_restricted")
    );
    assert_eq!(
        historical_source_error_code("symlink paths are blocked to preserve workspace isolation"),
        Some("path_restricted")
    );
    assert_eq!(
        historical_source_error_code("path traversal is not allowed"),
        Some("path_restricted")
    );
    assert_eq!(
        historical_source_error_code("one explicit file is required"),
        Some("invalid_path")
    );
    assert_eq!(
        historical_source_error_code("historical source Git probe is incomplete"),
        None
    );
}

#[test]
fn revision_rows_with_control_characters_or_bad_oids_are_incomplete() {
    let valid = format!(
        "{}\u{1f}abc1234\u{1f}2026-11-03\u{1f}a normal subject\n",
        "a".repeat(40)
    );
    let (rows, incomplete) = parse_historical_revisions(&valid, 8);
    assert!(!incomplete);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].subject, "a normal subject");

    for malformed in [
        format!(
            "{}\u{1f}abc\u{1f}2026-11-03\u{1f}subject\u{1f}extra",
            "a".repeat(40)
        ),
        format!("{}\u{1f}abc\u{1f}2026-11-03\u{1f}subject", "b".repeat(12)),
        format!(
            "{}\u{1f}abc\u{1f}2026-11-03\u{1f}has\u{1}control",
            "a".repeat(40)
        ),
        format!("{}\u{1f}abc\u{1f}2026-11-03\u{1f}", "a".repeat(40)),
    ] {
        let (rows, incomplete) = parse_historical_revisions(&malformed, 8);
        assert!(rows.is_empty(), "{malformed:?}");
        assert!(incomplete, "{malformed:?}");
    }
}
