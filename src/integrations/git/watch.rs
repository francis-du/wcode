//! Supervised, fixed-candidate reconciliation. No inbox replay, checkout,
//! verification execution, installation or cross-system atomic merge guarantee.
use super::*;
use std::path::{Path, PathBuf};
use tokio::time::{sleep_until, Instant};

const POLL_DELAY: Duration = Duration::from_secs(30);

/// One controller owns one exact target and the Check it created in this process.
/// Never deserializable: a caller cannot import an arbitrary Check ID or green receipt.
/// Local cooperative publishers share nonblocking OS admission for the Check/head.
/// A trusted supervisor still coordinates separate hosts and remote ambiguous effects.
pub struct GitHubCandidateWatch<'a> {
    provider: &'a GitHubProvider,
    target: ProviderTarget,
    root: PathBuf,
    workspace_id: String,
    owned: Option<CheckObservation>,
    admission: Option<Arc<PublicationGuard>>,
}

impl GitHubProvider {
    /// Construct without network or state writes. CLI also enforces disjoint
    /// trusted configuration and candidate roots before opening API credentials.
    pub fn watch_candidate(
        &self,
        change: u64,
        candidate: (&str, &str),
        root: impl AsRef<Path>,
        workspace_id: &str,
    ) -> Result<GitHubCandidateWatch<'_>> {
        let target = ProviderTarget {
            repository: self.config.repository.clone(),
            change,
            base_sha: candidate.0.to_ascii_lowercase(),
            head_sha: candidate.1.to_ascii_lowercase(),
        };
        target.validate()?;
        if workspace_id.trim().is_empty()
            || workspace_id.len() > 256
            || workspace_id.chars().any(char::is_control)
        {
            bail!("candidate watch requires a bounded workspace identity");
        }
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|_| anyhow!("candidate watch workspace unavailable"))?;
        Ok(GitHubCandidateWatch {
            provider: self,
            target,
            root,
            workspace_id: workspace_id.to_owned(),
            owned: None,
            admission: None,
        })
    }
}

impl GitHubCandidateWatch<'_> {
    /// Recheck live deployment, native Policy/Evidence and the remote Check.
    /// Unchanged facts perform reads only; changed facts reuse the red-first lane.
    /// An error attempts to deny only this controller's original Check/commit.
    pub async fn refresh(&mut self) -> Result<GitHubGateReceipt> {
        let result = self.refresh_inner().await;
        match result {
            Ok(receipt) => Ok(receipt),
            Err(error) if error.is::<PublicationBusy>() => Err(error),
            Err(_) => match self.revoke().await {
                Ok(true) => Err(anyhow!("candidate watch unavailable; owned Check denial confirmed")),
                Ok(false) => Err(anyhow!("candidate watch unavailable; no owned Check to revoke")),
                Err(_) => Err(anyhow!("candidate watch unavailable; Check denial unconfirmed; prior success may remain visible")),
            },
        }
    }

    async fn refresh_inner(&mut self) -> Result<GitHubGateReceipt> {
        if self.admission.is_none() {
            self.admission = Some(Arc::new(PublicationGuard::acquire(
                self.provider,
                &self.target,
            )?));
        }
        let report = self.provider.preflight(self.target.change).await?;
        if report.target != self.target || !report.configuration_verified {
            bail!("candidate watch preflight changed or incomplete");
        }
        let root = self.root.clone();
        let workspace_id = self.workspace_id.clone();
        self.provider
            .publish_bound_with_check(
                self.target.change,
                Some(self.target.clone()),
                move |target| {
                    let root = root.clone();
                    let workspace_id = workspace_id.clone();
                    async move {
                        crate::verification::acceptance_native::capture_local(
                            root,
                            &workspace_id,
                            &target.base_sha,
                            &target.head_sha,
                        )
                        .await
                    }
                },
                &mut self.owned,
                &mut self.admission,
            )
            .await
    }

    /// Never select a new PR head here. Check identity is re-read before mutation
    /// and failure is read back before reporting confirmation. No receipt is proof
    /// of a remote update when the network or credentials are unavailable.
    pub async fn revoke(&mut self) -> Result<bool> {
        let Some(known) = self.owned.as_ref() else {
            return Ok(false);
        };
        let guard = self
            .admission
            .as_deref()
            .ok_or_else(|| anyhow!("watcher has no publication guard"))?;
        guard.check(self.provider, &self.target)?;
        let current = self
            .provider
            .read_owned_check(&self.target, known.id)
            .await?;
        let denied = NativePublication::unavailable(self.target.clone())?;
        if self.provider.validate_check(&current, &denied).is_err() {
            self.provider.update_check(known.id, &denied, guard).await?;
        }
        let observed = self
            .provider
            .read_owned_check(&self.target, known.id)
            .await?;
        self.provider.validate_check(&observed, &denied)?;
        Ok(true)
    }

    /// Run in the foreground until shutdown. Each interval starts after completed
    /// work, not the previous tick. Errors back off without returning cached green.
    /// Graceful termination drains the current attempt and denies our Check;
    /// force-kill/outage remains an explicit deployment limitation.
    pub async fn run<S, E>(&mut self, shutdown: S, emit: E) -> Result<()>
    where
        S: Future<Output = Result<()>>,
        E: FnMut(Value) -> Result<()>,
    {
        self.run_with_delay(shutdown, emit, POLL_DELAY).await
    }

    pub(super) async fn run_with_delay<S, E>(
        &mut self,
        shutdown: S,
        mut emit: E,
        delay: Duration,
    ) -> Result<()>
    where
        S: Future<Output = Result<()>>,
        E: FnMut(Value) -> Result<()>,
    {
        let result = async {
            tokio::pin!(shutdown);
            emit(
                json!({"event":"candidate_watch_started", "target":self.target,
                "poll_interval_ms":delay.as_millis(), "current_acceptance":false,
                "shutdown":"drain_current_attempt_then_deny_owned_check"}),
            )?;
            let mut next = Instant::now();
            let mut failures = 0u32;
            loop {
                tokio::select! {
                    biased;
                    signal = &mut shutdown => return signal,
                    _ = sleep_until(next) => {}
                }
                match self.refresh().await {
                    Ok(receipt) => {
                        failures = 0;
                        emit(json!({"event":"candidate_observed", "receipt":receipt,
                            "continuous_merge_lock":false}))?;
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        // refresh() emits only fixed, sanitized local messages.
                        emit(
                            json!({"event":"candidate_unavailable", "current_acceptance":false,
                            "error":error.to_string(), "consecutive_failures":failures}),
                        )?;
                    }
                }
                let multiplier = 1u32 << failures.min(3);
                next = Instant::now() + delay.saturating_mul(multiplier);
            }
        }
        .await;
        // Also deny after an output/signal error. Never drop a green Check merely
        // because reporting failed, or report clean shutdown before acknowledgement.
        let denial = self.revoke().await;
        // Do not retain local admission after the controller exits. No subsequent
        // restart can reuse this object's old remote identity without recapture.
        self.owned = None;
        self.admission = None;
        let denied = denial.map_err(|_| {
            anyhow!("candidate watch shutdown denial unconfirmed; prior success may remain visible")
        })?;
        result?;
        emit(
            json!({"event":"candidate_watch_stopped", "owned_check_denied":denied,
            "current_acceptance":false}),
        )?;
        Ok(())
    }
}
