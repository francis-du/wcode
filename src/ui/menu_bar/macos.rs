use super::*;
use std::fs::TryLockError;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(super) struct InstanceLock {
    _file: File,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // A concurrent fork can inherit this open file description until exec.
        // Its lifetime must not extend the menu bar owner's exclusive lock.
        let _ = self._file.unlock();
    }
}

impl InstanceLock {
    #[cfg(test)]
    pub(super) fn clone_file_for_test(&self) -> std::io::Result<File> {
        self._file.try_clone()
    }

    fn acquire() -> Result<Option<Self>> {
        Self::acquire_at(&crate::auth::authority_state_root()?)
    }

    pub(super) fn acquire_at(authority: &Path) -> Result<Option<Self>> {
        if !authority.is_absolute() {
            anyhow::bail!("menu bar state directory must be absolute");
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(authority)?;
        let directory = fs::symlink_metadata(authority)?;
        // Never chmod an existing path supplied through WCODE_STATE_DIR.
        let uid = unsafe { libc::geteuid() };
        if !directory.is_dir()
            || directory.file_type().is_symlink()
            || directory.uid() != uid
            || directory.mode() & 0o022 != 0
        {
            anyhow::bail!(
                "menu bar state directory must be owned by you and not writable by others"
            );
        }
        let path = authority.join("menu-bar.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let validate = || -> Result<()> {
            let held = file.metadata()?;
            let named = fs::symlink_metadata(&path)?;
            let current_directory = fs::symlink_metadata(authority)?;
            if !held.is_file()
                || held.len() != 0
                || held.nlink() != 1
                || held.uid() != uid
                || held.mode() & 0o077 != 0
                || named.file_type().is_symlink()
                || held.ino() != named.ino()
                || held.dev() != named.dev()
                || directory.ino() != current_directory.ino()
                || directory.dev() != current_directory.dev()
                || current_directory.mode() & 0o022 != 0
            {
                anyhow::bail!("menu bar lock is unsafe or changed during startup");
            }
            Ok(())
        };
        validate()?;
        match file.try_lock() {
            Ok(()) => {
                if let Err(error) = validate() {
                    let _ = file.unlock();
                    return Err(error);
                }
                Ok(Some(Self { _file: file }))
            }
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

pub(super) fn run() -> Result<()> {
    native::run(InstanceLock::acquire()?)
}
