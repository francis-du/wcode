use super::*;

#[test]
fn diagnostic_context_redacted_excerpt_never_becomes_edit_ready() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = "synthetic-private-fixture";
    let text = format!("heading\r\n{}=\"{sentinel}\"\r\ntrailer\r\n", "password");
    std::fs::write(root.path().join("settings.txt"), text).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for budget in [1_000, 1_400, 4_000] {
        let pack = harness
            .agent_context("demo", &workspace, "settings.txt:2", budget, &[])
            .unwrap();
        assert_eq!(pack["hot_source"][0]["body"]["redacted"], true);
        assert_eq!(pack["hot_source"][0]["path"], "settings.txt");
        assert_eq!(pack["readiness"]["edit"], "needs_source");
        assert!(pack["readiness"]["advisories"]
            .as_array()
            .unwrap()
            .contains(&json!("source_body_redacted")));
        assert!(!serde_json::to_string(&pack).unwrap().contains(sentinel));
        assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
    }
}

#[test]
fn diagnostic_context_report_to_guarded_edit_preserves_original_newlines() {
    for newline in ["\n", "\r\n"] {
        for budget in [1_000, 1_400, 4_000] {
            let root = tempfile::tempdir().unwrap();
            let lines = (0..20)
                .map(|index| format!("// diagnostic_{index:02} {}", "原文🚀".repeat(80)))
                .collect::<Vec<_>>();
            let original = format!("// header{newline}{}{newline}", lines.join(newline));
            std::fs::write(root.path().join("failure.rs"), &original).unwrap();
            let workspace = Workspace::new(root.path(), true, false).unwrap();
            let harness = ToolHarness::new(4).unwrap();
            let pack = harness
                .agent_context(
                    "demo",
                    &workspace,
                    "修复：failure.rs:2，保留原文",
                    budget,
                    &[],
                )
                .unwrap();
            let source = &pack["hot_source"][0];
            let excerpt = source["body"]["content"].as_str().unwrap();
            assert!(
                original.contains(excerpt),
                "returned source must be an actual byte slice: {newline:?}/{budget}"
            );
            assert_eq!(source["path"], "failure.rs");
            assert_eq!(source["body"]["redacted"], false);
            assert_eq!(pack["readiness"]["edit"], "ready");
            assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
            let sha = source["sha256"].as_str().unwrap();
            let replacement = excerpt.replacen("diagnostic_00", "resolved_00", 1);
            assert_ne!(replacement, excerpt);
            let edit = workspace
                .replace_text("failure.rs", excerpt, &replacement, sha)
                .unwrap();
            assert_ne!(edit.sha256_after, sha);
            let expected = original.replacen(excerpt, &replacement, 1);
            assert_eq!(
                std::fs::read_to_string(root.path().join("failure.rs")).unwrap(),
                expected
            );
            let stale = workspace
                .replace_text("failure.rs", &replacement, excerpt, sha)
                .unwrap_err();
            assert!(stale.to_string().contains("stale file"));
        }
    }
}

#[test]
fn diagnostic_context_read_windows_match_original_bytes_at_eof() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for text in [
        "",
        "\n",
        "\r\n",
        "a\r",
        "a\nb\n",
        "a\r\nb\r\n",
        "a\r\nb",
        "a\r\nb\nc\r\n",
    ] {
        std::fs::write(root.path().join("window.txt"), text).unwrap();
        for (start, end) in [(1, 1), (1, 3), (2, 3), (3, 5)] {
            let view = workspace.read_file("window.txt", start, Some(end)).unwrap();
            assert!(
                text.contains(&view.content),
                "{text:?} {start}..{end}: {:?}",
                view.content
            );
            assert_eq!(view.total_lines, text.lines().count());
            assert_eq!(
                view.content.lines().count(),
                text.lines()
                    .skip(start - 1)
                    .take(end - start + 1)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .lines()
                    .count()
            );
        }
    }
}

#[test]
fn diagnostic_context_cjk_separators_preserve_real_paths() {
    let anchors = query_anchors("修复：src/worker.rs:120，检查：源码/模型.rs#L9；结束");
    assert_eq!(anchors.len(), 2);
    assert_eq!(
        (anchors[0].path.as_str(), anchors[0].line),
        ("src/worker.rs", Some(120))
    );
    assert_eq!(
        (anchors[1].path.as_str(), anchors[1].line),
        ("源码/模型.rs", Some(9))
    );
    assert!(query_anchors("https://example.org/src/worker.rs:120").is_empty());
}

#[test]
fn diagnostic_context_relative_paths_keep_leading_dots() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for (input, expected) in [
        (".hidden.rs", ".hidden.rs"),
        ("./.hidden.rs", ".hidden.rs"),
        ("././src/file.rs", "src/file.rs"),
        (".env", ".env"),
    ] {
        assert_eq!(
            relative_anchor(&workspace, input).as_deref(),
            Some(expected),
            "{input}"
        );
    }
    for input in ["../outside.rs", "./../outside.rs", "src/../../outside.rs"] {
        assert!(relative_anchor(&workspace, input).is_none(), "{input}");
    }
}

#[test]
fn diagnostic_context_anchor_snippets_keep_exact_prefix_and_range() {
    let root = tempfile::tempdir().unwrap();
    let source = format!(
        "// header\n{}",
        format!("// {}\n", "原文🚀".repeat(100)).repeat(20)
    );
    std::fs::write(root.path().join("failure.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let original = workspace.read_file("failure.rs", 2, Some(14)).unwrap();
    let records = retrieve(
        &harness,
        "demo",
        &workspace,
        "failure.rs:2",
        &mut Vec::new(),
    )
    .unwrap();
    let body = &records[0]["source"]["body"];
    let text = body["content"].as_str().unwrap();
    assert!(
        original.content.starts_with(text),
        "diagnostic excerpts must never invent an ellipsis"
    );
    assert!(text.chars().count() <= MAX_ANCHOR_CHARS);
    assert_eq!(
        body["end_line"].as_u64(),
        Some(1 + text.lines().count() as u64)
    );
    assert_eq!(body["truncated"], true);
    assert_eq!(records[0]["source"]["sha256"], original.sha256);
}

#[test]
fn explicit_location_anchors_follow_canonical_language_variants() {
    let anchors = query_anchors(
        "src/app.mjs:7 types/api.mts#L9 views/index.phtml:4 Gemfile:2 ignored.unknown:8",
    );
    assert_eq!(
        anchors
            .iter()
            .map(|anchor| (anchor.path.as_str(), anchor.line))
            .collect::<Vec<_>>(),
        [
            ("src/app.mjs", Some(7)),
            ("types/api.mts", Some(9)),
            ("views/index.phtml", Some(4)),
            ("Gemfile", Some(2)),
        ]
    );
}

#[test]
fn explicit_location_anchors_keep_supported_auxiliary_files() {
    let anchors = query_anchors("deno.jsonc:3 schema.proto:8 component.vue:12 notes.unknown:4");
    assert_eq!(anchors.len(), 3);
    assert_eq!(anchors[0].path, "deno.jsonc");
    assert_eq!(anchors[1].path, "schema.proto");
    assert_eq!(anchors[2].path, "component.vue");
}
