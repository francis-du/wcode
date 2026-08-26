use super::*;

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
            entries.push(WorkspaceRoot { id, workspace });
        }

        let Some(first) = entries.first() else {
            bail!("at least one workspace is required");
        };
        Ok(Self {
            default_id: first.id.clone(),
            roots: Arc::new(RwLock::new(entries)),
            allow_write,
            allow_exec,
            security,
            authorization,
        })
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    pub fn select(&self, id: Option<&str>) -> Result<(String, Workspace)> {
        let id = id.unwrap_or(&self.default_id);
        let roots = self.roots.read().expect("workspace registry lock poisoned");
        let root = roots
            .iter()
            .find(|root| root.id == id)
            .ok_or_else(|| anyhow!("unknown workspace: {id}"))?;
        Ok((root.id.clone(), root.workspace.clone()))
    }

    pub fn add_workspace(&self, root: impl AsRef<Path>) -> Result<(String, PathBuf)> {
        let workspace = Workspace::new_with_authorization(
            root,
            self.allow_write,
            self.allow_exec,
            self.security,
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
        validate_non_overlapping_root(&roots, &workspace, self.security)?;
        let mut used_ids = HashMap::<String, usize>::new();
        for existing in roots.iter() {
            *used_ids
                .entry(workspace_id(&existing.workspace.root))
                .or_insert(0) += 1;
        }
        let id = next_workspace_id(&workspace.root, &roots, &mut used_ids);
        workspace.set_authorization_workspace_id(&id);
        let canonical = workspace.root.clone();
        roots.push(WorkspaceRoot {
            id: id.clone(),
            workspace,
        });
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

    #[cfg(test)]
    pub fn latest_pending_authorization(&self) -> Option<AuthorizationRequest> {
        self.authorization.latest_pending()
    }

    pub fn approve_authorization_session(&self, id: &str) -> bool {
        let Some(request) = self.authorization.request_by_id(id) else {
            return false;
        };
        let command_access = if request.kind == AuthorizationKind::CommandAccess {
            let Some(program) = request.program.clone() else {
                return false;
            };
            let Ok((_, workspace)) = self.select(Some(&request.workspace)) else {
                return false;
            };
            if validate_authorizable_program(&program).is_err() {
                return false;
            }
            Some((workspace, program))
        } else {
            None
        };
        if !self.authorization.approve_session(id) {
            return false;
        }
        if let Some((workspace, program)) = command_access {
            return workspace.allow_command(&program).is_ok();
        }
        true
    }

    pub fn deny_authorization(&self, id: &str) -> bool {
        self.authorization.deny(id)
    }

    pub fn capabilities(&self) -> serde_json::Value {
        let roots = self.roots.read().expect("workspace registry lock poisoned");
        serde_json::json!({
            "default_workspace": self.default_id,
            "security": {
                "delete_tool_exposed": true,
                "delete_policy": "one-shot-human-approval; regular-file-or-empty-directory only; no recursive delete",
                "coding_primitives": ["create_directory", "move_path", "path_info", "write_file", "apply_edits", "create_files", "move_paths", "apply_file_edits", "delete_path"],
                "symlink_paths": "blocked",
                "protected_paths": "blocked",
                "overlapping_workspaces": self.security.allow_overlapping_workspaces,
                "broad_workspace_roots": self.security.allow_broad_workspace,
                "risky_exec_enabled": self.security.allow_risky_exec,
                "dynamic_authorization": true,
                "pending_authorizations": self.authorization.requests(MAX_WORKSPACES).iter().filter(|request| matches!(request.status, crate::authorization::AuthorizationStatus::Pending)).count(),
                "destructive_writes_enabled": self.security.allow_destructive_writes,
                "max_write_bytes": MAX_WRITE_BYTES,
            },
            "workspaces": roots.iter().map(|root| serde_json::json!({
                "id": root.id,
                "root": root.workspace.root,
                "write_enabled": root.workspace.allow_write,
                "exec_enabled": root.workspace.allow_exec,
                "risky_exec_enabled": root.workspace.security.allow_risky_exec,
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
}

fn validate_non_overlapping_root(
    entries: &[WorkspaceRoot],
    workspace: &Workspace,
    security: WorkspaceSecurity,
) -> Result<()> {
    if security.allow_overlapping_workspaces {
        return Ok(());
    }
    if let Some(existing) = entries.iter().find(|existing| {
        workspace.root.starts_with(&existing.workspace.root)
            || existing.workspace.root.starts_with(&workspace.root)
    }) {
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
            .filter(|entry| workspace_id(&entry.workspace.root) == base_id)
            .count()
    });
    *count += 1;
    if *count == 1 {
        base_id
    } else {
        format!("{base_id}-{}", *count)
    }
}
