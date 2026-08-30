use super::*;

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
    let (paths, truncated) = workspace.source_files(".", max_files.clamp(1, MAX_PROVIDER_FILES))?;
    let mut providers = BTreeMap::<String, PathBuf>::new();
    let mut inputs = Vec::new();
    for path in paths {
        let Some(language) = language_for_path(&path) else {
            continue;
        };
        let Some((provider, executable)) = select_provider(workspace, language) else {
            continue;
        };
        if !automatic_provider(provider) {
            continue;
        }
        let (len, modified_nanos) = workspace.source_metadata_stamp(&path)?;
        providers
            .entry(provider.id.to_owned())
            .or_insert(executable);
        inputs.push(format!("{path}:{len}:{modified_nanos}"));
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
