use super::*;
use std::env;

pub(super) fn trusted_provider_path(workspace: &Workspace, candidate: &Path) -> Option<PathBuf> {
    let executable = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        env::current_dir().ok()?.join(candidate)
    };
    let canonical = executable.canonicalize().ok()?;
    (!canonical.starts_with(workspace.root())).then_some(executable)
}

pub(super) fn find_executable(workspace: &Workspace, name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.components().count() > 1 && candidate.is_file() {
        return trusted_provider_path(workspace, &candidate);
    }
    if let Some(path) = find_executable_on_path(workspace, name) {
        return Some(path);
    }
    known_language_tool_paths(name)
        .into_iter()
        .find_map(|candidate| {
            candidate
                .is_file()
                .then(|| trusted_provider_path(workspace, &candidate))
                .flatten()
        })
}

fn find_executable_on_path(workspace: &Workspace, name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    #[cfg(windows)]
    let extensions = env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".CMD".into(), ".BAT".into()]);
    for directory in env::split_paths(&path) {
        let plain = directory.join(name);
        if plain.is_file() {
            if let Some(path) = trusted_provider_path(workspace, &plain) {
                return Some(path);
            }
        }
        #[cfg(windows)]
        for extension in &extensions {
            let with_extension = directory.join(format!("{name}{extension}"));
            if with_extension.is_file() {
                if let Some(path) = trusted_provider_path(workspace, &with_extension) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn known_language_tool_paths(name: &str) -> Vec<PathBuf> {
    known_language_tool_paths_from(
        name,
        env::var_os("GOBIN"),
        env::var_os("GOPATH"),
        env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }),
    )
}

pub(super) fn known_language_tool_paths_from(
    name: &str,
    gobin: Option<std::ffi::OsString>,
    gopath: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if name == "gopls" {
        if let Some(gobin) = gobin.filter(|value| !value.is_empty()) {
            paths.push(PathBuf::from(gobin).join(executable_name(name)));
        }
        if let Some(gopath) = gopath.filter(|value| !value.is_empty()) {
            for root in env::split_paths(&gopath) {
                paths.push(root.join("bin").join(executable_name(name)));
            }
        }
        if let Some(home) = home.filter(|value| !value.is_empty()) {
            paths.push(
                PathBuf::from(home)
                    .join("go/bin")
                    .join(executable_name(name)),
            );
        }
    }
    paths
}

pub(super) fn executable_name(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub(super) fn executable_discovery_source(
    workspace: &Workspace,
    executable: &Path,
) -> &'static str {
    let name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_owned();
    let discovered_on_path = find_executable_on_path(workspace, &name)
        .as_ref()
        .is_some_and(|path| paths_equal(path, executable));
    if discovered_on_path {
        "trusted_path"
    } else {
        "language_tool_directory"
    }
}

#[cfg(windows)]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().to_ascii_lowercase() == right.to_string_lossy().to_ascii_lowercase()
}

#[cfg(not(windows))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}
