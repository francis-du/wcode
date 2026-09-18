use super::*;
use ignore::DirEntry;

pub(super) struct ProfileDiscoveryPaths {
    pub(super) candidate_dirs: Vec<PathBuf>,
    pub(super) contract_configs: Vec<PathBuf>,
}

pub(super) fn scan(workspace_root: &Path) -> ProfileDiscoveryPaths {
    let mut candidate_dirs = BTreeSet::new();
    let mut contract_configs = Vec::new();
    let mut profile_seen = 0usize;
    let mut contract_seen = 0usize;
    let mut contract_complete = false;

    let mut builder = crate::workspace::repository_ignore_builder(workspace_root, true);
    builder
        .max_depth(Some(MAX_PROFILE_SCAN_DEPTH.saturating_add(1)))
        .filter_entry(shared_visible_entry);
    for entry in builder.build().filter_map(Result::ok) {
        let profile_visible = entry.depth() >= 1
            && profile_seen < MAX_PROFILE_SCAN_ENTRIES
            && entry_visible_for(workspace_root, &entry, islands::profile_excluded_directory);
        if profile_visible {
            profile_seen = profile_seen.saturating_add(1);
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                let name = entry.file_name().to_string_lossy();
                if islands::is_manifest_file_name(&name) {
                    if let Some(parent) = entry.path().parent() {
                        if parent != workspace_root {
                            candidate_dirs.insert(parent.to_path_buf());
                        }
                    }
                }
            }
        }

        let contract_visible = !contract_complete
            && contract_seen < MAX_PROFILE_SCAN_ENTRIES
            && entry_visible_for(
                workspace_root,
                &entry,
                contracts::contract_excluded_directory,
            );
        if contract_visible {
            contract_seen = contract_seen.saturating_add(1);
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && contracts::is_contract_config_name(&entry.file_name().to_string_lossy())
            {
                contract_configs.push(entry.path().to_path_buf());
                contract_complete = contract_configs.len() > contracts::MAX_CONTRACT_CONFIGS;
            }
        }
        contract_complete |= contract_seen >= MAX_PROFILE_SCAN_ENTRIES;

        if profile_seen >= MAX_PROFILE_SCAN_ENTRIES && contract_complete {
            break;
        }
    }

    let mut candidate_dirs = candidate_dirs.into_iter().collect::<Vec<_>>();
    candidate_dirs.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    contract_configs.sort();
    ProfileDiscoveryPaths {
        candidate_dirs,
        contract_configs,
    }
}

fn shared_visible_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_some_and(|kind| kind.is_symlink()) {
        return false;
    }
    if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !islands::profile_excluded_directory(&name) || !contracts::contract_excluded_directory(&name)
}

fn entry_visible_for(
    workspace_root: &Path,
    entry: &DirEntry,
    excluded_directory: fn(&str) -> bool,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_some_and(|kind| kind.is_symlink()) {
        return false;
    }
    let Ok(relative) = entry.path().strip_prefix(workspace_root) else {
        return false;
    };
    let checked = if entry.file_type().is_some_and(|kind| kind.is_dir()) {
        relative
    } else {
        relative.parent().unwrap_or_else(|| Path::new(""))
    };
    checked.components().all(|component| {
        component
            .as_os_str()
            .to_str()
            .is_none_or(|name| !excluded_directory(name))
    })
}
