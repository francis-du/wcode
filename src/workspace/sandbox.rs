use anyhow::{bail, Context, Result};
use serde::Serialize;
#[cfg(any(target_os = "linux", test))]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

const MAX_PROTECTED_PATHS: usize = 256;
const MAX_PROTECTED_SCAN_ENTRIES: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SandboxBackend {
    #[cfg(target_os = "macos")]
    MacOsSandboxExec,
    #[cfg(target_os = "linux")]
    LinuxBubblewrap,
}

impl SandboxBackend {
    fn name(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOsSandboxExec => "macos_sandbox_exec",
            #[cfg(target_os = "linux")]
            Self::LinuxBubblewrap => "linux_bubblewrap",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExecutionSandboxStatus {
    pub broad_execution_requires_sandbox: bool,
    pub approval_independent: bool,
    pub fail_closed: bool,
    pub available: bool,
    pub backend: &'static str,
    pub filesystem: &'static str,
    pub network: &'static str,
}

pub(super) struct SandboxGuard {
    scratch: Option<PathBuf>,
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        if let Some(path) = self.scratch.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

pub(super) fn broad_execution_requires_sandbox(
    unrestricted_commands: bool,
    bounded_profile_admissible: bool,
) -> bool {
    unrestricted_commands && !bounded_profile_admissible
}

pub(super) fn command_requires_sandbox(
    unrestricted_commands: bool,
    _allow_exec: bool,
    _allow_write: bool,
    program: &str,
    args: &[String],
) -> bool {
    if !unrestricted_commands {
        return false;
    }
    // Full Access is the explicit authority to cross the base exec/write
    // switches. The strong sandbox is only for command shapes that still fall
    // outside wcode's bounded policy, never as a second authorization layer for
    // already validated Git/development operations.
    let policy_bypass = super::command_requires_broad_sandbox(program, args);
    broad_execution_requires_sandbox(unrestricted_commands, !policy_bypass)
}

pub(crate) fn status() -> ExecutionSandboxStatus {
    let backend = detect_backend();
    ExecutionSandboxStatus {
        broad_execution_requires_sandbox: true,
        approval_independent: true,
        fail_closed: true,
        available: backend.is_some(),
        backend: backend.map_or("unavailable", SandboxBackend::name),
        filesystem: "host_read_only_workspace_write_protected_paths_denied",
        network: "denied",
    }
}

pub(super) fn prepare(
    workspace_root: &Path,
    cwd: &Path,
    executable: &Path,
    args: &[String],
) -> Result<(Command, SandboxGuard)> {
    let backend = detect_backend().ok_or_else(|| {
        anyhow::anyhow!(
            "sandbox_unavailable: strong OS sandbox is unavailable; broad unrestricted command execution is denied"
        )
    })?;
    match backend {
        #[cfg(target_os = "macos")]
        SandboxBackend::MacOsSandboxExec => prepare_macos(workspace_root, cwd, executable, args),
        #[cfg(target_os = "linux")]
        SandboxBackend::LinuxBubblewrap => prepare_linux(workspace_root, cwd, executable, args),
    }
}

fn detect_backend() -> Option<SandboxBackend> {
    #[cfg(target_os = "macos")]
    {
        if executable_file(Path::new("/usr/bin/sandbox-exec")) {
            return Some(SandboxBackend::MacOsSandboxExec);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if trusted_bwrap_path().is_some() {
            return Some(SandboxBackend::LinuxBubblewrap);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn trusted_bwrap_path() -> Option<&'static Path> {
    ["/usr/bin/bwrap", "/bin/bwrap"]
        .into_iter()
        .map(Path::new)
        .find(|path| executable_file(path))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn rust_toolchain_home_overrides(executable: &Path) -> Vec<(&'static str, PathBuf)> {
    let name = executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .trim_end_matches(".exe");
    if !matches!(
        name,
        "cargo" | "cargo-clippy" | "clippy-driver" | "rustc" | "rustdoc" | "rustfmt" | "rustup"
    ) {
        return Vec::new();
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok())
    else {
        return Vec::new();
    };
    [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")]
        .into_iter()
        .filter_map(|(key, default)| {
            let candidate = std::env::var_os(key)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default));
            let candidate = candidate.canonicalize().ok()?;
            (candidate.is_dir() && candidate.starts_with(&home)).then_some((key, candidate))
        })
        .collect()
}

fn apply_rust_toolchain_home_overrides(command: &mut Command, executable: &Path) {
    for (key, path) in rust_toolchain_home_overrides(executable) {
        command.env(key, path);
    }
}

#[cfg(target_os = "macos")]
fn prepare_macos(
    workspace_root: &Path,
    cwd: &Path,
    executable: &Path,
    args: &[String],
) -> Result<(Command, SandboxGuard)> {
    let scratch = create_scratch()?;
    let guard = SandboxGuard {
        scratch: Some(scratch.clone()),
    };
    let profile = macos_profile(workspace_root, &scratch)?;
    let home = scratch.join("home");
    let temp = scratch.join("tmp");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&temp)?;

    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-p")
        .arg(profile)
        .arg(executable)
        .args(args)
        .current_dir(cwd)
        .env("HOME", &home)
        .env("TMPDIR", &temp)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("WCODE_SANDBOX", "1");
    apply_rust_toolchain_home_overrides(&mut command, executable);

    Ok((command, guard))
}

#[cfg(any(target_os = "macos", test))]
fn macos_profile(workspace_root: &Path, scratch: &Path) -> Result<String> {
    macos_profile_from_paths(
        workspace_root,
        scratch,
        &sandbox_protected_paths(workspace_root)?,
    )
}

#[cfg(any(target_os = "macos", test))]
fn macos_profile_from_paths(
    workspace_root: &Path,
    scratch: &Path,
    protected: &[PathBuf],
) -> Result<String> {
    let workspace = sandbox_string(workspace_root)?;
    let scratch = sandbox_string(scratch)?;
    let mut profile = format!(
        r#"(version 1)
(deny default)
(allow process*)
(allow sysctl-read)
(allow mach-lookup)
(allow ipc-posix-shm)
(allow file-read*)
(allow file-write*
    (subpath "{workspace}")
    (subpath "{scratch}")
    (literal "/dev/null"))
"#
    );
    for path in protected {
        let path = sandbox_string(path)?;
        profile.push_str(&format!(
            "(deny file-read* file-write* (literal \"{path}\"))\n(deny file-read* file-write* (subpath \"{path}\"))\n"
        ));
    }
    Ok(profile)
}

#[cfg(any(target_os = "macos", test))]
fn sandbox_string(path: &Path) -> Result<String> {
    let value = path.to_str().context("sandbox path is not valid UTF-8")?;
    if value.chars().any(char::is_control) {
        bail!("sandbox path contains control characters");
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn create_scratch() -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("wcode-sandbox-{}", Uuid::new_v4().simple()));
    fs::create_dir(&path)
        .with_context(|| format!("cannot create sandbox scratch {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn prepare_linux(
    workspace_root: &Path,
    cwd: &Path,
    executable: &Path,
    args: &[String],
) -> Result<(Command, SandboxGuard)> {
    #[cfg(target_os = "linux")]
    {
        let launcher = trusted_bwrap_path().ok_or_else(|| {
            anyhow::anyhow!(
                "sandbox_unavailable: bubblewrap is unavailable; broad unrestricted command execution is denied"
            )
        })?;
        let scratch = create_scratch()?;
        let guard = SandboxGuard {
            scratch: Some(scratch.clone()),
        };
        let home = scratch.join("home");
        let temp = scratch.join("tmp");
        let empty = scratch.join("empty");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&temp)?;
        fs::create_dir_all(&empty)?;
        let mut command = Command::new(launcher);
        command
            .args(linux_bwrap_prefix(workspace_root, cwd, &scratch)?)
            .arg("--")
            .arg(executable)
            .args(args)
            .env("HOME", &home)
            .env("TMPDIR", &temp)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("WCODE_SANDBOX", "1");
        apply_rust_toolchain_home_overrides(&mut command, executable);
        Ok((command, guard))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace_root, cwd, executable, args);
        bail!("sandbox_unavailable: Linux bubblewrap sandbox is unavailable on this platform")
    }
}

#[cfg(target_os = "linux")]
fn linux_bwrap_prefix(workspace_root: &Path, cwd: &Path, scratch: &Path) -> Result<Vec<OsString>> {
    linux_bwrap_prefix_from_paths(
        workspace_root,
        cwd,
        scratch,
        &sandbox_protected_paths(workspace_root)?,
    )
}

#[cfg(any(target_os = "linux", test))]
fn linux_bwrap_prefix_from_paths(
    workspace_root: &Path,
    cwd: &Path,
    scratch: &Path,
    protected: &[PathBuf],
) -> Result<Vec<OsString>> {
    let mut args = vec![
        "--die-with-parent".into(),
        "--new-session".into(),
        "--unshare-net".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--bind".into(),
        workspace_root.as_os_str().to_owned(),
        workspace_root.as_os_str().to_owned(),
        "--bind".into(),
        scratch.as_os_str().to_owned(),
        scratch.as_os_str().to_owned(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
    ];
    for path in protected {
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!(
                "sandbox_unavailable: cannot mask protected path {}",
                path.display()
            )
        })?;
        if metadata.is_dir() {
            args.extend([
                OsString::from("--ro-bind"),
                scratch.join("empty").into_os_string(),
                path.as_os_str().to_owned(),
            ]);
        } else if metadata.is_file() {
            args.extend([
                OsString::from("--ro-bind"),
                OsString::from("/dev/null"),
                path.as_os_str().to_owned(),
            ]);
        } else {
            bail!(
                "sandbox_unavailable: unsupported protected path {}",
                path.display()
            );
        }
    }
    args.extend([OsString::from("--chdir"), cwd.as_os_str().to_owned()]);
    Ok(args)
}

fn sandbox_protected_paths(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let roots = super::fs_safety::authority_roots()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    sandbox_protected_paths_with_roots(workspace_root, &roots, home.as_deref())
}

fn sandbox_protected_paths_with_roots(
    workspace_root: &Path,
    authority_roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    let workspace_root = workspace_root.canonicalize().with_context(|| {
        format!(
            "cannot canonicalize sandbox Workspace {}",
            workspace_root.display()
        )
    })?;
    let mut protected = authority_roots
        .iter()
        .map(|path| super::fs_safety::normalize_authority_root(path))
        .collect::<Result<Vec<_>>>()?;
    if protected
        .iter()
        .any(|path| workspace_root.starts_with(path))
    {
        bail!("sandbox_unavailable: Workspace is inside an authority-state root");
    }
    if protected.len() > MAX_PROTECTED_PATHS {
        bail!("sandbox_unavailable: authority-root set exceeds bounded capacity");
    }
    collect_protected_paths(&workspace_root, true, &mut protected)?;

    if let Some(home) = home {
        if let Ok(home) = home.canonicalize() {
            if home != workspace_root {
                // Outside the selected Workspace the host is read-only. Mask
                // direct user-home credential roots too, while keeping broad
                // execution cost independent from unrelated home-tree size.
                collect_protected_paths(&home, false, &mut protected)?;
            }
        }
    }
    protected.sort();
    protected.dedup();
    // Mount a parent once: masking an authority root already hides its child
    // stores, and a second bind inside the empty read-only mask cannot work.
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in protected {
        if !roots.iter().any(|parent| path.starts_with(parent)) {
            roots.push(path);
        }
    }
    Ok(roots)
}

fn collect_protected_paths(
    root: &Path,
    recursive: bool,
    protected: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).with_context(|| {
            format!(
                "sandbox cannot inspect protected paths under {}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "sandbox cannot inspect an entry under {}",
                    directory.display()
                )
            })?;
            scanned = scanned.saturating_add(1);
            if scanned > MAX_PROTECTED_SCAN_ENTRIES {
                bail!(
                    "sandbox_unavailable: protected-path scan exceeded the {MAX_PROTECTED_SCAN_ENTRIES}-entry safety bound"
                );
            }
            let path = entry.path();
            // Authority stores are identified by absolute resolver roots. Do
            // not inspect their contents while planning a command sandbox.
            if protected.iter().any(|root| path.starts_with(root)) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).with_context(|| {
                format!(
                    "sandbox cannot inspect protected path candidate {}",
                    path.display()
                )
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // Repository metadata inside the selected Workspace is part of
            // the mutation surface required by sandboxed VCS commands. It is
            // not a host credential root: keep it visible and avoid scanning
            // the VCS object database as protected-path input.
            if recursive && matches!(name.to_ascii_lowercase().as_str(), ".git" | ".hg" | ".svn") {
                continue;
            }
            if super::protected_component_kind(&name).is_some() {
                if metadata.file_type().is_symlink() {
                    let target = path.canonicalize().with_context(|| {
                        format!(
                            "sandbox cannot resolve protected symlink {}",
                            path.display()
                        )
                    })?;
                    protected.push(target);
                    if protected.len() > MAX_PROTECTED_PATHS {
                        bail!(
                            "sandbox_unavailable: protected-path set exceeds the {MAX_PROTECTED_PATHS}-path safety bound"
                        );
                    }
                    continue;
                }
                #[cfg(unix)]
                if metadata.is_file() {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() > 1 {
                        bail!(
                            "sandbox_unavailable: protected path {} has multiple hard links and cannot be safely masked",
                            path.display()
                        );
                    }
                }
                protected.push(path);
                if protected.len() > MAX_PROTECTED_PATHS {
                    bail!(
                        "sandbox_unavailable: protected-path set exceeds the {MAX_PROTECTED_PATHS}-path safety bound"
                    );
                }
                continue;
            }
            if recursive && metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/sandbox.rs"]
mod tests;
