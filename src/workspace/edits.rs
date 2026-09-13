use super::*;

impl Workspace {
    pub fn replace_text(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
        expected_sha256: &str,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        if old_text.is_empty() {
            bail!("old_text must not be empty");
        }
        let file = self.existing_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.existing_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the edit");
        }
        ensure_single_link_file(&file)?;
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let before = sha256(content.as_bytes());
        if before != expected_sha256 {
            bail!("stale file: expected sha256 {expected_sha256}, current sha256 is {before}");
        }
        let count = content.matches(old_text).count();
        if count != 1 {
            bail!("old_text must occur exactly once; found {count} matches");
        }
        let updated = content.replacen(old_text, new_text, 1);
        validate_write_content(&updated)?;
        let relative = portable_relative_path(file.strip_prefix(&self.root)?);
        crate::conventions::validate_source_write(&relative, Some(&content), &updated)?;
        reject_destructive_replacement(&content, &updated, self.security)?;
        atomic_write(&file, updated.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: Some(before),
            sha256_after: sha256(updated.as_bytes()),
            bytes_written: updated.len(),
        })
    }

    pub fn create_directory(&self, path: &str) -> Result<DirectoryResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            return Ok(DirectoryResult {
                path: ".".to_owned(),
                created: false,
            });
        }

        let mut current = self.root.clone();
        let mut created = false;
        for component in relative.components() {
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
                Ok(metadata) if !metadata.is_dir() => {
                    bail!(
                        "directory path collides with a non-directory: {}",
                        current.display()
                    )
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).with_context(|| {
                        format!("cannot create directory {}", current.display())
                    })?;
                    created = true;
                }
                Err(error) => return Err(error.into()),
            }
        }

        Ok(DirectoryResult {
            path: portable_relative_path(current.strip_prefix(&self.root)?),
            created,
        })
    }

    pub(crate) fn ensure_directory(&self, path: &str) -> Result<()> {
        self.create_directory(path).map(|_| ())
    }

    pub fn path_info(&self, path: &str) -> Result<PathInfo> {
        let resolved = self.existing_path(path)?;
        let metadata = fs::symlink_metadata(&resolved)?;
        let kind = if metadata.is_file() {
            "file"
        } else if metadata.is_dir() {
            "directory"
        } else {
            bail!("path is neither a regular file nor a directory");
        };
        let digest = metadata
            .is_file()
            .then(|| sha256_file(&resolved))
            .transpose()?;
        let modified_at_ms = metadata.modified().ok().and_then(|modified| {
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        });
        Ok(PathInfo {
            path: portable_relative_path(resolved.strip_prefix(&self.root)?),
            kind: kind.to_owned(),
            size: metadata.len(),
            sha256: digest,
            readonly: metadata.permissions().readonly(),
            modified_at_ms,
            hard_links: hard_link_count(&metadata),
        })
    }

    pub fn write_file(
        &self,
        path: &str,
        content: &str,
        expected_sha256: Option<&str>,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        validate_write_content(content)?;
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("file path is required");
        }
        let candidate = self.root.join(&relative);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let expected = expected_sha256.ok_or_else(|| {
                    anyhow!("expected_sha256 is required when overwriting an existing file")
                })?;
                let file = self.existing_path(path)?;
                let file_lock = self.write_lock_for(&file)?;
                let _write_guard = file_lock
                    .lock()
                    .map_err(|_| anyhow!("file write lock poisoned"))?;
                let locked_file = self.existing_path(path)?;
                if locked_file != file {
                    bail!("path target changed while waiting for the write lock; retry the write");
                }
                ensure_single_link_file(&file)?;
                let before_content =
                    fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
                let before = sha256(before_content.as_bytes());
                if before != expected {
                    bail!("stale file: expected sha256 {expected}, current sha256 is {before}");
                }
                let relative = portable_relative_path(file.strip_prefix(&self.root)?);
                crate::conventions::validate_source_write(
                    &relative,
                    Some(&before_content),
                    content,
                )?;
                reject_destructive_replacement(&before_content, content, self.security)?;
                atomic_write(&file, content.as_bytes())?;
                Ok(EditResult {
                    path: portable_relative_path(file.strip_prefix(&self.root)?),
                    sha256_before: Some(before),
                    sha256_after: sha256(content.as_bytes()),
                    bytes_written: content.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if expected_sha256.is_some() {
                    bail!("expected_sha256 was supplied but the target file does not exist");
                }
                self.create_file(path, content)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn apply_edits(
        &self,
        path: &str,
        edits: &[TextEdit],
        expected_sha256: &str,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        if edits.is_empty() || edits.len() > MAX_TEXT_EDITS {
            bail!("edits must contain between 1 and {MAX_TEXT_EDITS} entries");
        }
        let file = self.existing_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.existing_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the edits");
        }
        ensure_single_link_file(&file)?;
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let before = sha256(content.as_bytes());
        if before != expected_sha256 {
            bail!("stale file: expected sha256 {expected_sha256}, current sha256 is {before}");
        }
        let updated = apply_text_edits(&content, edits)?;
        validate_write_content(&updated)?;
        let relative = portable_relative_path(file.strip_prefix(&self.root)?);
        crate::conventions::validate_source_write(&relative, Some(&content), &updated)?;
        reject_destructive_replacement(&content, &updated, self.security)?;
        atomic_write(&file, updated.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: Some(before),
            sha256_after: sha256(updated.as_bytes()),
            bytes_written: updated.len(),
        })
    }

    pub fn create_file(&self, path: &str, content: &str) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        validate_write_content(content)?;
        let file = self.new_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.new_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the create");
        }
        let relative = portable_relative_path(file.strip_prefix(&self.root)?);
        crate::conventions::validate_source_write(&relative, None, content)?;
        atomic_create_new(&file, content.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: None,
            sha256_after: sha256(content.as_bytes()),
            bytes_written: content.len(),
        })
    }

    pub fn create_files(&self, files: &[CreateFileRequest]) -> Result<Vec<BatchEditItem>> {
        validate_batch_paths(files.iter().map(|file| file.path.as_str()))?;
        crate::resource::parallel_io(files, |file| {
            match self.create_file(&file.path, &file.content) {
                Ok(result) => BatchEditItem {
                    path: file.path.clone(),
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => BatchEditItem {
                    path: file.path.clone(),
                    ok: false,
                    result: None,
                    error: Some(error.to_string()),
                },
            }
        })
    }

    pub fn apply_file_edits(&self, files: &[FileEditRequest]) -> Result<Vec<BatchEditItem>> {
        validate_batch_paths(files.iter().map(|file| file.path.as_str()))?;
        crate::resource::parallel_io(files, |file| {
            match self.apply_edits(&file.path, &file.edits, &file.expected_sha256) {
                Ok(result) => BatchEditItem {
                    path: file.path.clone(),
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => BatchEditItem {
                    path: file.path.clone(),
                    ok: false,
                    result: None,
                    error: Some(error.to_string()),
                },
            }
        })
    }
}
