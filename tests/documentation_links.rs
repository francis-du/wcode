use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

mod common;

use common::markdown_files;

fn collect_published_files(directory: &Path, base: &Path, published: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_published_files(&path, base, published);
        } else {
            let relative = path.strip_prefix(base).unwrap();
            published.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Collapse `.`/`..` and drop empty segments so a permalink and a raw asset
/// path compare equal. The result has no leading slash.
fn normalize_url(url: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in url.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

fn resolve_url(base: &str, target: &str) -> String {
    if target.starts_with('/') {
        return normalize_url(target);
    }
    let directory = base.rsplit_once('/').map(|(head, _)| head).unwrap_or("");
    normalize_url(&format!("{directory}/{target}"))
}

/// A published link may name a file, a directory index, or a page whose
/// permalink differs from its source path.
fn is_published_target(published: &BTreeSet<String>, resolved: &str) -> bool {
    ["", "/README.md", "/index.md", "/index.html"]
        .iter()
        .any(|suffix| published.contains(&format!("{resolved}{suffix}")))
}

/// The link target of every `](...)` in the document, without trimming code
/// fences: the manual does not embed markdown links in code samples.
fn markdown_link_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut index = 0;
    while let Some(offset) = content[index..].find("](") {
        let start = index + offset + 2;
        let Some(end) = content[start..].find(')') else {
            break;
        };
        targets.push(content[start..start + end].trim().to_owned());
        index = start + end + 1;
    }
    targets
}

fn declared_permalink(content: &str) -> Option<&str> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("permalink: "))
        .map(|p| p.trim())
}

fn page_url(content: &str, relative: &Path) -> String {
    if let Some(permalink) = declared_permalink(content) {
        return permalink.to_owned();
    }
    let without_extension = relative.with_extension("");
    let text = without_extension.to_string_lossy().replace('\\', "/");
    // A directory index is served from its directory, so links inside it
    // resolve against the directory rather than against the index file.
    if relative
        .file_name()
        .is_some_and(|name| matches!(name.to_string_lossy().as_ref(), "README.md" | "index.md"))
    {
        return match text.rsplit_once('/') {
            Some((head, _)) => format!("{head}/"),
            None => text,
        };
    }
    format!("{text}/")
}

/// GitHub/Jekyll heading anchor: lowercase, spaces to `-`, and anything that is
/// not a letter, digit, `-` or `_` dropped (including CJK punctuation).
fn heading_anchor(heading: &str) -> String {
    let mut anchor = String::new();
    for character in heading.trim().chars() {
        if character.is_alphanumeric() {
            anchor.push(character.to_ascii_lowercase());
        } else if character.is_whitespace() {
            anchor.push('-');
        } else if matches!(character, '-' | '_') {
            anchor.push(character);
        }
    }
    anchor
}

/// Whether the page content exposes this anchor, either as a derived heading
/// slug or as an explicit `{#anchor}` override.
fn page_has_anchor(content: &str, anchor: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        let hashes = trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count();
        if !(1..=6).contains(&hashes) || !trimmed[hashes..].starts_with(' ') {
            return false;
        }
        let heading = trimmed[hashes..].trim();
        let explicit = heading.rfind("{#").and_then(|start| {
            heading[start + 2..]
                .find('}')
                .map(|end| &heading[start + 2..start + 2 + end])
        });
        let explicit = explicit.map(|id| id.trim().to_ascii_lowercase());
        explicit.as_deref() == Some(anchor) || heading_anchor(heading) == anchor
    })
}

#[test]
fn published_documentation_links_resolve_to_pages_or_assets() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    let mut published = BTreeSet::new();
    collect_published_files(&docs_root, &docs_root, &mut published);
    let mut pages: BTreeMap<String, String> = BTreeMap::new();
    for page in markdown_files(&docs_root) {
        let relative = page.strip_prefix(&docs_root).unwrap();
        let content = fs::read_to_string(&page).unwrap();
        let stem = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let source_key = normalize_url(&stem);
        pages.insert(source_key, content.clone());
        if let Some(permalink) = declared_permalink(&content) {
            published.insert(normalize_url(permalink));
            pages
                .entry(normalize_url(permalink))
                .or_default()
                .clone_from(&content);
        }
    }
    let mut broken = Vec::new();
    for page in markdown_files(&docs_root) {
        let relative = page.strip_prefix(&docs_root).unwrap().to_path_buf();
        let content = fs::read_to_string(&page).unwrap();
        let base = page_url(&content, &relative);
        for target in markdown_link_targets(&content) {
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
                || target.starts_with("{{")
            {
                continue;
            }
            let (path, fragment) = match target.split_once('#') {
                Some((path, fragment)) => (path, Some(fragment)),
                None => (target.as_str(), None),
            };
            let resolved = if path.is_empty() {
                normalize_url(&base)
            } else {
                resolve_url(&base, path)
            };
            if !is_published_target(&published, &resolved) {
                broken.push(format!("{} -> {target} ({resolved})", relative.display()));
                continue;
            }
            if let Some(anchor) = fragment
                .filter(|fragment| !fragment.is_empty())
                .map(heading_anchor)
            {
                if !pages
                    .get(&resolved)
                    .is_some_and(|page| page_has_anchor(page, &anchor))
                {
                    broken.push(format!(
                        "{} -> {target} (no matching heading on {resolved})",
                        relative.display()
                    ));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "broken published links:\n{}",
        broken.join("\n")
    );
}

