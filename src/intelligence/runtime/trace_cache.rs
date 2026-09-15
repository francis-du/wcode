use super::*;
use crate::design::{CodeRef, VerificationRef};
use rayon::prelude::*;
use std::hash::{DefaultHasher, Hash, Hasher};

#[cfg(test)]
thread_local! {
    pub(crate) static TRACE_RESOLUTION_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn traceability_fingerprint(
    workspace: &Workspace,
    state: &design::DesignState,
    design_fingerprint: u64,
    known_checks: &HashSet<String>,
) -> Result<u64> {
    let mut hasher = DefaultHasher::new();
    workspace.root().hash(&mut hasher);
    design_fingerprint.hash(&mut hasher);

    let mut checks = known_checks.iter().collect::<Vec<_>>();
    checks.sort_unstable();
    checks.len().hash(&mut hasher);
    for check in checks {
        check.hash(&mut hasher);
    }

    let mut paths = BTreeSet::new();
    for component in state.components.values() {
        for reference in &component.implementation {
            match reference {
                CodeRef::File { path } | CodeRef::Symbol { path, .. } => {
                    paths.insert(path.as_str());
                }
            }
        }
    }
    for criterion in state.acceptance.values() {
        for reference in &criterion.verification {
            if let VerificationRef::Test { path, .. } = reference {
                paths.insert(path.as_str());
            }
        }
    }
    let paths = paths.into_iter().collect::<Vec<_>>();
    let stamps = paths
        .par_iter()
        .map(|path| {
            workspace
                .source_metadata_stamp(path)
                .map_err(|error| error.to_string())
        })
        .collect::<Vec<_>>();
    paths.len().hash(&mut hasher);
    for (path, stamp) in paths.into_iter().zip(stamps) {
        path.hash(&mut hasher);
        match stamp {
            Ok(stamp) => {
                0u8.hash(&mut hasher);
                stamp.hash(&mut hasher);
            }
            Err(error) => {
                1u8.hash(&mut hasher);
                error.hash(&mut hasher);
            }
        }
    }
    Ok(hasher.finish())
}

impl SoftwareIntelligenceRuntime {
    pub(super) fn cached_traceability_status(
        &self,
        workspace: &Workspace,
        fingerprint: u64,
    ) -> Result<Option<TraceabilityStatus>> {
        let root = workspace.root().to_path_buf();
        let mut cache = self
            .traceability_cache
            .lock()
            .map_err(|_| anyhow!("traceability cache poisoned"))?;
        let Some(cached) = cache
            .get_mut(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
        else {
            return Ok(None);
        };
        cached.last_used = Instant::now();
        Ok(Some(cached.status.as_ref().clone()))
    }

    pub(super) fn cache_traceability_status(
        &self,
        workspace: &Workspace,
        fingerprint: u64,
        status: &TraceabilityStatus,
    ) -> Result<()> {
        let root = workspace.root().to_path_buf();
        let mut cache = self
            .traceability_cache
            .lock()
            .map_err(|_| anyhow!("traceability cache poisoned"))?;
        if cache.len() >= MAX_DESIGN_CACHE_WORKSPACES && !cache.contains_key(&root) {
            if let Some(oldest) = cache
                .iter()
                .min_by(|(_, left), (_, right)| left.last_used.cmp(&right.last_used))
                .map(|(root, _)| root.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedTraceabilityStatus {
                fingerprint,
                last_used: Instant::now(),
                status: Arc::new(status.clone()),
            },
        );
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum SymbolLookup {
    Found(Option<SymbolResolution>),
    Failed(String),
}

pub(super) struct TraceResolutionSnapshot {
    files: HashMap<String, std::result::Result<(), String>>,
    symbols: HashMap<(String, String), SymbolLookup>,
}

impl TraceResolutionSnapshot {
    pub(super) fn build(
        code_index: &CodeIndex,
        workspace: &Workspace,
        state: &design::DesignState,
    ) -> Self {
        #[cfg(test)]
        TRACE_RESOLUTION_BUILDS.with(|count| count.set(count.get().saturating_add(1)));
        let mut files = BTreeSet::new();
        let mut symbols_by_path = BTreeMap::<String, BTreeSet<String>>::new();
        for component in state.components.values() {
            for reference in &component.implementation {
                match reference {
                    CodeRef::File { path } => {
                        files.insert(path.clone());
                    }
                    CodeRef::Symbol { path, symbol } => {
                        symbols_by_path
                            .entry(path.clone())
                            .or_default()
                            .insert(symbol.clone());
                    }
                }
            }
        }
        for criterion in state.acceptance.values() {
            for reference in &criterion.verification {
                if let VerificationRef::Test { path, symbol } = reference {
                    symbols_by_path
                        .entry(path.clone())
                        .or_default()
                        .insert(symbol.clone());
                }
            }
        }

        let files = files
            .into_iter()
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|path| {
                let result = workspace
                    .source_stamp(&path)
                    .map(|_| ())
                    .map_err(|error| error.to_string());
                (path, result)
            })
            .collect();
        let resolved_paths = symbols_by_path
            .into_iter()
            .map(|(path, requested)| (path, requested.into_iter().collect::<Vec<_>>()))
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|(path, requested)| {
                let resolutions = code_index.resolve_symbols(workspace, &path, &requested);
                (path, requested, resolutions)
            })
            .collect::<Vec<_>>();
        let mut symbols = HashMap::new();
        for (path, requested, resolutions) in resolved_paths {
            match resolutions {
                Ok(resolutions) => {
                    for symbol in requested {
                        symbols.insert(
                            (path.clone(), symbol.clone()),
                            SymbolLookup::Found(resolutions.get(symbol.trim()).cloned().flatten()),
                        );
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    for symbol in requested {
                        symbols.insert(
                            (path.clone(), symbol),
                            SymbolLookup::Failed(message.clone()),
                        );
                    }
                }
            }
        }
        Self { files, symbols }
    }

    pub(super) fn code_references(
        &self,
        owner: &str,
        references: &[CodeRef],
    ) -> Vec<TraceReference> {
        references
            .iter()
            .map(|reference| match reference {
                CodeRef::File { path } => match self.files.get(path) {
                    Some(Ok(())) => TraceReference {
                        owner: owner.to_owned(),
                        kind: TraceReferenceKind::File,
                        target: path.clone(),
                        resolved: true,
                        provider: "filesystem".into(),
                        precision: "deterministic".into(),
                        node_id: Some(format!("file:{path}")),
                        revision: None,
                        message: None,
                    },
                    Some(Err(error)) => unresolved_reference(
                        owner,
                        TraceReferenceKind::File,
                        path,
                        "filesystem",
                        "deterministic",
                        error,
                    ),
                    None => unresolved_reference(
                        owner,
                        TraceReferenceKind::File,
                        path,
                        "filesystem",
                        "deterministic",
                        "file reference was not present in the traceability snapshot",
                    ),
                },
                CodeRef::Symbol { path, symbol } => {
                    self.symbol_reference(owner, TraceReferenceKind::Symbol, path, symbol)
                }
            })
            .collect()
    }

    pub(super) fn verification_references(
        &self,
        known_checks: &HashSet<String>,
        owner: &str,
        references: &[VerificationRef],
    ) -> Vec<TraceReference> {
        references
            .iter()
            .map(|reference| match reference {
                VerificationRef::Test { path, symbol } => {
                    self.symbol_reference(owner, TraceReferenceKind::Test, path, symbol)
                }
                VerificationRef::Check { id } if known_checks.contains(id) => TraceReference {
                    owner: owner.to_owned(),
                    kind: TraceReferenceKind::Check,
                    target: id.clone(),
                    resolved: true,
                    provider: "harness".into(),
                    precision: "deterministic".into(),
                    node_id: Some(format!("verification:{id}")),
                    revision: None,
                    message: None,
                },
                VerificationRef::Check { id } => unresolved_reference(
                    owner,
                    TraceReferenceKind::Check,
                    id,
                    "harness",
                    "deterministic",
                    "verification check is not present in the inferred project profile",
                ),
            })
            .collect()
    }

    fn symbol_reference(
        &self,
        owner: &str,
        kind: TraceReferenceKind,
        path: &str,
        symbol: &str,
    ) -> TraceReference {
        let target = format!("{path}::{symbol}");
        match self.symbols.get(&(path.to_owned(), symbol.to_owned())) {
            Some(SymbolLookup::Found(Some(resolution))) => {
                resolved_symbol_reference(owner, kind, target, resolution.clone())
            }
            Some(SymbolLookup::Found(None)) => unresolved_reference(
                owner,
                kind,
                &target,
                "tree-sitter",
                "syntax",
                "no unique symbol definition matched the declared reference",
            ),
            Some(SymbolLookup::Failed(error)) => {
                unresolved_reference(owner, kind, &target, "tree-sitter", "syntax", error)
            }
            None => unresolved_reference(
                owner,
                kind,
                &target,
                "tree-sitter",
                "syntax",
                "symbol reference was not present in the traceability snapshot",
            ),
        }
    }
}
