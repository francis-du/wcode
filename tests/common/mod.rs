use std::fs;
use std::path::{Path, PathBuf};

/// Every Markdown file under `directory`, recursively.
pub fn markdown_files(directory: &Path) -> Vec<PathBuf> {
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
