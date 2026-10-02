use super::*;
use std::sync::atomic::Ordering;

pub(super) struct ValidationParticipant<'a> {
    flight: &'a ValidationFlight,
    coalescible: bool,
}

impl<'a> ValidationParticipant<'a> {
    pub(super) fn join(flight: &'a ValidationFlight, observed_generation: u64) -> Self {
        let coalescible = observed_generation.is_multiple_of(2);
        if coalescible {
            flight.coalescible_callers.fetch_add(1, Ordering::AcqRel);
        }
        Self {
            flight,
            coalescible,
        }
    }

    pub(super) fn has_coalescible_peer(&self) -> bool {
        self.coalescible && self.flight.coalescible_callers.load(Ordering::Acquire) > 1
    }
}

impl Drop for ValidationParticipant<'_> {
    fn drop(&mut self) {
        if self.coalescible {
            self.flight
                .coalescible_callers
                .fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub(super) struct ValidationGuard<'a> {
    flight: &'a ValidationFlight,
    invalidation_revision: u64,
    succeeded: bool,
}

impl<'a> ValidationGuard<'a> {
    pub(super) fn begin(flight: &'a ValidationFlight) -> Self {
        debug_assert!(flight.generation.load(Ordering::Acquire).is_multiple_of(2));
        let invalidation_revision = flight.invalidation_revision.load(Ordering::Acquire);
        flight.generation.fetch_add(1, Ordering::AcqRel);
        Self {
            flight,
            invalidation_revision,
            succeeded: false,
        }
    }

    pub(super) fn mark_success(&mut self) {
        self.succeeded = true;
    }

    pub(super) fn is_current(&self) -> bool {
        self.flight.invalidation_revision.load(Ordering::Acquire) == self.invalidation_revision
    }
}

impl Drop for ValidationGuard<'_> {
    fn drop(&mut self) {
        let completed_generation = self.flight.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if self.succeeded && self.is_current() {
            self.flight
                .successful_revision
                .store(self.invalidation_revision, Ordering::Release);
            self.flight
                .successful_generation
                .store(completed_generation, Ordering::Release);
        }
    }
}

impl ValidationFlight {
    pub(super) fn acquire(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        // A native caller can also retain a tool permit while waiting for an
        // owner that is suspended in another pool. Bound only the waiter: the
        // owner's guard, generation and actual-work permit remain untouched.
        let deadline = Instant::now() + Duration::from_secs(5);
        let parallel_worker = rayon::current_thread_index().is_some();
        loop {
            match self.gate.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            // Never yield or sleep on a Rayon stack: the owner may be the
            // suspended caller below it, so even cooperative yielding can wait
            // for itself. An explicit error does not validate a cached result.
            if parallel_worker {
                bail!("shared validation busy on parallel worker; retry after current validation completes");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("shared validation busy after 5s; retry after current validation completes");
            }
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }

    pub(super) fn can_reuse_after(&self, observed_generation: u64) -> bool {
        if !observed_generation.is_multiple_of(2) {
            return false;
        }
        let current_generation = self.generation.load(Ordering::Acquire);
        let successful_generation = self.successful_generation.load(Ordering::Acquire);
        let invalidation_revision = self.invalidation_revision.load(Ordering::Acquire);
        let successful_revision = self.successful_revision.load(Ordering::Acquire);
        current_generation != observed_generation
            && successful_generation == current_generation
            && successful_revision == invalidation_revision
    }

    fn invalidate(&self) {
        self.invalidation_revision.fetch_add(1, Ordering::AcqRel);
    }
}

impl ToolHarness {
    pub(super) fn project_flight(&self, root: &Path) -> Result<Arc<ValidationFlight>> {
        let mut flights = self
            .project_flights
            .lock()
            .map_err(|_| anyhow::anyhow!("project profile flight registry poisoned"))?;
        if let Some(flight) = flights.get(root).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        flights.retain(|_, flight| flight.strong_count() > 0);
        if flights.len() >= self.max_parallel {
            bail!(
                "too many independent project profile validations; retry after current builds complete"
            );
        }
        let flight = Arc::new(ValidationFlight::default());
        flights.insert(root.to_path_buf(), Arc::downgrade(&flight));
        Ok(flight)
    }

    pub(super) fn repo_map_flight(
        &self,
        cache_key: &RepoMapCacheKey,
    ) -> Result<Arc<ValidationFlight>> {
        let mut flights = self
            .repo_map_flights
            .lock()
            .map_err(|_| anyhow::anyhow!("repo map flight registry poisoned"))?;
        if let Some(flight) = flights.get(cache_key).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        flights.retain(|_, flight| flight.strong_count() > 0);
        if flights.len() >= self.max_parallel {
            bail!("too many independent repo map builds; retry after current builds complete");
        }
        let flight = Arc::new(ValidationFlight::default());
        flights.insert(cache_key.clone(), Arc::downgrade(&flight));
        Ok(flight)
    }

    pub(super) fn convention_flight(&self, root: &Path) -> Result<Arc<ValidationFlight>> {
        let mut flights = self
            .convention_flights
            .lock()
            .map_err(|_| anyhow::anyhow!("convention flight registry poisoned"))?;
        if let Some(flight) = flights.get(root).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        flights.retain(|_, flight| flight.strong_count() > 0);
        if flights.len() >= self.max_parallel {
            bail!(
                "too many independent convention validations; retry after current builds complete"
            );
        }
        let flight = Arc::new(ValidationFlight::default());
        flights.insert(root.to_path_buf(), Arc::downgrade(&flight));
        Ok(flight)
    }

    pub(super) fn invalidate_project_flights(&self, root: Option<&Path>) {
        if let Ok(mut flights) = self.project_flights.lock() {
            flights.retain(|_, flight| flight.strong_count() > 0);
            for (flight_root, flight) in flights.iter() {
                if root.is_none_or(|root| flight_root == root) {
                    if let Some(flight) = flight.upgrade() {
                        flight.invalidate();
                    }
                }
            }
        }
    }

    pub(super) fn invalidate_repo_map_flights(&self, root: Option<&Path>) {
        if let Ok(mut flights) = self.repo_map_flights.lock() {
            flights.retain(|_, flight| flight.strong_count() > 0);
            for ((flight_root, _), flight) in flights.iter() {
                if root.is_none_or(|root| flight_root == root) {
                    if let Some(flight) = flight.upgrade() {
                        flight.invalidate();
                    }
                }
            }
        }
    }

    pub(super) fn invalidate_convention_flights(&self, root: Option<&Path>) {
        if let Ok(mut flights) = self.convention_flights.lock() {
            flights.retain(|_, flight| flight.strong_count() > 0);
            for (flight_root, flight) in flights.iter() {
                if root.is_none_or(|root| flight_root == root) {
                    if let Some(flight) = flight.upgrade() {
                        flight.invalidate();
                    }
                }
            }
        }
    }
}
