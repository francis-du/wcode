use super::*;

struct SourceScan {
    paths: Vec<String>,
    stamps: Option<Vec<SourceMetadataStamp>>,
    truncated: bool,
}

const WRITE_LOCK_PRUNE_INTERVAL: usize = 128;
const MAX_WRITE_LOCKS: usize = 4_096;

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

    pub(crate) fn semantic_exec_enabled(&self) -> bool {
        self.allow_exec && self.security.allow_semantic_exec
    }

    pub(crate) fn is_linked_worktree_of(&self, shared: &Workspace) -> Result<bool> {
        if self.root == shared.root {
            return Ok(false);
        }
        let Some((isolated_common, isolated_linked)) = git_common_dir(&self.root)? else {
            return Ok(false);
        };
        if !isolated_linked {
            return Ok(false);
        }
        let Some((shared_common, _)) = git_common_dir(&shared.root)? else {
            return Ok(false);
        };
        Ok(isolated_common == shared_common)
    }

    pub(crate) fn mutation_domain_root(&self) -> Result<PathBuf> {
        for ancestor in self.root.ancestors() {
            let dotgit = ancestor.join(".git");
            let metadata = match fs::symlink_metadata(&dotgit) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if metadata.file_type().is_symlink() {
                bail!(
                    "Git metadata path must not be a symlink: {}",
                    dotgit.display()
                );
            }
            if metadata.is_dir() || metadata.is_file() {
                return ancestor.canonicalize().with_context(|| {
                    format!("cannot resolve Git worktree root {}", ancestor.display())
                });
            }
        }
        Ok(self.root.clone())
    }

    pub(crate) fn readonly_subspace(&self, relative: &str) -> Result<Self> {
        let root = self.existing_path(relative)?;
        if !root.is_dir() {
            bail!("quality subspace is not a directory: {relative}");
        }
        Ok(Self {
            root_identity: root_identity(&root)?,
            root,
            allow_write: false,
            allow_exec: self.allow_exec,
            security: self.security,
            authorization: self.authorization.clone(),
            authorization_workspace: self.authorization_workspace.clone(),
            commands: self.commands.clone(),
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        })
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

    pub(crate) fn workspace_commands_granted(&self) -> bool {
        self.authorization
            .workspace_commands_granted(&self.authorization_workspace_id())
    }

    pub(crate) fn command_allowed(&self, program: &str) -> bool {
        self.workspace_commands_granted()
            || self
                .commands
                .read()
                .expect("workspace command allowlist lock poisoned")
                .contains(program)
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

    pub(crate) fn risky_operation_authorized(&self, operation: &str) -> bool {
        if self.security.allow_risky_exec || self.workspace_commands_granted() {
            return true;
        }
        let fingerprint = operation_fingerprint(&self.root, operation);
        self.authorization.is_granted(&fingerprint)
    }

    pub(crate) fn authorize_risky_operation(
        &self,
        kind: AuthorizationKind,
        operation: &str,
        summary: &str,
    ) -> Result<()> {
        if self.security.allow_risky_exec || self.workspace_commands_granted() {
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
        Err(AuthorizationRequired::new(request).into())
    }

    pub(crate) fn source_stamp(&self, path: &str) -> Result<SourceStamp> {
        let file = self.existing_path(path)?;
        let metadata = fs::metadata(&file)?;
        validate_source_metadata(&metadata)?;
        Ok(source_stamp(&metadata))
    }

    pub(crate) fn source_metadata_stamp(&self, path: &str) -> Result<SourceMetadataStamp> {
        let file = self.existing_path(path)?;
        let metadata = fs::metadata(&file)?;
        if !metadata.is_file() {
            bail!("path is not a file");
        }
        let stamp = source_stamp(&metadata);
        Ok(stamp.metadata_stamp())
    }

    pub(crate) fn load_source(&self, path: &str) -> Result<SourceDocument> {
        let file = self.existing_path(path)?;
        let metadata_before = fs::metadata(&file)?;
        validate_source_metadata(&metadata_before)?;
        let stamp_before = source_stamp(&metadata_before);
        self.load_source_after_stamp(file, &stamp_before)
    }

    pub(crate) fn load_source_at_stamp(
        &self,
        path: &str,
        expected: &SourceStamp,
    ) -> Result<SourceDocument> {
        let file = self.existing_path(path)?;
        self.load_source_after_stamp(file, expected)
    }

    fn load_source_after_stamp(
        &self,
        file: PathBuf,
        expected: &SourceStamp,
    ) -> Result<SourceDocument> {
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let metadata_after = fs::metadata(&file)?;
        validate_source_metadata(&metadata_after)?;
        let stamp_after = source_stamp(&metadata_after);
        if expected != &stamp_after {
            bail!("source file changed while it was being read; retry the request");
        }
        Ok(SourceDocument {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256: sha256(content.as_bytes()),
            readonly: metadata_after.permissions().readonly(),
            content,
            stamp: stamp_after,
        })
    }

    pub(crate) fn source_files(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<String>, bool)> {
        self.source_files_with_class(
            path,
            max_entries,
            crate::resource::WorkClass::Interactive,
            true,
        )
    }

    pub(crate) fn source_files_with_stamps(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<StampedSourcePath>, bool)> {
        self.source_files_with_stamps_and_class(
            path,
            max_entries,
            crate::resource::WorkClass::Interactive,
        )
    }

    pub(crate) fn source_paths_with_stamps(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<StampedSourcePath>, bool)> {
        let scan = self.scan_source_files(
            path,
            max_entries,
            crate::resource::WorkClass::Interactive,
            false,
            true,
        )?;
        let stamps = scan
            .stamps
            .expect("source path scan requested metadata stamps");
        Ok((scan.paths.into_iter().zip(stamps).collect(), scan.truncated))
    }

    pub(crate) fn source_files_background(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<String>, bool)> {
        self.source_files_with_class(
            path,
            max_entries,
            crate::resource::WorkClass::Background,
            true,
        )
    }

    pub(crate) fn source_files_background_with_stamps(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<StampedSourcePath>, bool)> {
        self.source_files_with_stamps_and_class(
            path,
            max_entries,
            crate::resource::WorkClass::Background,
        )
    }

    fn source_files_with_class(
        &self,
        path: &str,
        max_entries: usize,
        work_class: crate::resource::WorkClass,
        readable_only: bool,
    ) -> Result<(Vec<String>, bool)> {
        let scan = self.scan_source_files(path, max_entries, work_class, readable_only, false)?;
        Ok((scan.paths, scan.truncated))
    }

    fn source_files_with_stamps_and_class(
        &self,
        path: &str,
        max_entries: usize,
        work_class: crate::resource::WorkClass,
    ) -> Result<(Vec<StampedSourcePath>, bool)> {
        let scan = self.scan_source_files(path, max_entries, work_class, true, true)?;
        let stamps = scan.stamps.expect("source scan requested metadata stamps");
        Ok((scan.paths.into_iter().zip(stamps).collect(), scan.truncated))
    }

    fn scan_source_files(
        &self,
        path: &str,
        max_entries: usize,
        work_class: crate::resource::WorkClass,
        readable_only: bool,
        capture_stamps: bool,
    ) -> Result<SourceScan> {
        let start = self.existing_path(path)?;
        if start.is_file() {
            let relative = portable_relative_path(start.strip_prefix(&self.root)?);
            let stamps = if capture_stamps {
                let metadata = fs::metadata(&start)?;
                let stamp = source_stamp(&metadata);
                Some(vec![stamp.metadata_stamp()])
            } else {
                None
            };
            return Ok(SourceScan {
                paths: vec![relative],
                stamps,
                truncated: false,
            });
        }
        if !start.is_dir() {
            bail!("path is not a file or directory");
        }

        let limit = max_entries.clamp(1, 50_000);
        let mut paths = Vec::new();
        let mut stamps = capture_stamps.then(Vec::new);
        let mut truncated = false;
        let mut visited = 0usize;
        let mut cpu_slice = Some(crate::resource::cpu_work(work_class));
        let honor_parent_ignores = start == self.root;
        for entry in repository_walk_builder(&start, honor_parent_ignores)
            .build()
            .filter_map(|entry| entry.ok())
        {
            visited = visited.saturating_add(1);
            if visited.is_multiple_of(64) {
                drop(cpu_slice.take());
                cpu_slice = Some(crate::resource::cpu_work(work_class));
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let metadata = if readable_only || capture_stamps {
                match entry.metadata() {
                    Ok(metadata) => Some(metadata),
                    Err(_) => {
                        if capture_stamps {
                            truncated = true;
                        }
                        continue;
                    }
                }
            } else {
                None
            };
            if readable_only
                && metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.len() > MAX_READ_BYTES)
            {
                continue;
            }
            if paths.len() == limit {
                truncated = true;
                break;
            }
            paths.push(portable_relative_path(
                entry.path().strip_prefix(&self.root)?,
            ));
            if let (Some(stamps), Some(metadata)) = (stamps.as_mut(), metadata.as_ref()) {
                let stamp = source_stamp(metadata);
                stamps.push(stamp.metadata_stamp());
            }
        }
        if let Some(stamps) = stamps.as_mut() {
            let mut combined = paths
                .into_iter()
                .zip(std::mem::take(stamps))
                .collect::<Vec<_>>();
            combined.sort_by(|left, right| left.0.cmp(&right.0));
            let (sorted_paths, sorted_stamps): (Vec<_>, Vec<_>) = combined.into_iter().unzip();
            return Ok(SourceScan {
                paths: sorted_paths,
                stamps: Some(sorted_stamps),
                truncated,
            });
        }
        paths.sort();
        Ok(SourceScan {
            paths,
            stamps: None,
            truncated,
        })
    }

    pub(crate) fn normalize_relative_scope(path: &str) -> Result<String> {
        let relative = Self::validate_relative(path)?;
        Ok(relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"))
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
        if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        if locks.len() >= MAX_WRITE_LOCKS
            || (locks.len() >= WRITE_LOCK_PRUNE_INTERVAL
                && locks.len().is_multiple_of(WRITE_LOCK_PRUNE_INTERVAL))
        {
            locks.retain(|_, lock| lock.strong_count() > 0);
        }
        if locks.len() >= MAX_WRITE_LOCKS {
            bail!("too many concurrently active workspace file locks; retry after current writes finish");
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
        Ok(lock)
    }
}

fn git_common_dir(root: &Path) -> Result<Option<(PathBuf, bool)>> {
    for ancestor in root.ancestors() {
        let dotgit = ancestor.join(".git");
        let metadata = match fs::symlink_metadata(&dotgit) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            bail!(
                "Git metadata path must not be a symlink: {}",
                dotgit.display()
            );
        }
        if metadata.is_dir() {
            return Ok(Some((dotgit.canonicalize()?, false)));
        }
        if !metadata.is_file() || metadata.len() > 4 * 1024 {
            return Ok(None);
        }
        let marker = fs::read_to_string(&dotgit)
            .with_context(|| format!("cannot read Git worktree marker {}", dotgit.display()))?;
        let Some(value) = marker.trim().strip_prefix("gitdir:") else {
            return Ok(None);
        };
        let gitdir = PathBuf::from(value.trim());
        let gitdir = if gitdir.is_absolute() {
            gitdir
        } else {
            ancestor.join(gitdir)
        }
        .canonicalize()
        .with_context(|| {
            format!(
                "cannot resolve Git worktree metadata for {}",
                root.display()
            )
        })?;
        let commondir_path = gitdir.join("commondir");
        let commondir = fs::read_to_string(&commondir_path).with_context(|| {
            format!(
                "cannot read Git common-dir marker {}",
                commondir_path.display()
            )
        })?;
        let common = PathBuf::from(commondir.trim());
        let common = if common.is_absolute() {
            common
        } else {
            gitdir.join(common)
        }
        .canonicalize()
        .with_context(|| format!("cannot resolve Git common directory for {}", root.display()))?;
        return Ok(Some((common, true)));
    }
    Ok(None)
}
