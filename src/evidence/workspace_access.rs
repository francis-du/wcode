use crate::workspace::Workspace;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

const MAX_ACTIVE_STORES: usize = 4096;

/// Each store owns a registry so nested journal -> failure-memory access uses
/// distinct gates. Workspace roots are canonicalized at construction.
pub(super) struct WorkspaceStoreAccess {
    gates: Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>,
}

impl WorkspaceStoreAccess {
    pub(super) const fn new() -> Self {
        Self {
            gates: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn for_workspace(&self, workspace: &Workspace) -> Result<Arc<Mutex<()>>> {
        let mut gates = self
            .gates
            .lock()
            .map_err(|_| anyhow::anyhow!("workspace store access registry is poisoned"))?;
        if let Some(gate) = gates.get(workspace.root()).and_then(Weak::upgrade) {
            return Ok(gate);
        }
        if gates.len() >= MAX_ACTIVE_STORES {
            gates.retain(|_, gate| gate.strong_count() > 0);
        }
        if gates.len() >= MAX_ACTIVE_STORES {
            bail!("workspace store access capacity is exhausted");
        }
        let gate = Arc::new(Mutex::new(()));
        gates.insert(workspace.root().to_owned(), Arc::downgrade(&gate));
        Ok(gate)
    }
}
