use super::*;

impl ToolHarness {
    pub(crate) fn trim_memory(&self, aggressive: bool) {
        let limits = crate::resource::limits();
        if let Ok(mut cache) = self.repo_map_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.repo_map_cache_limit() / 2).max(1),
            );
        }
        if let Ok(mut cache) = self.project_cache.lock() {
            trim_cache(
                &mut cache,
                aggressive,
                (limits.project_cache_limit() / 2).max(1),
            );
        }
        self.code_index.trim_memory(aggressive);
        self.semantic_sessions.trim_memory(aggressive);
    }
}

fn trim_cache<K, V>(cache: &mut HashMap<K, V>, aggressive: bool, target: usize)
where
    K: Clone + Eq + std::hash::Hash,
{
    if aggressive {
        cache.clear();
        return;
    }
    let remove = cache
        .keys()
        .take(cache.len().saturating_sub(target))
        .cloned()
        .collect::<Vec<_>>();
    for key in remove {
        cache.remove(&key);
    }
}
