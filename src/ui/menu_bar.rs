use crate::runtime_presence::{self, RuntimePresenceSnapshot, RuntimeTransport};
use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct MenuBarSummary {
    pub runtime_count: usize,
    pub http_runtimes: usize,
    pub stdio_runtimes: usize,
    pub mcp_connected: usize,
    pub active_tasks: u64,
    pub queued_tasks: u64,
    pub active_verifications: Option<u64>,
    pub queued_verifications: Option<u64>,
    pub active_jobs: Option<u64>,
    pub queued_jobs: Option<u64>,
    pub partial: bool,
    pub state: &'static str,
}

impl MenuBarSummary {
    fn known_sum(
        snapshot: &RuntimePresenceSnapshot,
        field: impl Fn(&crate::runtime_presence::RuntimePresenceRecord) -> Option<u64>,
    ) -> Option<u64> {
        if snapshot.partial {
            return None;
        }
        snapshot.records.iter().try_fold(0_u64, |total, record| {
            Some(total.saturating_add(field(record)?))
        })
    }

    fn from_snapshot(snapshot: &RuntimePresenceSnapshot) -> Self {
        let http_runtimes = snapshot
            .records
            .iter()
            .filter(|record| record.transport == RuntimeTransport::Http)
            .count();
        let stdio_runtimes = snapshot
            .records
            .iter()
            .filter(|record| record.transport == RuntimeTransport::Stdio)
            .count();
        let mcp_connected = snapshot
            .records
            .iter()
            .filter(|record| record.mcp_connected)
            .count();
        let active_tasks = snapshot.records.iter().fold(0_u64, |total, record| {
            total.saturating_add(record.active_tasks)
        });
        let queued_tasks = snapshot.records.iter().fold(0_u64, |total, record| {
            total.saturating_add(record.queued_tasks)
        });
        let active_verifications = Self::known_sum(snapshot, |record| record.active_verifications);
        let queued_verifications = Self::known_sum(snapshot, |record| record.queued_verifications);
        let active_jobs = Self::known_sum(snapshot, |record| record.active_jobs);
        let queued_jobs = Self::known_sum(snapshot, |record| record.queued_jobs);
        let state = if snapshot.records.is_empty() {
            if snapshot.partial {
                "unknown"
            } else {
                "offline"
            }
        } else if snapshot.partial {
            "partial"
        } else if active_tasks > 0
            || queued_tasks > 0
            || [
                active_verifications,
                queued_verifications,
                active_jobs,
                queued_jobs,
            ]
            .into_iter()
            .flatten()
            .any(|count| count > 0)
        {
            "working"
        } else if mcp_connected > 0 {
            "connected"
        } else {
            "idle"
        };
        Self {
            runtime_count: snapshot.records.len(),
            http_runtimes,
            stdio_runtimes,
            mcp_connected,
            active_tasks,
            queued_tasks,
            active_verifications,
            queued_verifications,
            active_jobs,
            queued_jobs,
            partial: snapshot.partial,
            state,
        }
    }
}

fn companion_allowed(
    enabled: bool,
    graphical_session: bool,
    ci: bool,
    remote_session: bool,
) -> bool {
    enabled && graphical_session && !ci && !remote_session
}

fn graphical_session() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(target_os = "windows")]
    {
        windows::graphical_session()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn companion_error_tail(mut input: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(4096);
    let mut buffer = [0; 1024];
    loop {
        let count = match input.read(&mut buffer) {
            Ok(0) => return Ok(tail),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > 4096 {
            tail.drain(..tail.len() - 4096);
        }
    }
}

pub(crate) fn launch_companion(enabled: bool) -> Result<()> {
    let ci = std::env::var("CI").is_ok_and(|value| !matches!(value.as_str(), "" | "0" | "false"));
    let remote_session = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    if !companion_allowed(enabled, graphical_session(), ci, remote_session) {
        return Ok(());
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        use std::process::{Command, Stdio};
        let executable = std::env::current_exe()?;
        // A dedicated reaper never blocks the async runtime's shutdown. The child
        // is started inside this thread, so thread-creation failure leaves no child.
        std::thread::Builder::new()
            .name("wcode-menu-bar".into())
            .spawn(move || {
                let result = Command::new(executable)
                    .arg("menu-bar")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    // Never let a long-lived companion hold an MCP client's pipes open.
                    .stderr(Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        let detail = child.stderr.take().map(companion_error_tail).transpose();
                        let status = child.wait()?;
                        Ok((status, detail?.unwrap_or_default()))
                    });
                match result {
                    Ok((status, _)) if status.success() => {}
                    Ok((status, detail)) => eprintln!(
                        "WCode menu bar exited: {status}: {}",
                        String::from_utf8_lossy(&detail).trim()
                    ),
                    Err(error) => eprintln!("WCode menu bar unavailable: {error}"),
                }
            })?;
    }
    Ok(())
}

pub(crate) fn run(json: bool) -> Result<()> {
    if json {
        let snapshot = runtime_presence::snapshot()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "menu_bar_status",
                "summary": MenuBarSummary::from_snapshot(&snapshot),
                "presence": snapshot,
                "observation_only": true,
            }))?
        );
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        macos::run()
    }
    #[cfg(target_os = "windows")]
    {
        windows::run()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("native menu bar is currently implemented on macOS and Windows; use wcode menu-bar --json for the portable status projection");
    }
}

#[cfg(target_os = "macos")]
#[path = "menu_bar/macos.rs"]
mod macos;
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[path = "menu_bar/native.rs"]
mod native;
#[cfg(target_os = "windows")]
#[path = "menu_bar/windows.rs"]
mod windows;

#[cfg(test)]
#[path = "../../tests/unit/ui/menu_bar.rs"]
mod tests;
