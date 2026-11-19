use super::*;

#[tokio::test]
async fn worktree_status_uses_git_file_as_well_as_git_directory() {
    let root = tempfile::tempdir().unwrap();
    let worktree = root.path().join("worktree");
    let metadata = root.path().join("git-metadata");
    let output = std::process::Command::new("git")
        .args(["init", "--quiet", "--separate-git-dir"])
        .arg(&metadata)
        .arg(&worktree)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(worktree.join(".git").is_file());
    let mut expected_files = vec!["note.txt", "项目.txt"];
    if cfg!(unix) {
        expected_files.push("before -> after.txt");
    }
    for name in &expected_files {
        std::fs::write(worktree.join(name), "fixture\n").unwrap();
    }
    let workspace = Workspace::new(&worktree, false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let status = harness.worktree_status_snapshot(&workspace).await.unwrap();
    assert_eq!(status["available"], true, "{status}");
    let files = status["files"].as_array().unwrap();
    assert_eq!(files.len(), expected_files.len());
    for name in expected_files {
        assert!(
            files
                .iter()
                .any(|file| file["path"] == name && file["untracked"] == true),
            "missing exact path {name}: {status}"
        );
    }
    let monitor = crate::monitor::TaskMonitor::new(["fixture".to_owned()]);
    let report = harness
        .review_changes("fixture", &workspace, 30, &monitor)
        .await
        .unwrap();
    assert!(report
        .files
        .iter()
        .any(|file| file.path == "项目.txt" && file.untracked));
    #[cfg(unix)]
    {
        assert!(report
            .files
            .iter()
            .any(|file| file.path == "before -> after.txt" && file.untracked));
        assert!(report.files.iter().all(|file| file.path != "after.txt"));
    }
}

#[tokio::test]
async fn incomplete_review_does_not_classify_the_change_set_as_docs_only() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    for index in 0..MAX_REVIEW_FILES {
        std::fs::write(root.path().join(format!("doc-{index:04}.md")), "fixture\n").unwrap();
    }
    std::fs::write(root.path().join("zz-app.rs"), "fn fixture() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let monitor = crate::monitor::TaskMonitor::new(["fixture".to_owned()]);
    let report = ToolHarness::new(2)
        .unwrap()
        .review_changes("fixture", &workspace, 30, &monitor)
        .await
        .unwrap();
    assert!(report.truncated);
    assert!(!report.clean);
    assert!(!report.docs_only);
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.code == "docs-only"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "review-truncated"));
}

#[test]
fn review_status_only_splits_rename_and_copy_records() {
    let (files, truncated) = parse_git_status(concat!(
        "?? \"notes -> final.md\"\n",
        "R  before.rs -> after.rs\n",
        "C  source.rs -> copy.rs\n",
        "R  \"old -> name.rs\" -> \"new -> name.rs\"\n",
        "C  \"source\\\" -> quoted.rs\" -> \"copy -> final.rs\"\n",
        "?? \"invalid-\\777.txt\"\n",
    ));
    assert!(!truncated);
    assert!(files["notes -> final.md"].untracked);
    assert_eq!(files["after.rs"].status, "renamed");
    assert_eq!(files["copy.rs"].status, "copied");
    assert_eq!(files["new -> name.rs"].status, "renamed");
    assert_eq!(files["copy -> final.rs"].status, "copied");
    assert!(files[r#""invalid-\777.txt""#].untracked);
}

#[test]
fn review_status_marks_malformed_lines_incomplete_without_losing_valid_records() {
    let (files, incomplete) = parse_git_status(" M first.rs\n无效记录\n?? last.rs\n");
    assert!(incomplete);
    assert_eq!(files.len(), 2);
    assert!(files["first.rs"].unstaged);
    assert!(files["last.rs"].untracked);
}

#[test]
fn review_numstat_prefers_known_literal_paths_over_rename_shorthand() {
    let (mut files, _) =
        parse_git_status(" M \"notes => final.md\"\nR  src/old.rs -> src/new.rs\n");
    assert!(!merge_numstat(
        &mut files,
        "4\t2\tnotes => final.md\n1\t3\tsrc/{old.rs => new.rs}\n",
    ));
    assert_eq!(files.len(), 2);
    assert_eq!(files["notes => final.md"].additions, 4);
    assert_eq!(files["notes => final.md"].deletions, 2);
    assert_eq!(files["src/new.rs"].additions, 1);
}

#[test]
fn review_numstat_skips_invalid_records_and_reports_incomplete_metrics() {
    let (mut files, _) = parse_git_status(" M existing.rs\n");
    let incomplete = merge_numstat(
        &mut files,
        "2\t1\tfirst.rs\ninvalid\t0\tghost.rs\n-\t2\texisting.rs\npartial\n-\t-\tlogo.png\n0\t3\tlast.rs\n",
    );
    assert!(incomplete);
    assert!(!files.contains_key("ghost.rs"));
    assert!(!files["existing.rs"].has_numstat);
    assert!(!files["existing.rs"].binary);
    assert_eq!(files["first.rs"].additions, 2);
    assert!(files["logo.png"].binary);
    assert_eq!(files["last.rs"].deletions, 3);
}

#[test]
fn review_source_classification_tracks_canonical_language_variants() {
    for path in [
        "src/app.mjs",
        "src/api.mts",
        "include/value.hxx",
        "views/index.phtml",
        "scripts/check.bats",
        "lib/plugin.gemspec",
    ] {
        assert_eq!(file_category(path), "source", "{path}");
    }
}

#[test]
fn review_manifest_classification_covers_polyglot_project_files() {
    for path in [
        "deno.jsonc",
        "pubspec.yaml",
        "mix.exs",
        "Gemfile",
        "composer.json",
        "dune-project",
        "DESCRIPTION",
        "Package.swift",
        "pom.xml",
        "build.gradle.kts",
        "Demo.csproj",
    ] {
        assert_eq!(file_category(path), "manifest", "{path}");
    }
}

#[test]
fn review_classification_stays_case_insensitive() {
    assert_eq!(file_category("CARGO.TOML"), "manifest");
    assert_eq!(file_category("Docs/README.MD"), "docs");
    assert_eq!(file_category("Src/Auth/Login_Test.RS"), "test");
    assert_eq!(file_category(".GitHub/Workflows/CI.YML"), "workflow");
    assert!(security_sensitive_path("src/OAuth/TokenStore.rs"));
    assert!(!security_sensitive_path("src/author.rs"));
}
