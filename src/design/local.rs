//! Read-only native Design access for trusted local integration hosts.
use super::{load_design, DesignLoad};
use crate::workspace::{Workspace, WorkspaceSecurity};
use anyhow::Result;
use std::path::Path;

/// Load canonical bounded Design metadata without exposing Workspace execution
/// or write capabilities. The explicit local path is not a remote access grant,
/// and returned metadata cannot activate Policy or establish verification proof.
pub fn load_local_design(root: impl AsRef<Path>) -> Result<DesignLoad> {
    let workspace = Workspace::new_with_security(root, false, false, WorkspaceSecurity::default())?;
    load_design(&workspace)
}

#[cfg(test)]
#[path = "../../tests/unit/design/local.rs"]
mod tests;
