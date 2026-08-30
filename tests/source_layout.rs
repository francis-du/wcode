use std::fs;
use std::path::{Path, PathBuf};

const MAX_RUST_FILE_LINES: usize = 1_000;
const MAX_RUST_FILE_STEM_CHARS: usize = 24;
const MAX_MAINTAINED_TEXT_LINES: usize = 1_000;
const MAX_REPOSITORY_FILE_NAME_CHARS: usize = 32;

#[test]
fn maintained_text_files_remain_bounded() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut oversized = Vec::new();
    for path in repository_files(&root) {
        if path.file_name().is_some_and(|name| name == "Cargo.lock") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let lines = physical_lines(&source);
        if lines > MAX_MAINTAINED_TEXT_LINES {
            oversized.push(format!(
                "{} ({lines} lines)",
                path.strip_prefix(&root).unwrap_or(&path).display()
            ));
        }
    }
    assert!(
        oversized.is_empty(),
        "maintained text files must stay at or below {MAX_MAINTAINED_TEXT_LINES} physical lines; generated Cargo.lock and binary assets are exempt:\n{}",
        oversized.join("\n")
    );
}

#[test]
fn repository_file_names_remain_concise() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut long_names = Vec::new();
    for path in repository_files(&root) {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("repository filenames must be valid UTF-8");
        if name.chars().count() > MAX_REPOSITORY_FILE_NAME_CHARS {
            long_names.push(
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
    assert!(
        long_names.is_empty(),
        "repository filenames must stay at or below {MAX_REPOSITORY_FILE_NAME_CHARS} characters and use directories for context:\n{}",
        long_names.join("\n")
    );
}

#[test]
fn rust_files_remain_bounded() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut oversized = Vec::new();
    for area in ["src", "tests"] {
        for path in rust_files(&root.join(area)) {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let lines = physical_lines(&source);
            if lines > MAX_RUST_FILE_LINES {
                oversized.push(format!(
                    "{} ({lines} lines)",
                    path.strip_prefix(&root).unwrap_or(&path).display()
                ));
            }
        }
    }
    assert!(
        oversized.is_empty(),
        "Rust files must stay at or below {MAX_RUST_FILE_LINES} physical lines; split by domain:\n{}",
        oversized.join("\n")
    );
}

#[test]
fn rust_file_names_remain_concise() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut long_names = Vec::new();
    for area in ["src", "tests"] {
        for path in rust_files(&root.join(area)) {
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .expect("Rust source names must be valid UTF-8");
            if stem.chars().count() > MAX_RUST_FILE_STEM_CHARS {
                long_names.push(
                    path.strip_prefix(&root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }
    }
    assert!(
        long_names.is_empty(),
        "Rust filenames must use their domain directory for context and stay at or below {MAX_RUST_FILE_STEM_CHARS} characters:\n{}",
        long_names.join("\n")
    );
}

#[test]
fn source_modules_attach_tests_from_the_root_test_tree() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut inline = Vec::new();
    for path in rust_files(&root.join("src")) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        if source.lines().any(|line| line.trim() == "mod tests {") {
            inline.push(
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
    assert!(
        inline.is_empty(),
        "move test bodies under tests/unit/<domain>/ and attach them with #[path]:\n{}",
        inline.join("\n")
    );
}

#[test]
fn rust_sources_parse_without_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar must load");
    let mut invalid = Vec::new();
    for area in ["src", "tests"] {
        for path in rust_files(&root.join(area)) {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let tree = parser
                .parse(&source, None)
                .unwrap_or_else(|| panic!("parser returned no tree for {}", path.display()));
            if tree.root_node().has_error() {
                invalid.push(
                    path.strip_prefix(&root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }
    }
    assert!(
        invalid.is_empty(),
        "Tree-sitter reported Rust parse errors:\n{}",
        invalid.join("\n")
    );
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files.sort();
    files
}

fn repository_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_repository_files(root, &mut files);
    files.sort();
    files
}

fn collect_repository_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("directory entry must be readable");
        let file_type = entry.file_type().expect("file type must be readable");
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name == ".git" || name == "target")
            {
                continue;
            }
            collect_repository_files(&path, files);
        } else if file_type.is_file() {
            files.push(path);
        }
    }
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("directory entry must be readable").path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn physical_lines(source: &str) -> usize {
    source.bytes().filter(|byte| *byte == b'\n').count()
        + usize::from(!source.is_empty() && !source.ends_with('\n'))
}
