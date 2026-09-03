use super::*;
use crate::authorization::AuthorizationStatus;

const MAX_SUBSPACE_SCAN_DEPTH: usize = 8;
const SUBSPACE_FILE_MARKERS: &[(&str, &str)] = &[
    ("Cargo.toml", "cargo"),
    ("package.json", "npm"),
    ("pyproject.toml", "python"),
    ("go.mod", "go"),
    ("pom.xml", "maven"),
    ("build.gradle", "gradle"),
    ("build.gradle.kts", "gradle"),
    ("Package.swift", "swift"),
];

struct DiscoveredSubspace {
    root: PathBuf,
    markers: Vec<&'static str>,
}

impl Workspaces {
    #[cfg(test)]
    pub fn new<I, P>(roots: I, allow_write: bool, allow_exec: bool) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self::new_with_security(roots, allow_write, allow_exec, WorkspaceSecurity::default())
    }

    pub fn new_with_security<I, P>(
        roots: I,
        allow_write: bool,
        allow_exec: bool,
        security: WorkspaceSecurity,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let authorization = AuthorizationManager::default();
        let mut entries = Vec::<WorkspaceRoot>::new();
        let mut used_ids = HashMap::<String, usize>::new();
        let mut seen_roots = HashSet::<PathBuf>::new();

        for root in roots {
            if entries.len() >= MAX_WORKSPACES {
                bail!("at most {MAX_WORKSPACES} workspaces may be exposed by one process");
            }
            let workspace = Workspace::new_with_authorization(
                root,
                allow_write,
                allow_exec,
                security,
                authorization.clone(),
            )?;
            if !seen_roots.insert(workspace.root.clone()) {
                continue;
            }
            validate_non_overlapping_root(&entries, &workspace, security)?;
            let id = next_workspace_id(&workspace.root, &entries, &mut used_ids);
            workspace.set_authorization_workspace_id(&id);
            entries.push(WorkspaceRoot {
                id,
                workspace,
                parent_id: None,
                markers: Vec::new(),
            });
        }

        if entries.is_empty() {
            bail!("at least one workspace is required");
        }
        let default_id = entries[0].id.clone();
        let configured = entries
            .iter()
            .map(|entry| (entry.id.clone(), entry.workspace.clone()))
            .collect::<Vec<_>>();
        for (parent_id, parent) in configured {
            append_discovered_subspaces(
                &mut entries,
                &parent_id,
                &parent,
                allow_write,
                allow_exec,
                security,
                &authorization,
            );
        }
        Ok(Self {
            default_id,
            roots: Arc::new(RwLock::new(entries)),
            allow_write,
            allow_exec,
            security,
            full_access: Arc::new(AtomicBool::new(false)),
            authorization,
        })
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    pub fn configured_roots(&self) -> Vec<PathBuf> {
        self.roots
            .read()
            .expect("workspace registry lock poisoned")
            .iter()
            .filter(|root| root.parent_id.is_none())
            .map(|root| root.workspace.root().to_path_buf())
            .collect()
    }

    pub fn select(&self, id: Option<&str>) -> Result<(String, Workspace)> {
        let requested = id.unwrap_or(&self.default_id).trim_matches('/');
        let roots = self.roots.read().expect("workspace registry lock poisoned");
        if let Some(root) = roots.iter().find(|root| root.id == requested) {
            return Ok((root.id.clone(), root.workspace.clone()));
        }

        let qualified = if requested == self.default_id
            || requested.starts_with(&format!("{}/", self.default_id))
        {
            None
        } else {
            Some(format!("{}/{}", self.default_id, requested))
        };
        if let Some(root) = qualified
            .as_deref()
            .and_then(|qualified| roots.iter().find(|root| root.id == qualified))
        {
            return Ok((root.id.clone(), root.workspace.clone()));
        }

        Err(anyhow!("unknown workspace: {requested}"))
    }

    pub fn add_workspace(&self, root: impl AsRef<Path>) -> Result<(String, PathBuf)> {
        self.add_workspace_path(root.as_ref(), false)
    }

    fn full_access_security(&self) -> WorkspaceSecurity {
        WorkspaceSecurity {
            allow_risky_exec: true,
            allow_semantic_exec: true,
            allow_destructive_writes: true,
            allow_overlapping_workspaces: true,
            allow_user_home_workspace: true,
            allow_broad_workspace: self.security.allow_broad_workspace,
        }
    }

    fn effective_security(&self) -> WorkspaceSecurity {
        if self.full_access.load(Ordering::Relaxed) {
            self.full_access_security()
        } else {
            self.security
        }
    }

    pub fn full_access_enabled(&self) -> bool {
        self.full_access.load(Ordering::Relaxed)
    }

    pub fn grant_full_user_access(&self) -> Result<(String, PathBuf)> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .context("HOME/USERPROFILE is not set")?
            .canonicalize()
            .context("cannot resolve the current user home directory")?;
        let security = self.full_access_security();
        let mut roots = self
            .roots
            .write()
            .expect("workspace registry lock poisoned");

        let mut elevated = Vec::with_capacity(roots.len());
        for entry in roots.iter() {
            let workspace = Workspace::new_with_authorization(
                entry.workspace.root(),
                true,
                true,
                security,
                self.authorization.clone(),
            )?;
            workspace.set_authorization_workspace_id(&entry.id);
            elevated.push(workspace);
        }
        for (entry, workspace) in roots.iter_mut().zip(elevated) {
            entry.workspace = workspace;
            if !entry.markers.contains(&"full-access") {
                entry.markers.push("full-access");
            }
        }

        if let Some(existing) = roots.iter().find(|entry| entry.workspace.root() == home) {
            self.full_access.store(true, Ordering::Relaxed);
            return Ok((existing.id.clone(), home));
        }
        if roots.len() >= MAX_WORKSPACES {
            bail!("at most {MAX_WORKSPACES} workspaces may be exposed by one process");
        }
        let workspace = Workspace::new_with_authorization(
            &home,
            true,
            true,
            security,
            self.authorization.clone(),
        )?;
        let mut used_ids = HashMap::<String, usize>::new();
        for existing in roots.iter().filter(|entry| entry.parent_id.is_none()) {
            *used_ids
                .entry(workspace_id(existing.workspace.root()))
                .or_insert(0) += 1;
        }
        let id = next_workspace_id(&workspace.root, &roots, &mut used_ids);
        workspace.set_authorization_workspace_id(&id);
        roots.push(WorkspaceRoot {
            id: id.clone(),
            workspace,
            parent_id: None,
            markers: vec!["full-access", "user-home"],
        });
        self.full_access.store(true, Ordering::Relaxed);
        Ok((id, home))
    }

    pub fn add_workspace_from(
        &self,
        selected: Option<&str>,
        root: &str,
    ) -> Result<(String, PathBuf)> {
        let root = root.trim();
        if root.is_empty() {
            bail!("workspace root is required");
        }
        let (_, selected_workspace) = self.select(selected)?;
        let supplied = PathBuf::from(root);
        let candidate = if supplied.is_absolute() {
            supplied
        } else {
            selected_workspace.root().join(supplied)
        };
        let derived = is_lexical_child(selected_workspace.root(), &candidate);
        if derived {
            reject_symlink_child(selected_workspace.root(), &candidate)?;
        }
        self.add_workspace_path(&candidate, derived)
    }

    fn add_workspace_path(&self, root: &Path, allow_derived: bool) -> Result<(String, PathBuf)> {
        let security = self.effective_security();
        let allow_write = self.allow_write || self.full_access_enabled();
        let allow_exec = self.allow_exec || self.full_access_enabled();
        let workspace = Workspace::new_with_authorization(
            root,
            allow_write,
            allow_exec,
            security,
            self.authorization.clone(),
        )?;
        let mut roots = self
            .roots
            .write()
            .expect("workspace registry lock poisoned");
        if let Some(existing) = roots
            .iter()
            .find(|existing| existing.workspace.root == workspace.root)
        {
            return Ok((existing.id.clone(), existing.workspace.root.clone()));
        }
        if roots.len() >= MAX_WORKSPACES {
            bail!("at most {MAX_WORKSPACES} workspaces may be exposed by one process");
        }

        let derived_parent = if allow_derived {
            roots
                .iter()
                .filter(|entry| entry.parent_id.is_none())
                .filter(|entry| workspace.root.starts_with(entry.workspace.root()))
                .max_by_key(|entry| entry.workspace.root().components().count())
        } else {
            None
        };
        if let Some(parent) = derived_parent {
            let parent_id = parent.id.clone();
            let parent_root = parent.workspace.root().to_path_buf();
            let relative = workspace.root().strip_prefix(&parent_root)?;
            let relative = portable_relative_path(relative);
            if relative.is_empty() {
                return Ok((parent_id, parent_root));
            }
            let id = format!("{parent_id}/{relative}");
            workspace.set_authorization_workspace_id(&id);
            let canonical = workspace.root.clone();
            roots.push(WorkspaceRoot {
                id: id.clone(),
                workspace,
                parent_id: Some(parent_id),
                markers: vec!["authorized"],
            });
            return Ok((id, canonical));
        }

        validate_non_overlapping_root(&roots, &workspace, security)?;
        let mut used_ids = HashMap::<String, usize>::new();
        for existing in roots.iter().filter(|entry| entry.parent_id.is_none()) {
            *used_ids
                .entry(workspace_id(&existing.workspace.root))
                .or_insert(0) += 1;
        }
        let id = next_workspace_id(&workspace.root, &roots, &mut used_ids);
        workspace.set_authorization_workspace_id(&id);
        let canonical = workspace.root.clone();
        let parent = workspace.clone();
        roots.push(WorkspaceRoot {
            id: id.clone(),
            workspace,
            parent_id: None,
            markers: Vec::new(),
        });
        append_discovered_subspaces(
            &mut roots,
            &id,
            &parent,
            allow_write,
            allow_exec,
            security,
            &self.authorization,
        );
        Ok((id, canonical))
    }

    pub fn workspace_access(&self, id: Option<&str>) -> Result<serde_json::Value> {
        let (id, workspace) = self.select(id)?;
        Ok(serde_json::json!({
            "id": id,
            "root": workspace.root(),
            "write_enabled": workspace.write_enabled(),
            "exec_enabled": workspace.exec_enabled(),
            "allowed_commands": workspace.allowed_commands(),
            "available_commands": workspace.available_commands(),
        }))
    }

    pub fn allow_command(&self, id: Option<&str>, program: &str) -> Result<serde_json::Value> {
        let (id, workspace) = self.select(id)?;
        let changed = workspace.allow_command(program)?;
        let mut value = self.workspace_access(Some(&id))?;
        value["changed"] = serde_json::json!(changed);
        Ok(value)
    }

    pub fn revoke_command(&self, id: Option<&str>, program: &str) -> Result<serde_json::Value> {
        let (id, workspace) = self.select(id)?;
        let changed = workspace.revoke_command(program)?;
        let mut value = self.workspace_access(Some(&id))?;
        value["changed"] = serde_json::json!(changed);
        Ok(value)
    }

    pub fn authorization_requests(&self, limit: usize) -> Vec<AuthorizationRequest> {
        self.authorization.requests(limit)
    }

    pub fn authorization_request(&self, id: &str) -> Option<AuthorizationRequest> {
        self.authorization.request_by_id(id)
    }

    pub(crate) fn authorization_interactive_token(&self, id: &str) -> Option<String> {
        self.authorization.interactive_token(id)
    }

    #[cfg(test)]
    pub fn latest_pending_authorization(&self) -> Option<AuthorizationRequest> {
        self.authorization.latest_pending()
    }

    pub fn approve_authorization_session(&self, id: &str) -> bool {
        self.approve_authorization_session_result(id).is_ok()
    }

    pub fn approve_authorization_session_result(&self, id: &str) -> Result<AuthorizationRequest> {
        let request = self
            .authorization
            .request_by_id(id)
            .ok_or_else(|| anyhow!("authorization request does not exist: {id}"))?;
        if request.status != AuthorizationStatus::Pending {
            bail!(
                "authorization request {id} is no longer pending (status: {:?})",
                request.status
            );
        }
        let command_access = if request.kind == AuthorizationKind::CommandAccess {
            let program = request
                .program
                .clone()
                .ok_or_else(|| anyhow!("command authorization request {id} is missing program"))?;
            let (_, workspace) = self.select(Some(&request.workspace))?;
            validate_authorizable_program(&program)?;
            Some((workspace, program))
        } else {
            None
        };
        if !self.authorization.approve_session(id) {
            bail!("authorization request {id} changed while it was being approved");
        }
        if let Some((workspace, program)) = command_access {
            workspace.allow_command(&program)?;
        }
        self.authorization
            .request_by_id(id)
            .ok_or_else(|| anyhow!("authorization request disappeared after approval: {id}"))
    }

    pub fn authorize_command_operation(
        &self,
        id: Option<&str>,
        program: &str,
        args: &[String],
        cwd: &str,
    ) -> Result<AuthorizationRequest> {
        let (workspace_id, workspace) = self.select(id)?;
        let program = program.trim();
        validate_authorizable_program(program)?;
        if !workspace.exec_enabled() {
            bail!("command execution is disabled for workspace {workspace_id}");
        }
        let cwd_path = workspace.existing_path(cwd)?;
        if !cwd_path.is_dir() {
            bail!("cwd is not a directory");
        }
        let mut elevated = workspace.security;
        elevated.allow_risky_exec = true;
        validate_command_policy(program, args, elevated)?;
        if !workspace.command_allowed(program) {
            bail!(
                "executable access is not authorized for {program}; approve its CommandAccess request before authorizing an exact repository operation"
            );
        }
        let operation = format!("run_command\0{program}\0{}\0{cwd}", args.join("\0"));
        let fingerprint = operation_fingerprint(workspace.root(), &operation);
        let request = self.authorization.request(
            workspace_id,
            AuthorizationKind::RiskyExecution,
            format!(
                "allow repository-aware command: {program} {}",
                args.join(" ")
            ),
            fingerprint,
        );
        self.approve_authorization_session_result(&request.id)
    }

    pub fn deny_authorization(&self, id: &str) -> bool {
        self.authorization.deny(id)
    }

    pub fn capabilities(&self) -> serde_json::Value {
        let roots = self.roots.read().expect("workspace registry lock poisoned");
        let security = self.effective_security();
        serde_json::json!({
            "default_workspace": self.default_id,
            "security": {
                "delete_tool_exposed": true,
                "delete_policy": "one-shot-human-approval; regular-file-or-empty-directory only; no recursive delete",
                "coding_primitives": ["create_directory", "move_path", "path_info", "write_file", "apply_edits", "create_files", "move_paths", "apply_file_edits", "delete_path"],
                "symlink_paths": "blocked",
                "protected_paths": "blocked",
                "full_access": self.full_access_enabled(),
                "full_access_scope": "current-user-home; filesystem root and hard protected-path/symlink/hard-link/no-shell boundaries remain",
                "overlapping_workspaces": security.allow_overlapping_workspaces,
                "user_home_workspace": security.allow_user_home_workspace,
                "broad_workspace_roots": security.allow_broad_workspace,
                "risky_exec_enabled": security.allow_risky_exec,
                "semantic_exec_enabled": (self.allow_exec || self.full_access_enabled()) && security.allow_semantic_exec,
                "dynamic_authorization": true,
                "pending_authorizations": self.authorization.requests(MAX_WORKSPACES).iter().filter(|request| matches!(request.status, crate::authorization::AuthorizationStatus::Pending)).count(),
                "destructive_writes_enabled": security.allow_destructive_writes,
                "max_write_bytes": MAX_WRITE_BYTES,
            },
            "subspace_discovery": {
                "enabled": true,
                "max_depth": MAX_SUBSPACE_SCAN_DEPTH,
                "routing": "select the most specific discovered workspace id for project-scoped work",
                "markers": [".git", ".wcode/project.yaml", "Cargo.toml", "package.json", "pyproject.toml", "go.mod", "pom.xml", "build.gradle", "build.gradle.kts", "Package.swift"],
            },
            "workspaces": roots.iter().map(|root| serde_json::json!({
                "id": root.id,
                "root": root.workspace.root,
                "kind": if root.parent_id.is_some() { "subspace" } else { "configured" },
                "parent_workspace": root.parent_id.as_deref(),
                "markers": &root.markers,
                "write_enabled": root.workspace.allow_write,
                "exec_enabled": root.workspace.allow_exec,
                "risky_exec_enabled": root.workspace.security.allow_risky_exec,
                "semantic_exec_enabled": root.workspace.semantic_exec_enabled(),
                "destructive_writes_enabled": root.workspace.security.allow_destructive_writes,
                "allowed_commands": root.workspace.allowed_commands(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        self.roots
            .read()
            .expect("workspace registry lock poisoned")
            .iter()
            .map(|root| (root.id.clone(), root.workspace.root.clone()))
            .collect()
    }

    pub(crate) fn semantic_workspaces(&self) -> Vec<(String, Workspace)> {
        let roots = self.roots.read().expect("workspace registry lock poisoned");
        roots
            .iter()
            .filter(|candidate| {
                candidate.workspace.semantic_exec_enabled()
                    && !roots.iter().any(|other| {
                        other.workspace.root != candidate.workspace.root
                            && other.workspace.root.starts_with(&candidate.workspace.root)
                    })
            })
            .map(|root| (root.id.clone(), root.workspace.clone()))
            .collect()
    }
}

fn is_lexical_child(parent: &Path, candidate: &Path) -> bool {
    candidate.strip_prefix(parent).is_ok_and(|relative| {
        relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    })
}

fn reject_symlink_child(parent: &Path, candidate: &Path) -> Result<()> {
    let relative = candidate.strip_prefix(parent)?;
    let mut current = parent.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("workspace child path contains an unsupported component");
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "symlink workspace paths are blocked to preserve workspace isolation: {}",
                current.display()
            ),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn append_discovered_subspaces(
    entries: &mut Vec<WorkspaceRoot>,
    parent_id: &str,
    parent: &Workspace,
    allow_write: bool,
    allow_exec: bool,
    security: WorkspaceSecurity,
    authorization: &AuthorizationManager,
) {
    for discovered in discover_subspaces(parent) {
        if entries.len() >= MAX_WORKSPACES {
            break;
        }
        if entries
            .iter()
            .any(|entry| entry.workspace.root == discovered.root)
        {
            continue;
        }
        let Ok(workspace) = Workspace::new_with_authorization(
            &discovered.root,
            allow_write,
            allow_exec,
            security,
            authorization.clone(),
        ) else {
            continue;
        };
        if workspace.root() == parent.root() || !workspace.root().starts_with(parent.root()) {
            continue;
        }
        let Ok(relative) = workspace.root().strip_prefix(parent.root()) else {
            continue;
        };
        let relative = portable_relative_path(relative);
        if relative.is_empty() {
            continue;
        }
        let id = format!("{parent_id}/{relative}");
        if entries.iter().any(|entry| entry.id == id) {
            continue;
        }
        workspace.set_authorization_workspace_id(&id);
        entries.push(WorkspaceRoot {
            id,
            workspace,
            parent_id: Some(parent_id.to_owned()),
            markers: discovered.markers,
        });
    }
}

fn discover_subspaces(parent: &Workspace) -> Vec<DiscoveredSubspace> {
    let mut claimed_roots = Vec::<PathBuf>::new();
    let mut discovered = Vec::new();
    let mut visited = 0usize;
    let mut cpu_slice = Some(crate::resource::cpu_work(
        crate::resource::WorkClass::Interactive,
    ));
    for entry in WalkDir::new(parent.root())
        .min_depth(1)
        .max_depth(MAX_SUBSPACE_SCAN_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(visible_entry)
        .filter_map(|entry| entry.ok())
    {
        visited = visited.saturating_add(1);
        if visited.is_multiple_of(64) {
            drop(cpu_slice.take());
            cpu_slice = Some(crate::resource::cpu_work(
                crate::resource::WorkClass::Interactive,
            ));
        }
        if !entry.file_type().is_dir() {
            continue;
        }
        let root = entry.path();
        let mut markers = Vec::new();
        if marker_exists(&root.join(".git"), true) {
            markers.push("git");
        }
        if wcode_project_marker_exists(root) {
            markers.push("wcode");
        }
        for &(file, marker) in SUBSPACE_FILE_MARKERS {
            if marker_exists(&root.join(file), false) && !markers.contains(&marker) {
                markers.push(marker);
            }
        }
        if markers.is_empty() {
            continue;
        }

        let authoritative = markers.contains(&"git") || markers.contains(&"wcode");
        if !authoritative
            && claimed_roots
                .iter()
                .any(|claimed| root.starts_with(claimed))
        {
            continue;
        }
        claimed_roots.push(root.to_path_buf());
        discovered.push(DiscoveredSubspace {
            root: root.to_path_buf(),
            markers,
        });
    }
    discovered.sort_by(|left, right| left.root.cmp(&right.root));
    discovered
}

fn wcode_project_marker_exists(root: &Path) -> bool {
    let directory = root.join(".wcode");
    let safe_directory = fs::symlink_metadata(&directory)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
    safe_directory && marker_exists(&directory.join("project.yaml"), false)
}

fn marker_exists(path: &Path, allow_directory: bool) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        !metadata.file_type().is_symlink()
            && (metadata.is_file() || (allow_directory && metadata.is_dir()))
    })
}

fn validate_non_overlapping_root(
    entries: &[WorkspaceRoot],
    workspace: &Workspace,
    security: WorkspaceSecurity,
) -> Result<()> {
    if security.allow_overlapping_workspaces {
        return Ok(());
    }
    if let Some(existing) = entries
        .iter()
        .filter(|existing| existing.parent_id.is_none())
        .find(|existing| {
            workspace.root.starts_with(&existing.workspace.root)
                || existing.workspace.root.starts_with(&workspace.root)
        })
    {
        bail!(
            "overlapping workspace roots are blocked: {} and {}; expose only the narrow roots you need or restart with --allow-overlapping-workspaces",
            existing.workspace.root.display(),
            workspace.root.display()
        );
    }
    Ok(())
}

fn next_workspace_id(
    root: &Path,
    entries: &[WorkspaceRoot],
    used_ids: &mut HashMap<String, usize>,
) -> String {
    let base_id = workspace_id(root);
    let count = used_ids.entry(base_id.clone()).or_insert_with(|| {
        entries
            .iter()
            .filter(|entry| {
                entry.parent_id.is_none() && workspace_id(&entry.workspace.root) == base_id
            })
            .count()
    });
    *count += 1;
    if *count == 1 {
        base_id
    } else {
        format!("{base_id}-{}", *count)
    }
}
