use super::*;
use std::sync::atomic::Ordering;

impl ToolHarness {
    pub fn convention_status(&self, workspace: &Workspace) -> Result<ConventionReport> {
        Ok(self.convention_status_cached(workspace)?.as_ref().clone())
    }

    pub(super) fn convention_status_cached(
        &self,
        workspace: &Workspace,
    ) -> Result<Arc<ConventionReport>> {
        let root = workspace.root().to_path_buf();
        let flight = self.convention_flight(&root)?;
        let observed_generation = flight.generation.load(Ordering::Acquire);
        let participant =
            harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
        #[cfg(test)]
        flight.entrants.fetch_add(1, Ordering::AcqRel);
        let _flight = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // An overlapping caller may reuse the report only when the owner
        // completed a successful validation against the current invalidation
        // revision. Non-overlapping calls still fingerprint the workspace.
        if flight.can_reuse_after(observed_generation) {
            let mut cache = self
                .convention_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("convention cache poisoned"))?;
            if let Some(cached) = cache.get_mut(&root) {
                cached.last_used = Instant::now();
                return Ok(cached.report.clone());
            }
        }

        let mut validation = harness_cache_flight::ValidationGuard::begin(&flight);
        let (fingerprint, files, scan_truncated) = conventions::fingerprint_and_paths(workspace)?;
        {
            let mut cache = self
                .convention_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("convention cache poisoned"))?;
            if let Some(cached) = cache
                .get_mut(&root)
                .filter(|cached| cached.fingerprint == fingerprint)
            {
                let report = cached.report.clone();
                cached.last_used = Instant::now();
                drop(cache);
                ensure_shared_convention_fingerprint_current(&participant, workspace, fingerprint)?;
                validation.mark_success();
                return Ok(report);
            }
        }

        let report = Arc::new(conventions::status_from_paths(
            workspace,
            files,
            scan_truncated,
        )?);
        if !validation.is_current() {
            bail!("convention cache invalidated while building; retry the request");
        }
        ensure_shared_convention_fingerprint_current(&participant, workspace, fingerprint)?;
        let mut cache = self
            .convention_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("convention cache poisoned"))?;
        if !validation.is_current() {
            bail!("convention cache invalidated while building; retry the request");
        }
        let limit = crate::resource::limits().project_cache_limit();
        if cache.len() >= limit && !cache.contains_key(&root) {
            if let Some(oldest) = cache
                .iter()
                .min_by(|(_, left), (_, right)| left.last_used.cmp(&right.last_used))
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedConventionReport {
                fingerprint,
                last_used: Instant::now(),
                report: report.clone(),
            },
        );
        validation.mark_success();
        Ok(report)
    }
}

pub(super) fn ensure_shared_convention_fingerprint_current(
    participant: &harness_cache_flight::ValidationParticipant<'_>,
    workspace: &Workspace,
    expected_fingerprint: u64,
) -> Result<()> {
    if participant.has_coalescible_peer() {
        let (confirmed_fingerprint, _, _) = conventions::fingerprint_and_paths(workspace)?;
        if confirmed_fingerprint != expected_fingerprint {
            bail!("convention state changed during shared validation; retry the request");
        }
    }
    Ok(())
}
