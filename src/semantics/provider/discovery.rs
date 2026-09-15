use super::*;
use std::collections::HashMap;
use std::env;
use std::io::Read;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

const RUSTUP_COMPONENT_CACHE_TTL: StdDuration = StdDuration::from_secs(30);
const RUSTUP_COMPONENT_PROBE_TIMEOUT: StdDuration = StdDuration::from_secs(2);
type RustupComponentCacheKey = (PathBuf, PathBuf);
type RustupComponentCache = HashMap<RustupComponentCacheKey, (Instant, bool)>;
static RUSTUP_COMPONENT_CACHE: OnceLock<Mutex<RustupComponentCache>> = OnceLock::new();

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
        return trusted_provider_path(workspace, &candidate)
            .filter(|path| provider_executable_ready(workspace, name, path));
    }
    if let Some(path) = find_executable_on_path(workspace, name) {
        if provider_executable_ready(workspace, name, &path) {
            return Some(path);
        }
    }
    known_language_tool_paths(name)
        .into_iter()
        .find_map(|candidate| {
            candidate
                .is_file()
                .then(|| trusted_provider_path(workspace, &candidate))
                .flatten()
                .filter(|path| provider_executable_ready(workspace, name, path))
        })
}

fn provider_executable_ready(workspace: &Workspace, name: &str, executable: &Path) -> bool {
    if name != "rust-analyzer" || !is_rustup_proxy(executable) {
        return true;
    }
    let key = (workspace.root().to_path_buf(), executable.to_path_buf());
    let cache = RUSTUP_COMPONENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some((checked_at, available)) = cache.get(&key) {
            if checked_at.elapsed() < RUSTUP_COMPONENT_CACHE_TTL {
                return *available;
            }
        }
    }
    let available = rustup_proxy_component_ready_uncached(workspace, executable);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, (Instant::now(), available));
    }
    available
}

pub(super) fn is_rustup_proxy(executable: &Path) -> bool {
    rustup_proxy_path(executable).is_some()
}

pub(super) fn rustup_proxy_path(executable: &Path) -> Option<PathBuf> {
    let canonical = executable.canonicalize().ok()?;
    if canonical
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("rustup"))
    {
        return Some(canonical);
    }
    let sibling = executable.parent()?.join(executable_name("rustup"));
    same_file_identity(executable, &sibling).then_some(sibling)
}

#[cfg(unix)]
fn same_file_identity(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(left), Ok(right)) = (std::fs::metadata(left), std::fs::metadata(right)) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(left: &Path, right: &Path) -> bool {
    matches!((left.canonicalize(), right.canonicalize()), (Ok(left), Ok(right)) if left == right)
}

pub(super) fn rustup_proxy_component_ready_uncached(
    workspace: &Workspace,
    executable: &Path,
) -> bool {
    let Some(rustup) = rustup_proxy_path(executable) else {
        return true;
    };
    let Ok(mut child) = StdCommand::new(rustup)
        .args(["which", "rust-analyzer"])
        .current_dir(workspace.root())
        .stdin(StdStdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::null())
        .spawn()
    else {
        return false;
    };
    // rustup is a local probe, but cold filesystem/toolchain startup can exceed
    // sub-second deadlines under a fully parallel verification run.
    let deadline = Instant::now() + RUSTUP_COMPONENT_PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(StdDuration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    };
    if !status.success() {
        return false;
    }
    let Some(mut stream) = child.stdout.take() else {
        return false;
    };
    let mut stdout = String::new();
    if stream.read_to_string(&mut stdout).is_err() {
        return false;
    }
    let resolved = stdout.trim().to_owned();
    !resolved.is_empty() && Path::new(&resolved).is_file()
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

pub(super) fn executable_discovery_source(executable: &Path) -> &'static str {
    let file_name = executable.file_name();
    let discovered_on_path = file_name.is_some_and(|file_name| {
        env::var_os("PATH").is_some_and(|path| {
            env::split_paths(&path)
                .map(|directory| directory.join(file_name))
                .any(|candidate| paths_equal(&candidate, executable))
        })
    });
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
