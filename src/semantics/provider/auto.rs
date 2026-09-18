use super::*;

const MAX_AUTO_DISCOVERY_FILES: usize = 10_000;
const AUTO_CONFIGURATION_PATHS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rust-toolchain",
    ".cargo/config.toml",
    ".cargo/config",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticAutoState {
    pub fingerprint: String,
    pub providers: usize,
    pub files: usize,
    pub truncated: bool,
}

pub(crate) fn state(workspace: &Workspace, max_files: usize) -> Result<SemanticAutoState> {
    if !workspace.semantic_exec_enabled() {
        return Ok(SemanticAutoState {
            fingerprint: "disabled".to_owned(),
            providers: 0,
            files: 0,
            truncated: false,
        });
    }
    let file_limit = max_files.clamp(1, MAX_PROVIDER_FILES);
    let scan_limit = automatic_scan_limit(file_limit);
    let (paths, mut truncated) = workspace.source_files_background_with_stamps(".", scan_limit)?;
    let mut providers = BTreeMap::<String, PathBuf>::new();
    let mut inputs = Vec::<StampedSourcePath>::new();
    let mut remaining = file_limit;
    for (language, mut paths) in automatic_source_groups(paths) {
        let Some((provider, executable)) = select_automatic_provider(workspace, language) else {
            continue;
        };
        if remaining == 0 {
            truncated = true;
            break;
        }
        if paths.len() > remaining {
            paths.truncate(remaining);
            truncated = true;
        }
        remaining = remaining.saturating_sub(paths.len());
        providers
            .entry(provider.id.to_owned())
            .or_insert(executable);
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Background);
        inputs.extend(paths);
    }
    inputs.extend(automatic_configuration_stamps(workspace));
    inputs.sort_by(|left, right| left.0.cmp(&right.0));
    inputs.dedup_by(|left, right| left.0 == right.0);
    let file_count = inputs.len();
    let mut hasher = Sha256::new();
    let mut first = true;
    for (path, stamp) in &inputs {
        if !first {
            hasher.update(b"\n");
        }
        first = false;
        hasher.update(path.as_bytes());
        hasher.update(b":");
        stamp.update_sha256(&mut hasher);
    }
    for (provider, executable) in &providers {
        let metadata = std::fs::metadata(executable).ok();
        let len = metadata.as_ref().map_or(0, std::fs::Metadata::len);
        let modified = metadata
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        if !first {
            hasher.update(b"\n");
        }
        first = false;
        hasher.update(b"provider:");
        hasher.update(provider.as_bytes());
        hasher.update(b":");
        hasher.update(executable.to_string_lossy().as_bytes());
        hasher.update(b":");
        hasher.update(len.to_string().as_bytes());
        hasher.update(b":");
        hasher.update(modified.to_string().as_bytes());
    }
    let fingerprint = format!("sha256:{:x}", hasher.finalize());
    Ok(SemanticAutoState {
        fingerprint,
        providers: providers.len(),
        files: file_count,
        truncated,
    })
}

fn automatic_configuration_stamps(workspace: &Workspace) -> Vec<StampedSourcePath> {
    AUTO_CONFIGURATION_PATHS
        .iter()
        .filter_map(|path| {
            workspace
                .source_paths_with_stamps(path, 1)
                .ok()
                .and_then(|(mut paths, _)| paths.pop())
        })
        .collect()
}

pub(super) fn automatic_scan_limit(file_limit: usize) -> usize {
    let file_limit = file_limit.clamp(1, MAX_AUTO_DISCOVERY_FILES);
    file_limit
        .saturating_mul(32)
        .clamp(file_limit, MAX_AUTO_DISCOVERY_FILES)
}

fn automatic_source_groups(
    paths: Vec<StampedSourcePath>,
) -> BTreeMap<SemanticLanguage, Vec<StampedSourcePath>> {
    let mut groups = BTreeMap::<SemanticLanguage, Vec<StampedSourcePath>>::new();
    for path in paths {
        let Some(language) = language_for_path(&path.0) else {
            continue;
        };
        if !PROVIDERS
            .iter()
            .copied()
            .any(|provider| automatic_provider(provider) && provider.languages.contains(&language))
        {
            continue;
        }
        groups.entry(language).or_default().push(path);
    }
    groups
}

fn select_automatic_provider(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Option<(ProviderCandidate, PathBuf)> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| automatic_provider(*provider) && provider.languages.contains(&language))
        .find_map(|provider| {
            provider
                .executables
                .iter()
                .find_map(|executable| find_executable(workspace, executable))
                .map(|executable| (provider, executable))
        })
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/provider_auto.rs"]
mod tests;
