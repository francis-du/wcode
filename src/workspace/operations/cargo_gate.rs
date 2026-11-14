use super::*;

pub(super) async fn acquire_cargo_contention_gate(
    program: &str,
    args: &[String],
    workspace_root: &Path,
    wait_timeout: Duration,
) -> Result<Option<CargoContentionPermit>> {
    let lane = match cargo_contention_lane(program, args) {
        Some(lane) => lane,
        None => return Ok(None),
    };
    let deadline = tokio::time::Instant::now() + wait_timeout;
    match lane {
        CargoContentionLane::Registry => {
            let permit = tokio::time::timeout(
                wait_timeout,
                Arc::clone(cargo_registry_gate()).acquire_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?
            .map_err(|_| anyhow!("cargo contention gate is shutting down"))?;
            Ok(Some(CargoContentionPermit::Registry { _permit: permit }))
        }
        CargoContentionLane::WorkspaceTest => {
            let gate = cargo_workspace_gate(workspace_root);
            let slot = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.test_slots).acquire_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?
            .map_err(|_| anyhow!("cargo test contention gate is shutting down"))?;
            let access = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.access).read_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?;
            Ok(Some(CargoContentionPermit::WorkspaceTest {
                _gate: gate,
                _slot: slot,
                _access: access,
            }))
        }
        CargoContentionLane::WorkspaceExclusive | CargoContentionLane::RegistryAndWorkspace => {
            let gate = cargo_workspace_gate(workspace_root);
            let access = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.access).write_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?;
            // Fetch may create Cargo.lock; update may replace it. Reserve local
            // writer access first, so waiting for local readers cannot monopolize
            // the global registry. Both waits share the same absolute deadline.
            let registry = if lane == CargoContentionLane::RegistryAndWorkspace {
                Some(
                    tokio::time::timeout_at(
                        deadline,
                        Arc::clone(cargo_registry_gate()).acquire_owned(),
                    )
                    .await
                    .map_err(|_| cargo_gate_timeout_error())?
                    .map_err(|_| anyhow!("cargo contention gate is shutting down"))?,
                )
            } else {
                None
            };
            Ok(Some(CargoContentionPermit::WorkspaceExclusive {
                _gate: gate,
                _access: access,
                _registry: registry,
            }))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct VerificationCommandFlightKey {
    pub(super) workspace_root: PathBuf,
    pub(super) cwd: PathBuf,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) revision: String,
    pub(super) timeout_seconds: u64,
    pub(super) lane: &'static str,
}

pub(super) struct VerificationCommandFlight {
    result: Mutex<Option<std::result::Result<CommandResult, String>>>,
    notify: tokio::sync::Notify,
}

pub(super) enum VerificationCommandFlightClaim {
    Leader(VerificationCommandFlightLeader),
    Follower(Arc<VerificationCommandFlight>),
}

pub(super) struct VerificationCommandFlightLeader {
    pub(super) flight: Arc<VerificationCommandFlight>,
    pub(super) completed: bool,
}

impl VerificationCommandFlight {
    pub(super) fn new() -> Self {
        Self {
            result: Mutex::new(None),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn publish(&self, result: std::result::Result<CommandResult, String>) {
        let mut slot = self
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(result);
            drop(slot);
            self.notify.notify_waiters();
        }
    }

    pub(super) async fn wait(&self) -> Result<CommandResult> {
        loop {
            let notified = self.notify.notified();
            if let Some(result) = self
                .result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                return result.map_err(anyhow::Error::msg);
            }
            notified.await;
        }
    }
}

impl VerificationCommandFlightLeader {
    pub(super) fn complete(&mut self, result: &Result<CommandResult>) {
        self.flight.publish(match result {
            Ok(result) => Ok(result.clone()),
            Err(error) => Err(error.to_string()),
        });
        self.completed = true;
    }
}

impl Drop for VerificationCommandFlightLeader {
    fn drop(&mut self) {
        if !self.completed {
            self.flight.publish(Err(
                "in-flight verification command leader ended before publishing a result".to_owned(),
            ));
        }
    }
}
