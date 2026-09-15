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
fn complete_help_documentation_exposes_discovery_and_execution_boundaries() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/manual");
    let current_release = format!("releases/v{}", env!("CARGO_PKG_VERSION"));
    for suffix in ["", ".zh-CN"] {
        for page in ["reference", "getting-started", current_release.as_str()] {
            let content = fs::read_to_string(docs.join(format!("{page}{suffix}.md"))).unwrap();
            for command in [
                "wcode help-all",
                "wcode help-all setup",
                "wcode help-all --json",
            ] {
                assert!(
                    content.contains(command),
                    "{page}{suffix}: missing {command}"
                );
            }
        }
        let reference = fs::read_to_string(docs.join(format!("reference{suffix}.md"))).unwrap();
        for term in [
            "--plan",
            "--no-install-chatgpt",
            "schema_version: 1",
            "runtime_started: false",
        ] {
            assert!(reference.contains(term), "{suffix}: missing {term}");
        }
    }
}

#[test]
fn current_release_metadata_and_bilingual_notes_match_the_package() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let package = lock["package"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str() == Some("wcode"))
        .unwrap();
    assert_eq!(package["version"].as_str(), Some(version));
    for path in [
        "marketplace.json",
        "plugin/marketplace.json",
        "plugin/plugin.json",
        "plugin/.claude-plugin/plugin.json",
        "plugin/.codex-plugin/plugin.json",
        "plugin/.zcode-plugin/plugin.json",
    ] {
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(path)).unwrap()).unwrap();
        let actual = if path.ends_with("marketplace.json") {
            &manifest["plugins"][0]["version"]
        } else {
            &manifest["version"]
        };
        assert_eq!(actual.as_str(), Some(version), "version mismatch in {path}");
    }
    let docs = root.join("docs/manual");
    for suffix in ["", ".zh-CN"] {
        let index = fs::read_to_string(docs.join(format!("README{suffix}.md"))).unwrap();
        let releases = fs::read_to_string(docs.join(format!("releases{suffix}.md"))).unwrap();
        assert!(index.contains(&format!("releases/v{version}/")));
        assert!(releases.contains(&format!("(v{version}/)")));
        let notes =
            fs::read_to_string(docs.join(format!("releases/v{version}{suffix}.md"))).unwrap();
        for required in [
            version,
            "agent_context",
            "timed_out",
            "output_incomplete",
            "balanced",
            "fast",
            "light",
            "Node 24",
            "--read-only --no-exec --no-semantic",
            "cargo test --locked",
            "cargo clippy --locked --all-targets -- -D warnings",
        ] {
            assert!(
                notes.contains(required),
                "{suffix}: release notes missing {required}"
            );
        }
    }
}

#[test]
fn release_061_workflow_docs_keep_parallel_and_context_contracts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/manual");
    for suffix in ["", ".zh-CN"] {
        let index = fs::read_to_string(root.join(format!("README{suffix}.md"))).unwrap();
        assert!(index.contains("releases/v0.6.1/"));
        let releases = fs::read_to_string(root.join(format!("releases{suffix}.md"))).unwrap();
        assert!(releases.contains("(v0.6.1/)"));
        let getting_started =
            fs::read_to_string(root.join(format!("getting-started{suffix}.md"))).unwrap();
        for term in [
            "agent_context",
            "project_context",
            "parallel_tools",
            "OVERVIEW",
        ] {
            assert!(getting_started.contains(term));
        }
        assert!(!getting_started.contains("four runtime counters"));
        assert!(!getting_started.contains("四项运行指标"));
        let notes = fs::read_to_string(root.join(format!("releases/v0.6.1{suffix}.md"))).unwrap();
        for term in [
            "0.6.1",
            "parallel_tools",
            "project_context",
            "128",
            "cargo test --locked",
        ] {
            assert!(notes.contains(term));
        }
    }
}

#[test]
fn research_workflow_docs_keep_boundaries_and_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/manual");
    for suffix in ["", ".zh-CN"] {
        let index = fs::read_to_string(root.join(format!("README{suffix}.md"))).unwrap();
        assert!(index.contains("research-upgrades/"));
        let page = fs::read_to_string(root.join(format!("research-upgrades{suffix}.md"))).unwrap();
        for required in [
            "agent_context",
            "dry_run",
            "tasks_executed: 0",
            "authorization_checked: false",
            "file_preconditions_checked: false",
            "cargo clippy --locked --all-targets -- -D warnings",
            "2607.24882",
            "2609.08371",
            "2508.21433",
            "2607.27250",
            "2312.04511",
        ] {
            assert!(page.contains(required), "{suffix}: missing {required}");
        }
    }
}

#[test]
fn frontier_research_docs_distinguish_implementation_from_roadmap() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/manual");
    for suffix in ["", ".zh-CN"] {
        let index = fs::read_to_string(root.join(format!("README{suffix}.md"))).unwrap();
        assert!(index.contains("frontier-engineering/"));
        let page =
            fs::read_to_string(root.join(format!("frontier-engineering{suffix}.md"))).unwrap();
        for required in [
            "repo_map.deferred",
            "no checks executed",
            "32",
            "2601.16746",
            "2603.17829",
            "2603.20432",
            "2609.00006",
            "2512.08296",
            "2609.08149",
            "cargo clippy --locked --all-targets -- -D warnings",
        ] {
            assert!(page.contains(required), "{suffix}: missing {required}");
        }
        assert!(page.contains(if suffix.is_empty() {
            "proposed roadmap, not an implemented feature list"
        } else {
            "建议路线图，不是已经实现的功能列表"
        }));
    }
}

#[test]
fn configuration_docs_keep_presets_preview_and_local_setup_in_sync() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/manual");
    for suffix in ["", ".zh-CN"] {
        let reference = fs::read_to_string(root.join(format!("reference{suffix}.md"))).unwrap();
        for term in [
            "balanced",
            "fast",
            "light",
            "512 MiB",
            "1024 MiB",
            "256 MiB",
            "--show-config",
            "--max-memory-mb",
            "/setup/status",
            "--read-only",
            "wcode setup --performance fast --dry-run",
            "setup_launch",
            "--read-only --no-exec --no-semantic",
        ] {
            assert!(
                reference.contains(term),
                "{suffix}: missing configuration contract {term}"
            );
        }
        let started = fs::read_to_string(root.join(format!("getting-started{suffix}.md"))).unwrap();
        for term in [
            "wcode setup",
            "wcode mcp-stdio --performance fast",
            "wcode --show-config",
        ] {
            assert!(
                started.contains(term),
                "{suffix}: missing setup example {term}"
            );
        }
    }
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
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let homepage_en = fs::read_to_string(root.join("docs/index.html")).unwrap();
    let homepage_zh = fs::read_to_string(root.join("docs/zh/index.html")).unwrap();
    let intelligence_en = fs::read_to_string(docs_root.join("software-intelligence.md")).unwrap();
    let intelligence_zh =
        fs::read_to_string(docs_root.join("software-intelligence.zh-CN.md")).unwrap();
    for phrase in [
        "Make any coding agent understand your repo before it changes it.",
        "engineering control plane for coding agents",
        "UNDERSTAND",
        "Engineering Observatory",
        "Understand first. Change less. Prove it works. Learn only from proof.",
    ] {
        assert!(
            readme.contains(phrase),
            "README must lead with concrete agent value: {phrase}"
        );
    }
    for phrase in [
        "Engineering Control Plane",
        "REPOSITORY INTELLIGENCE",
        "Repository understanding is still the bottleneck",
        "Stop paying the context tax",
        "verification and evidence",
    ] {
        assert!(
            homepage_en.contains(phrase),
            "English homepage must explain the engineering-control-plane value story: {phrase}"
        );
    }
    for phrase in [
        "Engineering Control Plane",
        "仓库理解",
        "目标架构",
        "真实 LSP 语义关系",
        "验证证据",
        "可观测",
    ] {
        assert!(
            homepage_zh.contains(phrase),
            "Chinese homepage must keep the complete engineering-control-plane value story: {phrase}"
        );
    }
    assert!(intelligence_en.contains("Repository Intelligence & Engineering State"));
    assert!(intelligence_en.contains("The 60-second repository-intelligence model"));
    assert!(intelligence_en.contains("Engineering Observatory"));
    assert!(intelligence_en.contains("What will this change touch?"));
    assert!(intelligence_zh.contains("仓库理解与工程状态"));
    assert!(intelligence_zh.contains("60 秒理解仓库与工程状态"));
    assert!(intelligence_zh.contains("Engineering Observatory"));
    assert!(intelligence_zh.contains("这次修改会碰到什么？"));
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
    let development_en = fs::read_to_string(docs_root.join("development.md")).unwrap();
    let development_zh = fs::read_to_string(docs_root.join("development.zh-CN.md")).unwrap();
    for development in [&development_en, &development_zh] {
        for current_path in [
            "src/app/",
            "src/runtime/harness/",
            "src/runtime/tunnel/",
            "src/integrations/mcp/",
            "src/integrations/auth/",
            "src/ui/monitor/",
        ] {
            assert!(
                development.contains(current_path),
                "maintainer docs must describe the current module layout: {current_path}"
            );
        }
        for obsolete_path in [
            "src/runtime/tunnel.rs",
            "harness_agent_context.rs",
            "mcp_stdio.rs",
            "monitor_state.rs",
        ] {
            assert!(
                !development.contains(obsolete_path),
                "maintainer docs must not point agents at obsolete module paths: {obsolete_path}"
            );
        }
    }
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
                "covers",
                "Rscript --vanilla",
                "Standard Ruby",
                "Deno",
                "Flutter",
                "HTMLHint",
                "clang-tidy",
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
                "Repository Intelligence",
                "Engineering Observatory",
                "Design State",
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
        assert!(
            site.contains("id=\"performance\""),
            "both homepages must explain bounded parallel/resource behavior"
        );
        assert!(
            site.contains("id=\"languages\""),
            "both homepages must expose the polyglot quality model"
        );
        assert!(
            site.contains("language-quality/"),
            "both homepages must link to the canonical language-quality documentation"
        );
        assert!(!site.contains("updated Sep 1, 2026"));
        assert!(!site.contains("更新于 2026-09-01"));
        assert!(!site.contains("v0.5.2 · Cleaner MCP calls"));
        assert!(!site.contains("v0.5.2 · 更干净的 MCP 调用"));
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
    assert!(!site_js.contains("bind MCP to the source repository"));
    assert!(!site_js.contains("explicit repository binding"));
    assert!(!site_js.contains("把 MCP 绑定到源码仓库"));
    assert!(!site_js.contains("显式绑定当前仓库"));
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

    let integrations_en = fs::read_to_string(docs_root.join("code-agent-integrations.md")).unwrap();
    let integrations_zh =
        fs::read_to_string(docs_root.join("code-agent-integrations.zh-CN.md")).unwrap();
    assert!(integrations_en.contains("Global (recommended)"));
    assert!(integrations_en.contains("Host working directory is the default Workspace"));
    assert!(integrations_zh.contains("全局（推荐）"));
    assert!(integrations_zh.contains("Host 启动进程时的当前目录就是默认 Workspace"));

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
