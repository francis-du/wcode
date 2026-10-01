use super::*;

pub(super) const DYNAMIC_SUBSPACE_SCAN_DEPTH: usize = 3;
pub(super) const MAX_DYNAMIC_SUBSPACE_SCAN_ENTRIES: usize = 20_000;

impl Workspaces {
    pub(super) fn refresh_requested_subspace(&self, requested: &str) -> Result<()> {
        let candidate_ids = if requested == self.default_id
            || requested.starts_with(&format!("{}/", self.default_id))
        {
            vec![requested.to_owned()]
        } else {
            vec![
                requested.to_owned(),
                format!("{}/{}", self.default_id, requested),
            ]
        };

        let selected = {
            let roots = self.roots.read().expect("workspace registry lock poisoned");
            if candidate_ids.iter().any(|candidate_id| {
                roots
                    .iter()
                    .any(|entry| entry.id == *candidate_id && entry.parent_id.is_none())
            }) {
                return Ok(());
            }
            candidate_ids.into_iter().find_map(|candidate_id| {
                let parent = roots
                    .iter()
                    .filter(|entry| entry.parent_id.is_none())
                    .filter(|entry| candidate_id.starts_with(&format!("{}/", entry.id)))
                    .max_by_key(|entry| entry.id.len())?;
                let relative = candidate_id[parent.id.len() + 1..].to_owned();
                if relative.is_empty()
                    || Path::new(&relative)
                        .components()
                        .any(|component| !matches!(component, Component::Normal(_)))
                {
                    return None;
                }
                let explicitly_authorized = roots.iter().any(|entry| {
                    entry.id == candidate_id
                        && entry.parent_id.is_some()
                        && entry.markers.contains(&"authorized")
                });
                Some((
                    candidate_id,
                    parent.id.clone(),
                    parent.workspace.clone(),
                    relative,
                    explicitly_authorized,
                ))
            })
        };

        let Some((candidate_id, parent_id, parent, relative, explicitly_authorized)) = selected
        else {
            return Ok(());
        };
        if explicitly_authorized {
            return Ok(());
        }

        let candidate = parent.root().join(&relative);
        reject_symlink_child(parent.root(), &candidate)?;
        let markers = current_subspace_markers(&candidate);
        if !candidate.is_dir() || markers.is_empty() {
            self.remove_discovered_workspace(&candidate_id);
            return Ok(());
        }

        let security = self.effective_security();
        let allow_write = self.allow_write || self.full_access_enabled();
        let allow_exec = self.allow_exec || self.full_access_enabled();
        let workspace = Workspace::new_with_authorization(
            &candidate,
            allow_write,
            allow_exec,
            security,
            self.authorization.clone(),
        )?;
        if workspace.root() == parent.root() || !workspace.root().starts_with(parent.root()) {
            return Ok(());
        }
        workspace.set_authorization_workspace_id(&candidate_id);

        let mut roots = self
            .roots
            .write()
            .expect("workspace registry lock poisoned");
        if let Some(existing) = roots.iter_mut().find(|entry| entry.id == candidate_id) {
            if existing.markers.contains(&"authorized") {
                return Ok(());
            }
            if existing.workspace.root() != workspace.root() {
                self.authorization.revoke_workspace(&candidate_id);
                existing.workspace = workspace;
            }
            existing.markers = markers;
            existing.parent_id = Some(parent_id);
            return Ok(());
        }
        if roots.len() < MAX_WORKSPACES {
            roots.push(WorkspaceRoot {
                id: candidate_id,
                workspace,
                parent_id: Some(parent_id),
                markers,
            });
        }
        Ok(())
    }

    pub(super) fn refresh_dynamic_subspaces(&self) {
        let parents = self
            .roots
            .read()
            .expect("workspace registry lock poisoned")
            .iter()
            .filter(|entry| entry.parent_id.is_none() && !entry.markers.contains(&"user-home"))
            .map(|entry| (entry.id.clone(), entry.workspace.clone()))
            .collect::<Vec<_>>();

        for (parent_id, parent) in parents {
            for discovered in discover_dynamic_subspaces(&parent) {
                let relative = match discovered.root.strip_prefix(parent.root()) {
                    Ok(relative) if !relative.as_os_str().is_empty() => relative,
                    _ => continue,
                };
                let id = format!("{parent_id}/{}", portable_relative_path(relative));
                let security = self.effective_security();
                let allow_write = self.allow_write || self.full_access_enabled();
                let allow_exec = self.allow_exec || self.full_access_enabled();
                let Ok(workspace) = Workspace::new_with_authorization(
                    &discovered.root,
                    allow_write,
                    allow_exec,
                    security,
                    self.authorization.clone(),
                ) else {
                    continue;
                };
                workspace.set_authorization_workspace_id(&id);

                let mut roots = self
                    .roots
                    .write()
                    .expect("workspace registry lock poisoned");
                if let Some(existing) = roots.iter_mut().find(|entry| entry.id == id) {
                    if existing.parent_id.is_some() && !existing.markers.contains(&"authorized") {
                        if existing.workspace.root() != workspace.root() {
                            self.authorization.revoke_workspace(&id);
                            existing.workspace = workspace;
                        }
                        existing.markers = discovered.markers;
                    }
                    continue;
                }
                if roots.len() >= MAX_WORKSPACES {
                    break;
                }
                roots.push(WorkspaceRoot {
                    id,
                    workspace,
                    parent_id: Some(parent_id.clone()),
                    markers: discovered.markers,
                });
            }
        }

        let stale = self
            .roots
            .read()
            .expect("workspace registry lock poisoned")
            .iter()
            .filter(|entry| entry.parent_id.is_some() && !entry.markers.contains(&"authorized"))
            .filter(|entry| {
                !entry.workspace.root().is_dir()
                    || current_subspace_markers(entry.workspace.root()).is_empty()
            })
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        for id in stale {
            self.remove_discovered_workspace(&id);
        }
    }

    fn remove_discovered_workspace(&self, id: &str) {
        let removed = {
            let mut roots = self
                .roots
                .write()
                .expect("workspace registry lock poisoned");
            roots
                .iter()
                .position(|entry| {
                    entry.id == id
                        && entry.parent_id.is_some()
                        && !entry.markers.contains(&"authorized")
                })
                .map(|index| roots.remove(index))
        };
        if removed.is_some() {
            self.authorization.revoke_workspace(id);
        }
    }
}

fn current_subspace_markers(root: &Path) -> Vec<&'static str> {
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
    markers
}

fn discover_dynamic_subspaces(parent: &Workspace) -> Vec<DiscoveredSubspace> {
    let mut claimed_roots = Vec::<PathBuf>::new();
    let mut discovered = Vec::new();
    let mut visited = 0usize;
    let mut builder = repository_walk_builder(parent.root(), true);
    builder.max_depth(Some(DYNAMIC_SUBSPACE_SCAN_DEPTH));
    for entry in builder.build().filter_map(|entry| entry.ok()) {
        if entry.depth() == 0 {
            continue;
        }
        visited = visited.saturating_add(1);
        if visited > MAX_DYNAMIC_SUBSPACE_SCAN_ENTRIES {
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        let root = entry.path();
        let markers = current_subspace_markers(root);
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
