use super::*;

impl Workspace {
    #[cfg(test)]
    pub fn new(root: impl AsRef<Path>, allow_write: bool, allow_exec: bool) -> Result<Self> {
        Self::new_with_security(root, allow_write, allow_exec, WorkspaceSecurity::default())
    }

    pub fn new_with_security(
        root: impl AsRef<Path>,
        allow_write: bool,
        allow_exec: bool,
        security: WorkspaceSecurity,
    ) -> Result<Self> {
        Self::new_with_authorization(
            root,
            allow_write,
            allow_exec,
            security,
            AuthorizationManager::default(),
        )
    }

    pub(super) fn new_with_authorization(
        root: impl AsRef<Path>,
        allow_write: bool,
        allow_exec: bool,
        security: WorkspaceSecurity,
        authorization: AuthorizationManager,
    ) -> Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("workspace does not exist: {}", root.as_ref().display()))?;
        if !root.is_dir() {
            bail!("workspace is not a directory: {}", root.display());
        }
        validate_workspace_root(&root, security)?;
        let root_identity = root_identity(&root)?;
        let authorization_workspace = workspace_id(&root);
        let commands = Arc::new(RwLock::new(
            COMMAND_CATALOG.iter().copied().map(str::to_owned).collect(),
        ));
        Ok(Self {
            root,
            root_identity,
            allow_write,
            allow_exec,
            security,
            authorization,
            authorization_workspace: Arc::new(RwLock::new(authorization_workspace)),
            commands,
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn write_enabled(&self) -> bool {
        self.allow_write
    }

    pub(crate) fn exec_enabled(&self) -> bool {
        self.allow_exec
    }

    pub(crate) fn risky_exec_enabled(&self) -> bool {
        self.security.allow_risky_exec
    }

    pub(super) fn set_authorization_workspace_id(&self, id: &str) {
        *self
            .authorization_workspace
            .write()
            .expect("workspace authorization id lock poisoned") = id.to_owned();
    }

    pub(super) fn authorization_workspace_id(&self) -> String {
        self.authorization_workspace
            .read()
            .expect("workspace authorization id lock poisoned")
            .clone()
    }

    pub(super) fn command_access_fingerprint(&self, program: &str) -> String {
        operation_fingerprint(&self.root, &format!("command_access\0{program}"))
    }

    pub(crate) fn allowed_commands(&self) -> Vec<String> {
        let mut commands = self
            .commands
            .read()
            .expect("workspace command allowlist lock poisoned")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        commands.sort();
        commands
    }

    pub(crate) fn available_commands(&self) -> Vec<String> {
        let allowed = self
            .commands
            .read()
            .expect("workspace command allowlist lock poisoned");
        COMMAND_CATALOG
            .iter()
            .copied()
            .filter(|program| !allowed.contains(*program))
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn allow_command(&self, program: &str) -> Result<bool> {
        let program = program.trim();
        validate_authorizable_program(program)?;
        Ok(self
            .commands
            .write()
            .expect("workspace command allowlist lock poisoned")
            .insert(program.to_owned()))
    }

    pub(crate) fn revoke_command(&self, program: &str) -> Result<bool> {
        let program = program.trim();
        validate_authorizable_program(program)?;
        Ok(self
            .commands
            .write()
            .expect("workspace command allowlist lock poisoned")
            .remove(program))
    }

    pub(crate) fn authorize_risky_operation(
        &self,
        kind: AuthorizationKind,
        operation: &str,
        summary: &str,
    ) -> Result<()> {
        if self.security.allow_risky_exec {
            return Ok(());
        }
        let fingerprint = operation_fingerprint(&self.root, operation);
        if self.authorization.is_granted(&fingerprint) {
            return Ok(());
        }
        let request = self.authorization.request(
            self.authorization_workspace_id(),
            kind,
            summary,
            fingerprint,
        );
        bail!(
            "authorization required: {} · {}. Approve this request in the TUI, then retry the operation",
            request.id,
            request.summary
        )
    }

    pub(crate) fn source_stamp(&self, path: &str) -> Result<SourceStamp> {
        let file = self.existing_path(path)?;
        let metadata = fs::metadata(&file)?;
        validate_source_metadata(&metadata)?;
        Ok(source_stamp(&metadata))
    }

    pub(crate) fn load_source(&self, path: &str) -> Result<SourceDocument> {
        let file = self.existing_path(path)?;
        let metadata_before = fs::metadata(&file)?;
        validate_source_metadata(&metadata_before)?;
        let stamp_before = source_stamp(&metadata_before);
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let metadata_after = fs::metadata(&file)?;
        validate_source_metadata(&metadata_after)?;
        let stamp_after = source_stamp(&metadata_after);
        if stamp_before != stamp_after {
            bail!("source file changed while it was being read; retry the request");
        }
        Ok(SourceDocument {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256: sha256(content.as_bytes()),
            content,
            stamp: stamp_after,
        })
    }

    pub(crate) fn source_files(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<String>, bool)> {
        let start = self.existing_path(path)?;
        if start.is_file() {
            let relative = portable_relative_path(start.strip_prefix(&self.root)?);
            return Ok((vec![relative], false));
        }
        if !start.is_dir() {
            bail!("path is not a file or directory");
        }

        let limit = max_entries.clamp(1, 50_000);
        let mut files = Vec::new();
        let mut truncated = false;
        for entry in WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(visible_entry)
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry
                .metadata()
                .map(|metadata| metadata.len() > MAX_READ_BYTES)
                .unwrap_or(true)
            {
                continue;
            }
            if files.len() == limit {
                truncated = true;
                break;
            }
            files.push(portable_relative_path(
                entry.path().strip_prefix(&self.root)?,
            ));
        }
        files.sort();
        Ok((files, truncated))
    }

    pub(super) fn validate_relative(path: &str) -> Result<PathBuf> {
        if path.contains('\0') || path.contains(['\n', '\r']) {
            bail!("path contains forbidden control characters");
        }
        if path.trim().is_empty() || path == "." {
            return Ok(PathBuf::new());
        }
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            bail!("absolute paths are not allowed");
        }
        for component in candidate.components() {
            if matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                bail!("path traversal is not allowed");
            }
            if let Component::Normal(value) = component {
                let value = value.to_string_lossy();
                if value.contains(':') {
                    bail!("alternate data streams and colon-bearing path components are blocked");
                }
            }
        }
        reject_protected_path(&candidate)?;
        Ok(candidate)
    }

    pub(super) fn ensure_root_intact(&self) -> Result<()> {
        let current = self
            .root
            .canonicalize()
            .context("workspace root is no longer accessible")?;
        let identity = root_identity(&current)?;
        if current != self.root || identity != self.root_identity {
            bail!("workspace root identity changed after startup; restart wcode");
        }
        Ok(())
    }

    pub(super) fn ensure_no_symlink_components(
        &self,
        relative: &Path,
        allow_missing_leaf: bool,
    ) -> Result<()> {
        let mut current = self.root.clone();
        let components = relative.components().collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(value) = component else {
                continue;
            };
            current.push(value);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!(
                        "symlink paths are blocked to preserve workspace isolation: {}",
                        current.display()
                    )
                }
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        && allow_missing_leaf
                        && index + 1 == components.len() =>
                {
                    return Ok(())
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub(super) fn existing_path(&self, path: &str) -> Result<PathBuf> {
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        self.ensure_no_symlink_components(&relative, false)?;
        let resolved = self
            .root
            .join(relative)
            .canonicalize()
            .with_context(|| format!("path not found: {path}"))?;
        if !resolved.starts_with(&self.root) {
            bail!("path escapes workspace");
        }
        Ok(resolved)
    }

    pub(super) fn new_path(&self, path: &str) -> Result<PathBuf> {
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("file path is required");
        }
        self.ensure_no_symlink_components(&relative, true)?;
        let target = self.root.join(&relative);
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("invalid target path"))?;
        let resolved_parent = parent
            .canonicalize()
            .with_context(|| format!("parent directory not found: {}", parent.display()))?;
        if !resolved_parent.starts_with(&self.root) {
            bail!("path escapes workspace");
        }
        Ok(target)
    }

    pub(super) fn write_lock_for(&self, path: &Path) -> Result<Arc<Mutex<()>>> {
        let mut locks = self
            .write_locks
            .lock()
            .map_err(|_| anyhow!("workspace write lock registry poisoned"))?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
        Ok(lock)
    }
}
