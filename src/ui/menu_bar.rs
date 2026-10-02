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

fn companion_allowed(enabled: bool, macos: bool, ci: bool, remote_session: bool) -> bool {
    enabled && macos && !ci && !remote_session
}

#[cfg(any(target_os = "macos", test))]
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
    if !companion_allowed(enabled, cfg!(target_os = "macos"), ci, remote_session) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
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
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("native menu bar is currently implemented on macOS; use wcode menu-bar --json for the portable status projection");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::fs::TryLockError;
    use std::fs::{self, File, OpenOptions};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
        Icon, TrayIcon, TrayIconBuilder,
    };
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
        window::WindowId,
    };

    const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

    pub(super) struct InstanceLock {
        _file: File,
    }

    impl Drop for InstanceLock {
        fn drop(&mut self) {
            // A concurrent fork can inherit this open file description until exec.
            // Its lifetime must not extend the menu bar owner's exclusive lock.
            let _ = self._file.unlock();
        }
    }

    impl InstanceLock {
        #[cfg(test)]
        pub(super) fn clone_file_for_test(&self) -> std::io::Result<File> {
            self._file.try_clone()
        }

        fn acquire() -> Result<Option<Self>> {
            Self::acquire_at(&crate::auth::authority_state_root()?)
        }

        pub(super) fn acquire_at(authority: &Path) -> Result<Option<Self>> {
            if !authority.is_absolute() {
                anyhow::bail!("menu bar state directory must be absolute");
            }
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(authority)?;
            let directory = fs::symlink_metadata(authority)?;
            // Never chmod an existing path supplied through WCODE_STATE_DIR.
            let uid = unsafe { libc::geteuid() };
            if !directory.is_dir()
                || directory.file_type().is_symlink()
                || directory.uid() != uid
                || directory.mode() & 0o022 != 0
            {
                anyhow::bail!(
                    "menu bar state directory must be owned by you and not writable by others"
                );
            }
            let path = authority.join("menu-bar.lock");
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)?;
            let validate = || -> Result<()> {
                let held = file.metadata()?;
                let named = fs::symlink_metadata(&path)?;
                let current_directory = fs::symlink_metadata(authority)?;
                if !held.is_file()
                    || held.len() != 0
                    || held.nlink() != 1
                    || held.uid() != uid
                    || held.mode() & 0o077 != 0
                    || named.file_type().is_symlink()
                    || held.ino() != named.ino()
                    || held.dev() != named.dev()
                    || directory.ino() != current_directory.ino()
                    || directory.dev() != current_directory.dev()
                    || current_directory.mode() & 0o022 != 0
                {
                    anyhow::bail!("menu bar lock is unsafe or changed during startup");
                }
                Ok(())
            };
            validate()?;
            match file.try_lock() {
                Ok(()) => {
                    if let Err(error) = validate() {
                        let _ = file.unlock();
                        return Err(error);
                    }
                    Ok(Some(Self { _file: file }))
                }
                Err(TryLockError::WouldBlock) => Ok(None),
                Err(TryLockError::Error(error)) => Err(error.into()),
            }
        }
    }

    struct MenuState {
        tray: TrayIcon,
        runtime_item: MenuItem,
        transport_item: MenuItem,
        latest_item: MenuItem,
        workspace_item: MenuItem,
        mcp_item: MenuItem,
        task_item: MenuItem,
        verification_item: MenuItem,
        job_item: MenuItem,
        coverage_item: MenuItem,
        refresh_id: MenuId,
        quit_id: MenuId,
    }

    impl MenuState {
        fn new() -> Result<Self> {
            let menu = Menu::new();
            let runtime_item = MenuItem::new("Runtimes · loading", false, None);
            let transport_item = MenuItem::new("HTTP 0 · stdio 0", false, None);
            let latest_item = MenuItem::new("Latest · unknown", false, None);
            let workspace_item = MenuItem::new("Workspace · unknown", false, None);
            let mcp_item = MenuItem::new("MCP · unknown", false, None);
            let task_item = MenuItem::new("Tasks · unknown", false, None);
            let verification_item = MenuItem::new("Verification · unknown", false, None);
            let job_item = MenuItem::new("Jobs · unknown", false, None);
            let coverage_item = MenuItem::new("Coverage · unknown", false, None);
            let refresh = MenuItem::new("Refresh", true, None);
            let quit = MenuItem::new("Quit Menu Bar", true, None);
            menu.append_items(&[
                &runtime_item,
                &transport_item,
                &latest_item,
                &workspace_item,
                &mcp_item,
                &task_item,
                &verification_item,
                &job_item,
                &coverage_item,
                &PredefinedMenuItem::separator(),
                &refresh,
                &PredefinedMenuItem::separator(),
                &quit,
            ])?;
            let refresh_id = refresh.id().clone();
            let quit_id = quit.id().clone();
            let tray = TrayIconBuilder::new()
                .with_tooltip("WCode runtime status")
                .with_icon(status_icon()?)
                .with_icon_as_template(true)
                .with_menu(Box::new(menu))
                .build()?;
            Ok(Self {
                tray,
                runtime_item,
                transport_item,
                latest_item,
                workspace_item,
                mcp_item,
                task_item,
                verification_item,
                job_item,
                coverage_item,
                refresh_id,
                quit_id,
            })
        }
    }

    struct App {
        menu: Option<MenuState>,
        startup_error: Option<String>,
        next_refresh: Instant,
    }

    impl App {
        fn new() -> Self {
            Self {
                menu: None,
                startup_error: None,
                next_refresh: Instant::now(),
            }
        }

        fn install_menu(&mut self) -> Result<()> {
            if self.menu.is_none() {
                self.menu = Some(MenuState::new()?);
            }
            Ok(())
        }

        fn refresh(&mut self) {
            let Some(menu) = self.menu.as_mut() else {
                return;
            };
            match runtime_presence::snapshot() {
                Ok(snapshot) => {
                    let summary = MenuBarSummary::from_snapshot(&snapshot);
                    menu.runtime_item.set_text(format!(
                        "Runtimes · {} · {}",
                        summary.runtime_count, summary.state
                    ));
                    menu.transport_item.set_text(format!(
                        "HTTP {} · stdio {}",
                        summary.http_runtimes, summary.stdio_runtimes
                    ));
                    if let Some(record) = snapshot.records.first() {
                        menu.latest_item.set_text(format!(
                            "Latest · {} · v{}",
                            transport_label(record.transport),
                            record.version
                        ));
                        menu.workspace_item.set_text(format!(
                            "Workspace · {}",
                            workspace_label(&record.workspaces)
                        ));
                    } else {
                        menu.latest_item.set_text("Latest · none");
                        menu.workspace_item.set_text("Workspace · none");
                    }
                    let last_mcp_seen = snapshot
                        .records
                        .iter()
                        .filter_map(|record| record.last_mcp_seen_seconds_ago)
                        .min()
                        .map(|seconds| format!("{seconds}s ago"))
                        .unwrap_or_else(|| "never".to_owned());
                    menu.mcp_item.set_text(format!(
                        "MCP · {} connected · last {}",
                        summary.mcp_connected, last_mcp_seen
                    ));
                    menu.task_item.set_text(format!(
                        "Tasks · {} active · {} queued",
                        summary.active_tasks, summary.queued_tasks
                    ));
                    menu.verification_item.set_text(activity_label(
                        "Verification",
                        summary.active_verifications,
                        summary.queued_verifications,
                    ));
                    menu.job_item.set_text(activity_label(
                        "Jobs",
                        summary.active_jobs,
                        summary.queued_jobs,
                    ));
                    menu.coverage_item.set_text(if snapshot.partial {
                        format!(
                            "Coverage · partial · {} invalid · {} stale",
                            snapshot.invalid_records, snapshot.stale_records
                        )
                    } else if snapshot.records.is_empty() {
                        "Coverage · no active runtime".to_owned()
                    } else {
                        "Coverage · current".to_owned()
                    });
                    let title = if summary.active_tasks > 0 {
                        Some(format!(" {}", summary.active_tasks))
                    } else {
                        None
                    };
                    menu.tray.set_title(title);
                    let _ = menu.tray.set_tooltip(Some(format!(
                        "WCode · {} · {} runtime(s) · {} MCP connected",
                        summary.state, summary.runtime_count, summary.mcp_connected
                    )));
                }
                Err(_) => {
                    menu.runtime_item.set_text("Runtimes · unknown");
                    menu.transport_item.set_text("HTTP ? · stdio ?");
                    menu.latest_item.set_text("Latest · unknown");
                    menu.workspace_item.set_text("Workspace · unknown");
                    menu.mcp_item.set_text("MCP · unknown");
                    menu.task_item.set_text("Tasks · unknown");
                    menu.verification_item.set_text("Verification · unknown");
                    menu.job_item.set_text("Jobs · unknown");
                    menu.coverage_item
                        .set_text("Coverage · runtime state unavailable");
                }
            }
            self.next_refresh = Instant::now() + REFRESH_INTERVAL;
        }

        fn handle_menu(&mut self, event_loop: &ActiveEventLoop) {
            let Some(menu) = self.menu.as_ref() else {
                return;
            };
            let refresh_id = menu.refresh_id.clone();
            let quit_id = menu.quit_id.clone();
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == quit_id {
                    event_loop.exit();
                } else if event.id == refresh_id {
                    self.refresh();
                }
            }
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if let Err(error) = self.install_menu() {
                self.startup_error = Some(error.to_string());
                event_loop.exit();
                return;
            }
            self.refresh();
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_refresh));
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            _event: WindowEvent,
        ) {
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.handle_menu(event_loop);
            if Instant::now() >= self.next_refresh {
                self.refresh();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_refresh));
        }
    }

    pub(super) fn run() -> Result<()> {
        let Some(_instance_lock) = InstanceLock::acquire()? else {
            return Ok(());
        };
        let mut builder = EventLoop::builder();
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false)
            .with_activate_ignoring_other_apps(false);
        let event_loop = builder.build()?;
        let mut app = App::new();
        event_loop.run_app(&mut app)?;
        if let Some(error) = app.startup_error {
            anyhow::bail!("cannot start native menu bar: {error}");
        }
        Ok(())
    }

    fn activity_label(label: &str, active: Option<u64>, queued: Option<u64>) -> String {
        match (active, queued) {
            (Some(active), Some(queued)) => {
                format!("{label} · {active} active · {queued} queued")
            }
            _ => format!("{label} · unknown"),
        }
    }

    fn transport_label(transport: RuntimeTransport) -> &'static str {
        match transport {
            RuntimeTransport::Http => "HTTP",
            RuntimeTransport::Stdio => "stdio",
        }
    }

    fn workspace_label(workspaces: &[String]) -> String {
        if workspaces.is_empty() {
            return "none".to_owned();
        }
        let mut labels = workspaces
            .iter()
            .take(2)
            .map(|workspace| {
                let mut value = workspace.chars().take(32).collect::<String>();
                if workspace.chars().count() > 32 {
                    value.push('…');
                }
                value
            })
            .collect::<Vec<_>>();
        if workspaces.len() > labels.len() {
            labels.push(format!("+{}", workspaces.len() - labels.len()));
        }
        labels.join(", ")
    }

    fn status_icon() -> Result<Icon> {
        let width = 18_u32;
        let height = 18_u32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let dx = i64::from(x) - 8;
                let dy = i64::from(y) - 8;
                let ring = (dx * dx + dy * dy) <= 49;
                let inner = (dx * dx + dy * dy) <= 16;
                let alpha = if ring && !inner {
                    255
                } else if inner {
                    120
                } else {
                    0
                };
                rgba.extend_from_slice(&[255, 255, 255, alpha]);
            }
        }
        Icon::from_rgba(rgba, width, height).map_err(Into::into)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ui/menu_bar.rs"]
mod tests;
