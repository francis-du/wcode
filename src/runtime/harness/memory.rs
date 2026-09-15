use super::*;

impl ToolHarness {
    pub(crate) fn trim_memory(&self, aggressive: bool) {
        let limits = crate::resource::limits();
        if aggressive {
            self.invalidate_project_flights(None);
            self.invalidate_repo_map_flights(None);
            self.invalidate_convention_flights(None);
        }
        if let Ok(mut cache) = self.repo_map_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.repo_map_cache_limit() / 2).max(1),
                |entry| entry.last_used,
            );
        }
        if let Ok(mut cache) = self.project_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.project_cache_limit() / 2).max(1),
                |entry| entry.last_used,
            );
        }
        if let Ok(mut cache) = self.observatory_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.project_cache_limit() / 2).max(1),
                |entry| entry.last_used,
            );
        }
        if let Ok(mut cache) = self.convention_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.project_cache_limit() / 2).max(1),
                |entry| entry.last_used,
            );
        }
        if let Ok(mut cache) = self.verification_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits
                    .project_cache_limit()
                    .saturating_mul(MAX_VERIFICATION_CHECKS)
                    / 2)
                .max(1),
                |entry| entry.last_used,
            );
        }
        self.intelligence.trim_design_cache(aggressive);
        self.code_index.trim_memory(aggressive);
        self.semantic_sessions.trim_memory(aggressive);
    }
}

fn trim_cache<K, V, F>(cache: &mut HashMap<K, V>, aggressive: bool, target: usize, last_used: F)
where
    K: Clone + Eq + std::hash::Hash,
    F: Fn(&V) -> Instant,
{
    if aggressive {
        cache.clear();
        return;
    }
    let mut candidates = cache
        .iter()
        .map(|(key, value)| (last_used(value), key.clone()))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|entry| entry.0);
    for (_, key) in candidates
        .into_iter()
        .take(cache.len().saturating_sub(target))
    {
        cache.remove(&key);
    }
}
