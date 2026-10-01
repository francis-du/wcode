//! Cooperative single-writer admission for one remote App/Check/head binding.
//! Stable empty lock files are never unlinked: deleting one can split ownership
//! across two inodes. This is not a distributed lease or remote request fence.
//! File lock semantics: https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock
use super::*;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub(super) struct PublicationBusy;
impl std::fmt::Display for PublicationBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GitHub publication busy: another local publisher owns this App/Check/head; no Check write started")
    }
}
impl std::error::Error for PublicationBusy {}

pub(super) struct PublicationGuard {
    file: File,
    path: PathBuf,
    key: String,
}
impl Drop for PublicationGuard {
    fn drop(&mut self) {
        // Release even when another duplicated descriptor still exists. Do not
        // remove the inode, expire an active holder or change someone else's lock.
        let _ = self.file.unlock();
    }
}

impl PublicationGuard {
    pub(super) fn acquire(provider: &GitHubProvider, target: &ProviderTarget) -> Result<Self> {
        let key = binding_key(provider, target)?;
        let root = coordination_root()
            .map_err(|_| anyhow!("GitHub publication coordination root unavailable"))?;
        Self::acquire_at(&root, key)
    }

    fn acquire_at(root: &Path, key: String) -> Result<Self> {
        ensure!(
            key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid publication coordination identity"
        );
        ensure_directory(root).map_err(|_| {
            anyhow!("GitHub publication coordination directory is unsafe or unavailable")
        })?;
        let path = root.join(format!("{key}.lock"));
        // create_new wins atomically; an existing file is opened without truncate
        // only after the same private-path checks used by the durable inbox.
        let file = match inbox::create_file(&path) {
            Ok(file) => file,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                inbox::open_regular(&path, true, 0)
                    .map_err(|_| anyhow!("GitHub publication lock is unsafe or unavailable"))?
            }
            Err(_) => bail!("GitHub publication lock is unsafe or unavailable"),
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(PublicationBusy.into()),
            Err(_) => bail!("GitHub publication locking unavailable; no Check write started"),
        }
        let guard = Self { file, path, key };
        guard.check_path()?;
        Ok(guard)
    }

    pub(super) fn check(&self, provider: &GitHubProvider, target: &ProviderTarget) -> Result<()> {
        ensure!(
            self.key == binding_key(provider, target)?,
            "publication guard belongs to another remote Check binding"
        );
        self.check_path()
    }

    fn check_path(&self) -> Result<()> {
        let result = (|| -> Result<()> {
            let parent = self
                .path
                .parent()
                .ok_or_else(|| anyhow!("lock parent missing"))?;
            inbox::validate_root(parent, false)?;
            inbox::private_metadata(parent, true)?;
            let current = inbox::private_metadata(&self.path, false)?;
            let held = self.file.metadata()?;
            ensure!(
                current.len() == 0 && held.is_file() && held.len() == 0,
                "publication lock contents changed"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(
                    current.dev() == held.dev() && current.ino() == held.ino() && held.nlink() == 1,
                    "publication lock inode changed"
                );
            }
            Ok(())
        })();
        result.map_err(|_| anyhow!("GitHub publication coordination identity changed or unavailable; no Check write started"))
    }
}

fn binding_key(provider: &GitHubProvider, target: &ProviderTarget) -> Result<String> {
    target.validate()?;
    ensure!(
        target.repository == provider.config.repository,
        "cross-repository publication refused"
    );
    // Checks are attached to a commit, not a local workspace or PR number. Two
    // PRs/base revisions sharing one head must contend, including after token
    // rotation or when operators use distinct candidate/config directories.
    let bytes = serde_json::to_vec(&(
        "wcode-github-publication-v1",
        provider.api.as_str(),
        provider.config.repository_id,
        provider.config.app_id,
        &provider.config.check_name,
        target.head_sha.to_ascii_lowercase(),
    ))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn coordination_root() -> Result<PathBuf> {
    #[cfg(test)]
    {
        // Unit fixtures must never mutate the operator's real state directory.
        Ok(std::env::temp_dir()
            .canonicalize()?
            .join("wcode-test-publication")
            .join(std::process::id().to_string()))
    }
    #[cfg(not(test))]
    {
        Ok(crate::core_types::authority_state_root()?.join("github-publication-locks"))
    }
}

fn ensure_directory(root: &Path) -> Result<()> {
    ensure!(
        root.is_absolute()
            && !root
                .components()
                .any(|c| matches!(c, Component::CurDir | Component::ParentDir)),
        "normalized absolute lock root required"
    );
    // Validate every existing ancestor before creating even the first directory.
    let mut missing = Vec::new();
    for path in root.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(_) => inbox::validate_root(path, false)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing.push(path),
            Err(error) => return Err(error.into()),
        }
    }
    for path in missing.into_iter().rev() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        inbox::validate_root(path, false)?;
        inbox::private_metadata(path, true)?;
    }
    inbox::private_metadata(root, true)?;
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/publish_lock.rs"]
mod tests;
