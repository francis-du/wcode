use super::*;

const MAX_AUTO_DISCOVERY_FILES: usize = 10_000;

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
    let (paths, mut truncated) = workspace.source_files_background(".", scan_limit)?;
    let mut providers = BTreeMap::<String, PathBuf>::new();
    let mut inputs = Vec::new();
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
        for path in paths {
            let (len, modified_nanos) = workspace.source_metadata_stamp(&path)?;
            inputs.push(format!("{path}:{len}:{modified_nanos}"));
        }
    }
    inputs.sort();
    for (provider, executable) in &providers {
        let metadata = std::fs::metadata(executable).ok();
        let len = metadata.as_ref().map_or(0, std::fs::Metadata::len);
        let modified = metadata
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        inputs.push(format!(
            "provider:{provider}:{}:{len}:{modified}",
            executable.display()
        ));
    }
    let fingerprint = format!("sha256:{:x}", Sha256::digest(inputs.join("\n").as_bytes()));
    Ok(SemanticAutoState {
        fingerprint,
        providers: providers.len(),
        files: inputs.len().saturating_sub(providers.len()),
        truncated,
    })
}

pub(super) fn automatic_scan_limit(file_limit: usize) -> usize {
    let file_limit = file_limit.clamp(1, MAX_AUTO_DISCOVERY_FILES);
    file_limit
        .saturating_mul(32)
        .clamp(file_limit, MAX_AUTO_DISCOVERY_FILES)
}

fn automatic_source_groups(paths: Vec<String>) -> BTreeMap<SemanticLanguage, Vec<String>> {
    let mut groups = BTreeMap::<SemanticLanguage, Vec<String>>::new();
    for path in paths {
        let Some(language) = language_for_path(&path) else {
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
