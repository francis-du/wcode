use super::*;

impl TaskMonitor {
    pub fn new(workspaces: impl IntoIterator<Item = String>) -> Self {
        let workspaces = workspaces
            .into_iter()
            .map(|id| (id, WorkspaceStats::default()))
            .collect::<BTreeMap<_, _>>();
        Self {
            state: Arc::new(Mutex::new(MonitorState {
                next_id: 1,
                started_at: Instant::now(),
                intelligence: workspaces
                    .keys()
                    .cloned()
                    .map(|id| (id, IntelligenceStats::default()))
                    .collect(),
                workspaces,
                tasks: VecDeque::new(),
                traffic: VecDeque::new(),
                oauth_client_registered: false,
                oauth_authorized: false,
                chatgpt_connected: false,
                initialize_count: 0,
                last_initialize: None,
                last_mcp_seen: None,
                public_endpoint: None,
                tunnels: Vec::new(),
                public_url_healthy: None,
                public_url_last_checked: None,
                public_url_consecutive_failures: 0,
                public_url_error: None,
                tunnel_running: None,
                tunnel_error: None,
                active_total: 0,
                peak_active: 0,
                observed_active: 0,
                observed_queued: 0,
            })),
        }
    }

    pub fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> TaskTicket {
        self.queue_task(workspace, tool, detail, request_bytes, true)
    }

    pub fn queue_orchestration(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> TaskTicket {
        self.queue_task(workspace, tool, detail, request_bytes, false)
    }

    fn queue_task(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
        slot_counted: bool,
    ) -> TaskTicket {
        let workspace = workspace.into();
        let tool = tool.into();
        let detail = detail.into();
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        let stats = state.workspaces.entry(workspace.clone()).or_default();
        stats.queued = stats.queued.saturating_add(1);
        stats.calls = stats.calls.saturating_add(1);
        stats.request_bytes = stats.request_bytes.saturating_add(request_bytes);
        state.traffic.push_back(TrafficEvent {
            at: now,
            requests: 1,
            request_bytes,
            response_bytes: 0,
            context_bytes_avoided: 0,
        });
        state.tasks.push_back(TaskRecord {
            id,
            workspace,
            tool,
            detail,
            status: TaskStatus::Queued,
            slot_counted,
            queued_at: now,
            started_at: None,
            finished_at: None,
            request_bytes,
            response_bytes: 0,
            context_bytes_avoided: 0,
        });
        trim_history(&mut state, now);
        TaskTicket {
            monitor: self.clone(),
            id,
            finished: false,
        }
    }

    pub fn register_workspace(&self, workspace: impl Into<String>) {
        let workspace = workspace.into();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.workspaces.entry(workspace.clone()).or_default();
        state.intelligence.entry(workspace).or_default();
    }

    pub(crate) fn record_agent_context_metrics(
        &self,
        workspace: &str,
        model_bytes: u64,
        context_bytes_avoided: u64,
        repo_map_cache_hit: bool,
    ) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let stats = state.workspaces.entry(workspace.to_owned()).or_default();
        stats.agent_context_calls = stats.agent_context_calls.saturating_add(1);
        stats.agent_context_model_bytes =
            stats.agent_context_model_bytes.saturating_add(model_bytes);
        stats.agent_context_bytes_avoided = stats
            .agent_context_bytes_avoided
            .saturating_add(context_bytes_avoided);
        stats.agent_repo_map_cache_hits = stats
            .agent_repo_map_cache_hits
            .saturating_add(u64::from(repo_map_cache_hit));
    }

    pub fn spawn_renderer(&self, config: MonitorConfig, enabled: bool) -> Option<MonitorRenderer> {
        if !enabled || !io::stdout().is_terminal() {
            return None;
        }

        let monitor = self.clone();
        let (stop, stop_rx) = watch::channel(false);
        let (interrupt_tx, interrupted) = watch::channel(false);
        let join = tokio::task::spawn_blocking(move || {
            if let Err(error) = run_dashboard(monitor, config, stop_rx, interrupt_tx) {
                eprintln!("wcode dashboard stopped: {error}");
            }
        });
        Some(MonitorRenderer {
            stop,
            interrupted,
            join,
        })
    }

    pub fn mark_oauth_client_registered(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .oauth_client_registered = true;
    }

    pub fn mark_oauth_authorized(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .oauth_authorized = true;
    }

    pub fn mark_mcp_seen(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .last_mcp_seen = Some(Instant::now());
    }

    pub fn mark_mcp_connected(&self) {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.chatgpt_connected = true;
        state.last_mcp_seen = Some(now);
    }

    pub fn mark_mcp_initialized(&self) {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.chatgpt_connected = true;
        state.initialize_count = state.initialize_count.saturating_add(1);
        state.last_initialize = Some(now);
        state.last_mcp_seen = Some(now);
    }

    pub fn mark_public_url_check(&self, success: bool, error: Option<String>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.public_url_last_checked = Some(Instant::now());
        if success {
            state.public_url_healthy = Some(true);
            state.public_url_consecutive_failures = 0;
            state.public_url_error = None;
        } else {
            state.public_url_consecutive_failures =
                state.public_url_consecutive_failures.saturating_add(1);
            state.public_url_error = error;
            if state.public_url_consecutive_failures >= 3 {
                state.public_url_healthy = Some(false);
            }
        }
    }

    pub fn mark_public_endpoint(&self, mode: impl Into<String>, tunnel_running: Option<bool>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.public_endpoint = Some(mode.into());
        state.tunnel_running = tunnel_running;
        state.tunnel_error = None;
    }

    pub fn register_tunnel(&self, provider: &str, public_url: &str) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        if state
            .tunnels
            .iter()
            .any(|(existing_provider, url)| existing_provider == provider && url == public_url)
        {
            return;
        }
        state
            .tunnels
            .push((provider.to_owned(), public_url.to_owned()));
    }

    pub fn tunnel_links(&self) -> Vec<(String, String)> {
        let state = self.state.lock().expect("task monitor lock poisoned");
        state.tunnels.clone()
    }

    pub fn remove_tunnel(&self, public_url: &str) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.tunnels.retain(|(_, url)| url != public_url);
        if state.tunnels.is_empty() && state.public_endpoint.as_deref() == Some("quick-tunnel") {
            state.public_endpoint = Some("pending".to_owned());
        }
    }

    pub fn mark_tunnel_stopped(&self, error: impl Into<String>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.tunnel_running = Some(false);
        state.tunnel_error = Some(error.into());
    }

    pub fn connection_status(&self) -> MonitorConnectionStatus {
        let state = self.state.lock().expect("task monitor lock poisoned");
        MonitorConnectionStatus {
            oauth_client_registered: state.oauth_client_registered,
            oauth_authorized: state.oauth_authorized,
            chatgpt_initialized: state.chatgpt_connected,
            initialize_count: state.initialize_count,
            last_initialize_seconds_ago: state.last_initialize.map(|seen| seen.elapsed().as_secs()),
            last_mcp_seen_seconds_ago: state.last_mcp_seen.map(|seen| seen.elapsed().as_secs()),
            public_endpoint: state.public_endpoint.clone(),
            public_url_healthy: state.public_url_healthy,
            public_url_last_checked_seconds_ago: state
                .public_url_last_checked
                .map(|seen| seen.elapsed().as_secs()),
            public_url_consecutive_failures: state.public_url_consecutive_failures,
            public_url_error: state.public_url_error.clone(),
            tunnel_running: state.tunnel_running,
            tunnel_error: state.tunnel_error.clone(),
            active_tasks: state.active_total,
            queued_tasks: state.workspaces.values().map(|stats| stats.queued).sum(),
            peak_active_tasks: state.peak_active,
        }
    }

    pub fn record_intelligence_result(&self, workspace: &str, tool: &str, value: &Value) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(stats) = state.intelligence.get_mut(workspace) else {
            return;
        };
        match tool {
            "design_status" => {
                stats.design_state = Some(if value["valid"].as_bool() == Some(true) {
                    "valid".to_owned()
                } else if value["initialized"].as_bool() == Some(true) {
                    "invalid".to_owned()
                } else {
                    "uninitialized".to_owned()
                });
                stats.requirements = value["requirements"].as_u64().unwrap_or(0);
                stats.components = value["components"].as_u64().unwrap_or(0);
            }
            "scope_status" => {
                stats.scope_source_files = value["source_files"].as_u64().unwrap_or(0);
                stats.scope_mapped_files = value["mapped_files"].as_u64().unwrap_or(0);
                stats.scope_unmapped_files = stats
                    .scope_source_files
                    .saturating_sub(stats.scope_mapped_files);
            }
            "software_graph" => {
                stats.graph_nodes = value["node_count"].as_u64().unwrap_or(0);
                stats.graph_edges = value["edge_count"].as_u64().unwrap_or(0);
                stats.graph_precision = value["precision"].as_str().map(str::to_owned);
            }
            "graph_diff" => {
                stats.graph_added_nodes = value["added_node_count"].as_u64().unwrap_or(0);
                stats.graph_removed_nodes = value["removed_node_count"].as_u64().unwrap_or(0);
                stats.graph_changed_nodes = value["changed_node_count"].as_u64().unwrap_or(0);
                stats.graph_added_edges = value["added_edge_count"].as_u64().unwrap_or(0);
                stats.graph_removed_edges = value["removed_edge_count"].as_u64().unwrap_or(0);
                stats.graph_changed_edges = value["changed_edge_count"].as_u64().unwrap_or(0);
            }
            "traceability_status" => {
                stats.implementation_coverage =
                    value["design_to_implementation"]["percent"].as_u64();
                stats.verification_coverage =
                    value["acceptance_to_verification"]["percent"].as_u64();
            }
            "semantic_status" => {
                stats.semantic_confirmed = value["confirmed"].as_u64().unwrap_or(0);
                stats.semantic_candidates = value["candidates"].as_u64().unwrap_or(0);
            }
            "drift_status" => {
                stats.drift_findings = value["implementation_drift"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(value["design_drift"].as_u64().unwrap_or(0));
            }
            "risk_status" => {
                stats.risk_level = value["level"].as_str().map(str::to_owned);
                stats.drift_findings = value["drift"]["findings"]
                    .as_array()
                    .map(|findings| findings.len() as u64)
                    .unwrap_or(stats.drift_findings);
            }
            "evidence_status" => {
                stats.evidence_total = value["total"].as_u64().unwrap_or(0);
                stats.evidence_failed = value["failed"].as_u64().unwrap_or(0);
                stats.evidence_disagreed = value["disagreed"].as_u64().unwrap_or(0);
            }
            "verification_status" => {
                stats.verification_ready = value["ready"].as_bool();
                stats.verification_blockers = value["blockers"]
                    .as_array()
                    .map(|blockers| blockers.len() as u64)
                    .unwrap_or(0);
            }
            "reconciliation_execution_status" => {
                stats.reconciliation_converged = value["converged"].as_bool();
                stats.reconciliation_pending = value["pending"].as_u64().unwrap_or(0);
            }
            _ => return,
        }
        stats.updated_at = Some(Instant::now());
    }

    fn start(&self, id: u64) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
        if state.tasks[index].status != TaskStatus::Queued {
            return;
        }
        let queued_long_enough = state.tasks[index].queued_at.elapsed() >= Duration::from_millis(5);
        let queued_total = state.workspaces.values().map(|stats| stats.queued).sum();
        let workspace = state.tasks[index].workspace.clone();
        let slot_counted = state.tasks[index].slot_counted;
        if let Some(stats) = state.workspaces.get_mut(&workspace) {
            stats.queued = stats.queued.saturating_sub(1);
            if slot_counted {
                stats.active = stats.active.saturating_add(1);
            }
        }
        if queued_long_enough {
            state.observed_queued = state.observed_queued.max(queued_total);
        }
        if slot_counted {
            state.active_total = state.active_total.saturating_add(1);
            state.peak_active = state.peak_active.max(state.active_total);
            state.observed_active = state.observed_active.max(state.active_total);
        }
        let task = &mut state.tasks[index];
        task.status = TaskStatus::Running;
        task.started_at = Some(Instant::now());
    }

    fn finish(&self, id: u64, success: bool, response_bytes: u64, context_bytes_avoided: u64) {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
        if matches!(
            state.tasks[index].status,
            TaskStatus::Completed | TaskStatus::Failed
        ) {
            return;
        }

        let workspace = state.tasks[index].workspace.clone();
        let was_running = state.tasks[index].status == TaskStatus::Running;
        let was_queued = state.tasks[index].status == TaskStatus::Queued;
        let slot_counted = state.tasks[index].slot_counted;
        if let Some(stats) = state.workspaces.get_mut(&workspace) {
            if was_running && slot_counted {
                stats.active = stats.active.saturating_sub(1);
            }
            if was_queued {
                stats.queued = stats.queued.saturating_sub(1);
            }
            stats.response_bytes = stats.response_bytes.saturating_add(response_bytes);
            stats.context_bytes_avoided = stats
                .context_bytes_avoided
                .saturating_add(context_bytes_avoided);
            if success {
                stats.completed = stats.completed.saturating_add(1);
            } else {
                stats.failed = stats.failed.saturating_add(1);
            }
        }
        if was_running && slot_counted {
            state.active_total = state.active_total.saturating_sub(1);
        }
        state.traffic.push_back(TrafficEvent {
            at: now,
            requests: 0,
            request_bytes: 0,
            response_bytes,
            context_bytes_avoided,
        });
        let task = &mut state.tasks[index];
        task.status = if success {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
        task.finished_at = Some(now);
        task.response_bytes = response_bytes;
        task.context_bytes_avoided = context_bytes_avoided;
        trim_history(&mut state, now);
    }

    pub(super) fn snapshot(&self) -> MonitorSnapshot {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        trim_history(&mut state, now);
        let queued_now = state.workspaces.values().map(|stats| stats.queued).sum();
        let observed_active = state.observed_active.max(state.active_total);
        let observed_queued = state.observed_queued.max(queued_now);
        let snapshot = MonitorSnapshot {
            started_at: state.started_at,
            workspaces: state.workspaces.clone(),
            intelligence: state.intelligence.clone(),
            tasks: state.tasks.iter().cloned().collect(),
            traffic: state.traffic.iter().cloned().collect(),
            oauth_client_registered: state.oauth_client_registered,
            oauth_authorized: state.oauth_authorized,
            chatgpt_connected: state.chatgpt_connected,
            initialize_count: state.initialize_count,
            last_initialize: state.last_initialize,
            last_mcp_seen: state.last_mcp_seen,
            public_endpoint: state.public_endpoint.clone(),
            tunnels: state.tunnels.clone(),
            public_url_healthy: state.public_url_healthy,
            public_url_last_checked: state.public_url_last_checked,
            public_url_consecutive_failures: state.public_url_consecutive_failures,
            public_url_error: state.public_url_error.clone(),
            tunnel_running: state.tunnel_running,
            tunnel_error: state.tunnel_error.clone(),
            peak_active: state.peak_active,
            observed_active,
            observed_queued,
        };
        state.observed_active = state.active_total;
        state.observed_queued = queued_now;
        snapshot
    }
}

pub(super) fn trim_history(state: &mut MonitorState, now: Instant) {
    while state
        .traffic
        .front()
        .is_some_and(|event| now.saturating_duration_since(event.at) > TRAFFIC_WINDOW)
    {
        state.traffic.pop_front();
    }
    while state.traffic.len() > MAX_TRAFFIC_EVENTS {
        state.traffic.pop_front();
    }
    while state.tasks.len() > MAX_RECENT_TASKS {
        let removable = state
            .tasks
            .iter()
            .position(|task| matches!(task.status, TaskStatus::Completed | TaskStatus::Failed));
        if let Some(index) = removable {
            state.tasks.remove(index);
        } else {
            break;
        }
    }
}

impl TaskTicket {
    pub fn start(&mut self) {
        self.monitor.start(self.id);
    }

    pub fn finish(mut self, success: bool, response_bytes: u64) {
        self.monitor.finish(self.id, success, response_bytes, 0);
        self.finished = true;
    }

    pub fn finish_with_context_savings(
        mut self,
        success: bool,
        response_bytes: u64,
        context_bytes_avoided: u64,
    ) {
        self.monitor
            .finish(self.id, success, response_bytes, context_bytes_avoided);
        self.finished = true;
    }
}

impl Drop for TaskTicket {
    fn drop(&mut self) {
        if !self.finished {
            self.monitor.finish(self.id, false, 0, 0);
            self.finished = true;
        }
    }
}

impl MonitorRenderer {
    pub fn interrupt_receiver(&self) -> watch::Receiver<bool> {
        self.interrupted.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.join.await;
    }
}
