use super::github::GitHubConfig;
use super::ProviderRepository;
use crate::workspace::{Workspace, WorkspaceSecurity};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const GITHUB_ENROLLMENT_FILE: &str = ".wcode/github-publisher.yaml";
const SCHEMA_VERSION: u32 = 1;
const MAX_ENROLLMENT_BYTES: u64 = 16 * 1024;
const DEFAULT_CHECK_NAME: &str = "wcode/change-acceptance";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEnrollment {
    pub schema_version: u32,
    pub repository: String,
    pub repository_id: u64,
    pub app_id: u64,
    pub check_name: String,
}

impl GitHubEnrollment {
    pub fn new(
        repository: &str,
        repository_id: u64,
        app_id: u64,
        check_name: Option<&str>,
    ) -> Result<Self> {
        let enrollment = Self {
            schema_version: SCHEMA_VERSION,
            repository: repository.to_ascii_lowercase(),
            repository_id,
            app_id,
            check_name: check_name.unwrap_or(DEFAULT_CHECK_NAME).to_owned(),
        };
        enrollment.github_config()?;
        Ok(enrollment)
    }

    pub fn github_config(&self) -> Result<GitHubConfig> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported GitHub enrollment schema version {}",
                self.schema_version
            );
        }
        let (namespace, name) = self
            .repository
            .split_once('/')
            .filter(|(_, name)| !name.contains('/'))
            .ok_or_else(|| anyhow::anyhow!("GitHub enrollment repository must be owner/repo"))?;
        GitHubConfig::new(
            ProviderRepository::new(namespace, name)?,
            self.repository_id,
            self.app_id,
            &self.check_name,
        )
    }

    pub fn load(root: impl AsRef<Path>) -> Result<Option<Self>> {
        let workspace =
            Workspace::new_with_security(root, false, false, WorkspaceSecurity::default())?;
        Self::load_workspace(&workspace)
    }

    pub(crate) fn load_workspace(workspace: &Workspace) -> Result<Option<Self>> {
        // A missing leaf is not evidence that its parent is safe. Resolve the
        // existing directory through Workspace guards before accepting absence.
        match fs::symlink_metadata(workspace.root().join(".wcode")) {
            Ok(_) => {
                let parent = workspace.path_info(".wcode")?;
                if parent.kind != "directory" {
                    bail!("GitHub enrollment parent must be a directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("cannot inspect GitHub enrollment directory"),
        }
        let path = workspace.root().join(GITHUB_ENROLLMENT_FILE);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot inspect GitHub enrollment {}", path.display())
                })
            }
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("GitHub enrollment must be a regular non-symlink file");
        }
        if metadata.len() == 0 || metadata.len() > MAX_ENROLLMENT_BYTES {
            bail!("GitHub enrollment is empty or exceeds its byte bound");
        }
        let source = workspace.load_source(GITHUB_ENROLLMENT_FILE)?;
        if source.content.len() as u64 > MAX_ENROLLMENT_BYTES {
            bail!("GitHub enrollment exceeds its byte bound");
        }
        let enrollment: Self =
            serde_yaml::from_str(&source.content).context("invalid GitHub enrollment")?;
        enrollment.github_config()?;
        Ok(Some(enrollment))
    }

    pub(crate) fn create_in_workspace(&self, workspace: &Workspace) -> Result<()> {
        self.github_config()?;
        let encoded = serde_yaml::to_string(self).context("cannot encode GitHub enrollment")?;
        if encoded.len() as u64 > MAX_ENROLLMENT_BYTES {
            bail!("GitHub enrollment exceeds its byte bound");
        }
        workspace.create_file(GITHUB_ENROLLMENT_FILE, &encoded)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/enrollment.rs"]
mod tests;
