use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct DocPage {
    relative: PathBuf,
    lang: String,
    permalink: String,
    alternate: String,
}

#[test]
fn documentation_is_unified_bilingual_and_hosted_as_html() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let root_markdown = markdown_names(root);
    assert!(root_markdown.contains("README.md"));
    assert!(
        root_markdown
            .iter()
            .all(|name| matches!(name.as_str(), "AGENTS.md" | "README.md")),
        "project documentation must not be scattered across the repository root"
    );
    assert!(
        markdown_names(&root.join("docs")).is_empty(),
        "maintained Markdown belongs under docs/manual, not directly under docs"
    );

    let docs_root = root.join("docs/manual");
    let english_index = fs::read_to_string(docs_root.join("README.md")).unwrap();
    let chinese_index = fs::read_to_string(docs_root.join("README.zh-CN.md")).unwrap();
    let integration_en = fs::read_to_string(docs_root.join("code-agent-integrations.md")).unwrap();
    let integration_zh =
        fs::read_to_string(docs_root.join("code-agent-integrations.zh-CN.md")).unwrap();
    for integration in [&integration_en, &integration_zh] {
        for required in [
            "agent_context",
            "symbol_context",
            "apply_edits",
            "review_changes",
            "verify_project",
            "defer_loading",
            "wcode setup",
            "parallelism",
        ] {
            assert!(
                integration.contains(required),
                "both integration guides must preserve the compact 0.4 coding path and deferred-loading guidance: {required}"
            );
        }
    }
    let reference_en = fs::read_to_string(docs_root.join("reference.md")).unwrap();
    let reference_zh = fs::read_to_string(docs_root.join("reference.zh-CN.md")).unwrap();
    let releases_en = fs::read_to_string(docs_root.join("releases.md")).unwrap();
    let releases_zh = fs::read_to_string(docs_root.join("releases.zh-CN.md")).unwrap();
    assert!(reference_en.contains("agent_context(goal, scopes=...)"));
    assert!(reference_zh.contains("agent_context(goal, scopes=...)"));
    assert!(reference_en.contains("wcode setup"));
    assert!(reference_zh.contains("wcode setup"));
    assert!(releases_en.contains("(v0.5.2/)"));
    assert!(releases_zh.contains("(v0.5.2/)"));
    let security_en = fs::read_to_string(docs_root.join("security.md")).unwrap();
    let security_zh = fs::read_to_string(docs_root.join("security.zh-CN.md")).unwrap();
    for document in [&security_en, &security_zh, &reference_en, &reference_zh] {
        for tool in [
            "gh",
            "just",
            "task",
            "uv",
            "ruff",
            "biome",
            "deno",
            "docker",
            "kubectl",
            "terraform",
            "fd",
            "jq",
            "cmake",
            "ninja",
            "dotnet",
            "mvn",
            "gradle",
            "swift",
            "zig",
            "pre-commit",
            "act",
        ] {
            assert!(
                document.contains(&format!("`{tool}`")),
                "EN/ZH security and reference docs must expose the same bounded development tool catalog: {tool}"
            );
        }
        assert!(document.contains("cargo-nextest"));
        assert!(document.contains("cargo test"));
    }
    let pages = markdown_files(&docs_root)
        .into_iter()
        .map(|path| parse_page(&docs_root, path))
        .collect::<Vec<_>>();
    let by_route = pages
        .iter()
        .map(|page| (page.permalink.as_str(), page))
        .collect::<BTreeMap<_, _>>();

    for page in &pages {
        let (prefix, other_prefix, index) = match page.lang.as_str() {
            "en" => ("/docs/", "/zh/docs/", &english_index),
            "zh-CN" => ("/zh/docs/", "/docs/", &chinese_index),
            other => panic!("{:?} has unsupported lang {other}", page.relative),
        };
        let route = page
            .permalink
            .strip_prefix(prefix)
            .unwrap_or_else(|| panic!("{:?} must render below {prefix}", page.relative));
        assert!(
            page.alternate.starts_with(other_prefix),
            "{:?} alternate must point to the other language tree",
            page.relative
        );
        let counterpart = by_route.get(page.alternate.as_str()).unwrap_or_else(|| {
            panic!(
                "{:?} alternate route {} has no matching page",
                page.relative, page.alternate
            )
        });
        assert_eq!(
            counterpart.alternate, page.permalink,
            "language alternates must point back to each other"
        );

        let page_content = fs::read_to_string(docs_root.join(&page.relative)).unwrap();
        let counterpart_content =
            fs::read_to_string(docs_root.join(&counterpart.relative)).unwrap();
        assert_eq!(
            top_level_section_count(&page_content),
            top_level_section_count(&counterpart_content),
            "bilingual pages must keep the same top-level section structure: {:?} <-> {:?}",
            page.relative,
            counterpart.relative
        );

        let relative = page.relative.to_string_lossy().replace('\\', "/");
        let is_index = matches!(relative.as_str(), "README.md" | "README.zh-CN.md");
        if !is_index {
            let (navigation_index, navigation_route) = if relative.starts_with("releases/v") {
                (
                    if page.lang == "zh-CN" {
                        &releases_zh
                    } else {
                        &releases_en
                    },
                    route.strip_prefix("releases/").unwrap_or(route),
                )
            } else {
                (index, route)
            };
            assert!(
                navigation_index.contains(&format!("({navigation_route})")),
                "the appropriate documentation index must link to {:?}",
                page.relative
            );
        }
    }

    for (base, required) in [
        (
            "agentic-engineering",
            &[
                "agent_context",
                "symbol_context",
                "review_changes",
                "verify_project",
                "evidence_status",
                "product scope",
                "deterministic gate",
            ][..],
        ),
        (
            "language-quality",
            &[
                "language_quality_status",
                "language_quality_run",
                "check_only",
                "property",
                "mutation",
                "fuzz",
                "runtime_canary",
            ][..],
        ),
        (
            "maintainability-review",
            &[
                "maintainability-file-crossed-1k",
                "maintainability-concentrated-growth",
                "maintainability-cross-scope-churn",
                "maintainability_review",
            ][..],
        ),
        (
            "product-scopes",
            &[
                "runtime",
                "integrations",
                "workspace",
                "design",
                "graph",
                "semantics",
                "traceability",
                "risk",
                "verification",
                "evidence",
                "reconciliation",
                "experience",
                "agent_context",
            ][..],
        ),
        (
            "software-intelligence",
            &[
                "agent_context",
                "architecture-first",
                "observed drift",
                "evidence coverage",
                "implementation coverage",
                "semantic_provider_refresh",
                "verification_execute_stages",
            ][..],
        ),
        (
            "releases/v0.4.0",
            &[
                "agent_context",
                "architecture-first",
                "cargo-nextest",
                "localhost.run",
                "pinggy",
                "riskyexecution",
            ][..],
        ),
        (
            "releases/v0.5.2",
            &[
                "wcode setup",
                "input_required",
                "parallelism",
                "target-aware",
                "wcode update",
            ][..],
        ),
        (
            "releases/v0.5.0",
            &[
                "semantic_navigation",
                "warm session",
                "rust-analyzer",
                "didchange",
                "canonical",
                "gopls",
                "jdtls",
                "fallbacks",
                "launch_ready",
                "session_validated",
                "--no-semantic",
                "riskyexecution",
            ][..],
        ),
    ] {
        assert_bilingual_tokens(&docs_root, base, required);
    }

    let english_site = fs::read_to_string(root.join("docs/index.html")).unwrap();
    let chinese_site = fs::read_to_string(root.join("docs/zh/index.html")).unwrap();
    assert!(english_site.contains("href=\"./docs/\""));
    assert!(chinese_site.contains("href=\"../zh/docs/\""));
    assert!(!english_site.contains(">WIKI"));
    assert!(!chinese_site.contains(">WIKI"));

    for site in [&english_site, &chinese_site] {
        assert!(site.contains("id=\"clientGrid\""));
        assert!(site.contains("id=\"clientSearch\""));
        assert!(site.contains("id=\"sourceList\""));
        for filter in ["all", "auto", "manual", "cli", "ide", "web"] {
            assert!(
                site.contains(&format!("data-filter=\"{filter}\"")),
                "both language homepages must expose the same client filters"
            );
        }
    }
    let site_js = fs::read_to_string(root.join("docs/assets/site.js")).unwrap();
    assert!(site_js.contains("const capabilityLabels = pageIsChinese"));
    assert!(site_js.contains("function renderCapability(key, value)"));
    assert!(site_js.contains("pageIsChinese ? `厂商依据 ${index + 1}`"));
    for capability in [
        "package: '插件包'",
        "skill: '通用 Skill'",
        "stdio: 'stdio'",
        "http: 'HTTP'",
        "sse: 'SSE'",
        "oauth: 'OAuth'",
        "auto: '一键安装'",
        "manual: '仅手工'",
    ] {
        assert!(
            site_js.contains(capability),
            "website client matrix must keep separate capability: {capability}"
        );
    }
    assert!(!chinese_site.contains("Documentation"));
    assert!(!chinese_site.contains("CLI/MCP Reference"));

    let release_installer =
        "curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh";
    assert!(english_site.contains(release_installer));
    assert!(chinese_site.contains(release_installer));
    assert!(!english_site.contains("cargo install --path ."));
    assert!(!chinese_site.contains("cargo install --path ."));
    assert!(!fs::read_to_string(root.join("README.md"))
        .unwrap()
        .contains("cargo install --path ."));
    for path in markdown_files(&docs_root) {
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("cargo install --path ."),
            "user documentation must install releases through the installer script: {path:?}"
        );
    }

    let layout = fs::read_to_string(root.join("docs/_layouts/docs.html")).unwrap();
    assert!(layout.contains("page.lang == 'zh-CN'"));
    assert!(layout.contains("page.alternate"));
    assert!(layout.contains("'/zh/docs/'"));
    assert!(layout.contains("'/docs/reference/'"));
    assert!(layout.contains("'/docs/releases/'"));
    assert!(layout.contains("'/zh/docs/releases/'"));
    assert!(
        !layout.contains("'/docs/releases/v0.5.1/'"),
        "release versions belong in the release index, not the global sidebar"
    );

    let docs_css = fs::read_to_string(root.join("docs/assets/docs.css")).unwrap();
    assert!(docs_css.contains(".docs-shell"));
    assert!(docs_css.contains(".docs-sidebar"));
    assert!(docs_css.contains(".docs-content"));

    let legacy_index = fs::read_to_string(root.join("docs/wiki/index.html")).unwrap();
    assert!(legacy_index.contains("/docs/"));
    let legacy_zh_index = fs::read_to_string(root.join("docs/zh/wiki/index.html")).unwrap();
    assert!(legacy_zh_index.contains("/zh/docs/"));

    let workflow = fs::read_to_string(root.join(".github/workflows/pages.yml")).unwrap();
    assert!(workflow.contains("actions/jekyll-build-pages@v1"));
    assert!(workflow.contains("source: ./docs"));
    assert!(workflow.contains("path: _site"));
}

fn top_level_section_count(content: &str) -> usize {
    content
        .lines()
        .filter(|line| line.starts_with("## "))
        .count()
}

fn assert_bilingual_tokens(docs_root: &Path, base: &str, required: &[&str]) {
    let english = fs::read_to_string(docs_root.join(format!("{base}.md"))).unwrap();
    let chinese = fs::read_to_string(docs_root.join(format!("{base}.zh-CN.md"))).unwrap();
    let english = english.to_ascii_lowercase();
    let chinese = chinese.to_ascii_lowercase();
    for token in required {
        let token = token.to_ascii_lowercase();
        assert!(
            english.contains(&token),
            "English {base} must contain bilingual contract token: {token}"
        );
        assert!(
            chinese.contains(&token),
            "Chinese {base} must contain bilingual contract token: {token}"
        );
    }
}

fn parse_page(docs_root: &Path, path: PathBuf) -> DocPage {
    let relative = path.strip_prefix(docs_root).unwrap().to_path_buf();
    let content = fs::read_to_string(&path).unwrap();
    let field = |name: &str| {
        content
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}: ")))
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("{relative:?} must declare {name}"))
    };
    let lang = field("lang");
    let permalink = field("permalink");
    let alternate = field("alternate");
    DocPage {
        relative,
        lang,
        permalink,
        alternate,
    }
}

fn markdown_names(directory: &Path) -> BTreeSet<String> {
    fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

fn markdown_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            files.extend(markdown_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    files
}
