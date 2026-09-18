use ignore::{DirEntry, WalkBuilder};
use std::path::Path;

pub(crate) fn repository_ignore_builder(start: &Path, honor_parent_ignores: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(start);
    builder
        .hidden(false)
        .follow_links(false)
        .parents(honor_parent_ignores)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .sort_by_file_path(|left, right| left.cmp(right));
    builder
}

pub(crate) fn repository_walk_builder(start: &Path, honor_parent_ignores: bool) -> WalkBuilder {
    let mut builder = repository_ignore_builder(start, honor_parent_ignores);
    builder.filter_entry(source_visible_entry);
    builder
}

fn source_visible_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_some_and(|kind| kind.is_symlink()) {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    if super::reject_protected_path(Path::new(name)).is_err() {
        return false;
    }
    if entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return !generated_directory(name);
    }
    !(name == ".DS_Store"
        || name.ends_with(".log")
        || (name.starts_with(".wcode-") && name.ends_with(".tmp")))
}

pub(crate) fn generated_directory(name: &str) -> bool {
    matches!(
        name,
        ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
            | "build"
            | "dist"
            | "coverage"
            | ".dart_tool"
            | ".build"
            | ".gradle"
            | ".swiftpm"
            | "ephemeral"
            | "Pods"
            | ".symlinks"
            | ".plugin_symlinks"
            | ".next"
            | ".cache"
            | "DerivedData"
            | ".venv"
            | "venv"
            | "__pycache__"
            | "vendor"
    )
}
