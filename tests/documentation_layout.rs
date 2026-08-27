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

        let is_index = matches!(
            page.relative.to_string_lossy().as_ref(),
            "README.md" | "README.zh-CN.md"
        );
        if !is_index {
            assert!(
                index.contains(&format!("({route})")),
                "language index must link to {:?}",
                page.relative
            );
        }
    }

    let english_site = fs::read_to_string(root.join("docs/index.html")).unwrap();
    let chinese_site = fs::read_to_string(root.join("docs/zh/index.html")).unwrap();
    assert!(english_site.contains("href=\"./docs/\""));
    assert!(chinese_site.contains("href=\"../zh/docs/\""));
    assert!(!english_site.contains(">WIKI"));
    assert!(!chinese_site.contains(">WIKI"));

    let layout = fs::read_to_string(root.join("docs/_layouts/docs.html")).unwrap();
    assert!(layout.contains("page.lang == 'zh-CN'"));
    assert!(layout.contains("page.alternate"));
    assert!(layout.contains("'/zh/docs/'"));
    assert!(layout.contains("'/docs/reference/'"));

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
