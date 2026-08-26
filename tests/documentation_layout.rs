use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn documentation_is_canonical_and_hosted_as_html() {
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
        "maintained Markdown belongs under docs/wiki, not directly under docs"
    );

    let wiki_root = root.join("docs/wiki");
    let index = fs::read_to_string(wiki_root.join("README.md")).unwrap();
    for page in markdown_files(&wiki_root) {
        let relative = page.strip_prefix(&wiki_root).unwrap();
        if relative == Path::new("README.md") {
            continue;
        }
        let content = fs::read_to_string(&page).unwrap();
        let permalink = content
            .lines()
            .find_map(|line| line.strip_prefix("permalink: "))
            .unwrap_or_else(|| panic!("{relative:?} must declare its HTML permalink"));
        let route = permalink
            .strip_prefix("/wiki/")
            .unwrap_or_else(|| panic!("{relative:?} must render below /wiki/"));
        assert!(
            index.contains(&format!("({route})")),
            "docs/wiki/README.md must link to {relative:?}"
        );
    }

    let english_site = fs::read_to_string(root.join("docs/index.html")).unwrap();
    let chinese_site = fs::read_to_string(root.join("docs/zh/index.html")).unwrap();
    assert!(english_site.contains("href=\"./wiki/\""));
    assert!(chinese_site.contains("href=\"../wiki/\""));
    assert!(!english_site.contains(".md\""));
    assert!(!chinese_site.contains(".md\""));

    let workflow = fs::read_to_string(root.join(".github/workflows/pages.yml")).unwrap();
    assert!(workflow.contains("actions/jekyll-build-pages@v1"));
    assert!(workflow.contains("source: ./docs"));
    assert!(workflow.contains("path: _site"));
}

fn markdown_names(directory: &Path) -> BTreeSet<String> {
    fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

fn markdown_files(directory: &Path) -> Vec<std::path::PathBuf> {
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
