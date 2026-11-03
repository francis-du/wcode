use super::*;
use std::time::{Duration, Instant};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

pub(super) const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

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

pub(super) fn run<L>(instance_lock: Option<L>) -> Result<()> {
    let Some(_instance_lock) = instance_lock else {
        return Ok(());
    };
    let event_loop = build_event_loop()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.startup_error {
        anyhow::bail!("cannot start native menu bar: {error}");
    }
    Ok(())
}

fn build_event_loop() -> Result<EventLoop<()>> {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut builder = EventLoop::builder();
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false)
            .with_activate_ignoring_other_apps(false);
        builder.build().map_err(Into::into)
    }
    #[cfg(not(target_os = "macos"))]
    {
        EventLoop::builder().build().map_err(Into::into)
    }
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
