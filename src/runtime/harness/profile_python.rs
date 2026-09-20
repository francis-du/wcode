use super::*;

const PYTHON_MANIFESTS: &[&str] = &[
    "pyproject.toml",
    "setup.cfg",
    "tox.ini",
    "requirements.txt",
    "requirements-dev.txt",
    "requirements-test.txt",
];
const MAX_UNITTEST_FILES: usize = 256;
const MAX_UNITTEST_DEPTH: usize = 4;

pub(super) fn add_python_checks(root: &Path, checks: &mut Vec<CheckSpec>) {
    if pytest_declared(root) {
        push_check(
            checks,
            "python-tests",
            "full",
            "pytest",
            &["-q"],
            "Run the repository-declared pytest suite with concise output.",
        );
    } else if unittest_declared(root) {
        push_check(
            checks,
            "python-unittest",
            "full",
            "python3",
            &["-m", "unittest", "discover"],
            "Run the repository-declared Python stdlib unittest suite.",
        );
    }
}

fn pytest_declared(root: &Path) -> bool {
    if root.join("pytest.ini").is_file() || root.join("conftest.py").is_file() {
        return true;
    }
    PYTHON_MANIFESTS.iter().any(|name| {
        read_small_text(&root.join(name))
            .is_some_and(|content| content.to_ascii_lowercase().contains("pytest"))
    })
}

fn unittest_declared(root: &Path) -> bool {
    let markers = ["import unittest", "from unittest", "unittest.TestCase"];
    root_test_files(root, MAX_UNITTEST_FILES, MAX_UNITTEST_DEPTH)
        .into_iter()
        .filter_map(|path| read_small_text(&path))
        .any(|content| markers.iter().any(|marker| content.contains(marker)))
}

fn root_test_files(root: &Path, max_files: usize, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if files.len() == max_files {
                break;
            }
            let path = entry.path();
            if path.is_file() && python_test_file(&path) {
                files.push(path);
            }
        }
    }

    let tests = root.join("tests");
    if !tests.is_dir() || files.len() == max_files {
        return files;
    }
    let mut pending = vec![(tests, 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if files.len() == max_files {
                return files;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_file() && python_test_file(&path) {
                files.push(path);
            } else if file_type.is_dir() && depth < max_depth {
                pending.push((path, depth + 1));
            }
        }
    }
    files
}

fn python_test_file(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("py")
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("test") || name.ends_with("_test.py"))
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/profile_python.rs"]
mod tests;
