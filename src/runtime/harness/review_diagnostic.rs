use super::*;

// Accept only Git's two complete native non-repository diagnostics. The caller
// still checks exit/output integrity and the absence of ancestor .git metadata.
pub(super) fn known_non_repository_diagnostic(stderr: &str) -> bool {
    let mut lines = stderr.lines();
    let Some(first) = lines.next() else {
        return false;
    };
    if first == "fatal: not a git repository (or any of the parent directories): .git" {
        return lines.next().is_none();
    }
    let Some(mount) = first
        .strip_prefix("fatal: not a git repository (or any parent up to mount point ")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    !mount.is_empty()
        && mount.len() <= 4096
        && (mount.starts_with('/') || std::path::Path::new(mount).is_absolute())
        && !mount.chars().any(char::is_control)
        && lines.next()
            == Some("Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).")
        && lines.next().is_none()
}

pub(super) async fn git_repository_root(workspace: &Workspace) -> Result<Option<PathBuf>> {
    if !workspace.exec_enabled() {
        bail!("Git inspection requires Workspace execution access");
    }
    // Discovery needs the native exit status to distinguish a positively
    // identified non-repository from an unavailable/denied Git probe.
    let args = vec!["rev-parse".into(), "--show-toplevel".into()];
    let result = workspace.run_command("git", &args, ".", 10).await?;
    if !result.success {
        let known_non_git = result.exit_code == Some(128)
            && !result.timed_out
            && !result.truncated
            && !result.redacted
            && !result.output_incomplete
            && result.stdout.is_empty()
            && known_non_repository_diagnostic(&result.stderr);
        if known_non_git {
            for ancestor in workspace.root().ancestors() {
                match std::fs::symlink_metadata(ancestor.join(".git")) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => bail!("Git repository discovery is unavailable"),
                }
            }
            return Ok(None);
        }
        bail!("Git repository discovery is unavailable");
    }
    let output = complete_git_output(result)?;
    let path = output.strip_suffix('\n').unwrap_or(&output);
    let path = if cfg!(windows) {
        path.strip_suffix('\r').unwrap_or(path)
    } else {
        path
    };
    if path.is_empty() || path.chars().any(char::is_control) {
        bail!("Git repository root is invalid");
    }
    let root = Path::new(path)
        .canonicalize()
        .context("Git repository root is unavailable")?;
    if !workspace.root().starts_with(&root) {
        bail!("Git repository root does not contain the Workspace");
    }
    Ok(Some(root))
}

async fn git_resolve_commit(workspace: &Workspace, revision: &str) -> Result<String> {
    if revision != "HEAD" && !crate::verification::change::full_oid(revision) {
        bail!("Git change requires HEAD or a complete commit object ID");
    }
    let peeled = format!("{revision}^{{commit}}");
    let output = complete_git_output(
        workspace
            .change_probe(&["rev-parse", "--verify", "--end-of-options", &peeled])
            .await?,
    )?;
    let resolved = output.trim();
    if !crate::verification::change::full_oid(resolved) {
        bail!("Git commit identity is incomplete");
    }
    Ok(resolved.to_owned())
}

pub(super) async fn git_binding_inputs(
    workspace: &Workspace,
    root: &Path,
) -> Result<GitBindingInputs> {
    let head_sha = git_resolve_commit(workspace, "HEAD").await?;
    let tree = format!("{head_sha}^{{tree}}");
    let tree_args = ["rev-parse", "--verify", "--end-of-options", &tree];
    let status_args = [
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
        "--",
        ".",
    ];
    let index_args = ["ls-files", "--stage", "-z", "--", "."];
    let (tree, status, index) = tokio::try_join!(
        workspace.change_probe(&tree_args),
        workspace.change_probe(&status_args),
        workspace.change_probe(&index_args),
    )?;
    let tree_sha = complete_git_output(tree)?.trim().to_owned();
    let status = complete_git_output(status)?;
    let index = complete_git_output(index)?;
    if (!status.is_empty() && !status.ends_with('\0'))
        || (!index.is_empty() && !index.ends_with('\0'))
    {
        bail!("Git metadata records are incomplete");
    }
    // The repository and selected scope both participate in the private local
    // identity. Sub-Workspaces only hash their own status/index output.
    let identity = serde_json::to_vec(&(
        root.to_str().context("Git root is not UTF-8")?,
        workspace
            .root()
            .to_str()
            .context("Workspace root is not UTF-8")?,
    ))?;
    let binding = ExecutionGitBinding {
        repository: format!("sha256:{:x}", Sha256::digest(identity)),
        head_sha,
        tree_sha,
        dirty: !status.is_empty(),
        index_fingerprint: format!("sha256:{:x}", Sha256::digest(index.as_bytes())),
    };
    if !binding.valid() {
        bail!("Git execution binding is incomplete");
    }
    Ok(GitBindingInputs {
        binding,
        status,
        index,
    })
}

pub(super) async fn capture_git_candidate(
    workspace: &Workspace,
    snapshot: &mut GitChangeSnapshot,
) -> Result<()> {
    let Some(root) = git_repository_root(workspace).await? else {
        snapshot.unknown("not_git");
        return Ok(());
    };
    if root != workspace.root() {
        snapshot.unknown("workspace_not_git_root");
        return Ok(());
    }
    let before = git_binding_inputs(workspace, &root).await?;
    snapshot.binding = Some(before.binding.clone());
    let base = git_resolve_commit(workspace, &snapshot.requested_base).await?;
    let target = match &snapshot.target {
        GitChangeTarget::Worktree => before.binding.head_sha.clone(),
        GitChangeTarget::Commit { revision } => git_resolve_commit(workspace, revision).await?,
    };
    snapshot.base_sha = Some(base.clone());
    snapshot.target_sha = Some(target.clone());
    if matches!(snapshot.target, GitChangeTarget::Commit { .. }) {
        if before.binding.dirty {
            snapshot.unknown("dirty_commit_candidate");
        }
        if before.binding.head_sha != target {
            snapshot.unknown("target_not_current_head");
        }
        if !snapshot.unknown_reasons.is_empty() {
            return Ok(());
        }
    }
    let mut args = vec![
        "diff",
        "--raw",
        "-z",
        "--no-abbrev",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        &base,
    ];
    if matches!(snapshot.target, GitChangeTarget::Commit { .. }) {
        args.push(&target);
    }
    args.extend(["--", "."]);
    let raw = complete_git_output(workspace.change_probe(&args).await?)?;
    snapshot.changes = parse_raw_changes(&raw, MAX_REVIEW_FILES)?;
    if matches!(snapshot.target, GitChangeTarget::Worktree) {
        for change in parse_status_changes(&before.status, MAX_REVIEW_FILES)? {
            if change.status == "??" {
                if snapshot.changes.len() == MAX_REVIEW_FILES {
                    bail!("Git change path limit exceeded");
                }
                snapshot.changes.push(change);
            }
        }
    }
    let after = git_binding_inputs(workspace, &root).await?;
    let raw_after = complete_git_output(workspace.change_probe(&args).await?)?;
    if before != after
        || raw != raw_after
        || git_repository_root(workspace).await?.as_ref() != Some(&root)
    {
        bail!("Git metadata changed during candidate capture");
    }
    let mut executable = false;
    let mut regular = true;
    let mut known_modes = true;
    let mut unmerged = false;
    for change in &snapshot.changes {
        if change.status == "??" || (change.old_mode.is_none() && change.new_mode.is_none()) {
            known_modes = false;
        }
        unmerged |= change.status.starts_with('U');
        for mode in change.old_mode.iter().chain(change.new_mode.iter()) {
            match mode.as_str() {
                "100644" => {}
                "100755" => executable = true,
                "120000" | "160000" => regular = false,
                _ => known_modes = false,
            }
        }
    }
    if unmerged {
        snapshot.unknown("unmerged_change");
    }
    if known_modes {
        snapshot.executable_changes = Some(executable);
        snapshot.regular_file_changes = Some(regular);
    } else {
        snapshot.unknown("file_mode_unknown");
    }
    snapshot.complete = snapshot.unknown_reasons.is_empty();
    // These guards compare bounded metadata, not an atomic worktree byte
    // snapshot. Acceptance must also bind the canonical content revision.
    Ok(())
}
