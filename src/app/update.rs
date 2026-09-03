use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

#[cfg(unix)]
const UNIX_INSTALLER: &str = include_str!("../../install.sh");
#[cfg(windows)]
const WINDOWS_INSTALLER: &str = include_str!("../../install.ps1");
const RECONNECT_NOTICE: &str = "Reconnect your MCP Host to load the new WCode binary; existing stdio sessions keep running the previous version.";

pub(super) fn run() -> Result<()> {
    let executable = env::current_exe().context("cannot locate the running wcode executable")?;
    let install_dir = install_dir(&executable, env::var_os("WCODE_INSTALL_DIR"))?;
    run_platform(&install_dir)
}

#[cfg(unix)]
fn run_platform(install_dir: &Path) -> Result<()> {
    run_unix(install_dir)
}

#[cfg(windows)]
fn run_platform(install_dir: &Path) -> Result<()> {
    run_windows(install_dir)
}

#[cfg(not(any(unix, windows)))]
fn run_platform(_install_dir: &Path) -> Result<()> {
    bail!("wcode update is supported on Unix-like systems and Windows")
}

fn install_dir(executable: &Path, override_dir: Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(path) = override_dir {
        if path.is_empty() {
            bail!("WCODE_INSTALL_DIR cannot be empty");
        }
        return Ok(PathBuf::from(path));
    }
    executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("cannot determine the current wcode installation directory"))
}

fn temporary_script(extension: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "wcode-update-{}-{}.{}",
        std::process::id(),
        Uuid::new_v4().simple(),
        extension
    ))
}

#[cfg(unix)]
fn run_unix(install_dir: &Path) -> Result<()> {
    let script = temporary_script("sh");
    fs::write(&script, UNIX_INSTALLER).context("cannot stage the embedded wcode installer")?;
    let status = Command::new("sh")
        .arg(&script)
        .env("WCODE_INSTALL_DIR", install_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cannot run the embedded wcode installer")?;
    let _ = fs::remove_file(&script);
    if !status.success() {
        bail!("wcode update failed; the existing executable was left in place")
    }
    println!("WCode updated successfully. {RECONNECT_NOTICE}");
    Ok(())
}

#[cfg(windows)]
fn run_windows(install_dir: &Path) -> Result<()> {
    let script = temporary_script("ps1");
    fs::write(&script, WINDOWS_INSTALLER)
        .context("cannot stage the embedded wcode PowerShell installer")?;
    let script_literal = powershell_literal(&script.to_string_lossy());
    let parent_pid = std::process::id();
    let helper = format!(
        "$parent={parent_pid}; while (Get-Process -Id $parent -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 100 }}; try {{ & {script_literal} }} finally {{ Remove-Item -LiteralPath {script_literal} -Force -ErrorAction SilentlyContinue }}"
    );
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &helper,
        ])
        .env("WCODE_INSTALL_DIR", install_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("cannot start the Windows wcode update helper")?;
    println!(
        "WCode update is ready; replacement begins after this process exits. {RECONNECT_NOTICE}"
    );
    Ok(())
}

#[cfg(windows)]
fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
#[path = "../../tests/unit/app/update.rs"]
mod tests;
