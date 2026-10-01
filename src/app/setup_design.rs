use anyhow::{anyhow, ensure, Result};
use serde::Serialize;
use std::fs;
use std::sync::Mutex;

use crate::design::{self, ProjectDesign};
use crate::workspace::{PathInfo, Workspace};

// Setup is infrequent; serialize only this local bootstrap, not ordinary reads
// or repository tools. Atomic create still prevents cross-process overwrite.
static INITIALIZATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize)]
pub(super) struct DesignInitialization {
    pub status: &'static str,
    pub planned_paths: Vec<&'static str>,
    pub diagnostics: Vec<&'static str>,
    /// This operation never activates Policy; an earlier activation is unchanged.
    pub policy_activated: bool,
}

impl DesignInitialization {
    fn report(status: &'static str, diagnostic: Option<&'static str>) -> Self {
        Self {
            status,
            planned_paths: if matches!(status, "planned" | "initialized") {
                vec![
                    design::PROJECT_FILE,
                    design::DESIGN_ROOT,
                    ".wcode/design/product.yaml",
                    ".wcode/design/constraints.yaml",
                ]
            } else {
                Vec::new()
            },
            diagnostics: diagnostic.into_iter().collect(),
            policy_activated: false,
        }
    }
}

pub(super) fn initialize(workspace: &Workspace, dry_run: bool) -> Result<DesignInitialization> {
    let _guard = INITIALIZATION_LOCK
        .lock()
        .map_err(|_| anyhow!("project initialization lock poisoned"))?;
    let inspection = (|| -> Result<_> {
        let root = workspace.path_info(".")?;
        let directory = inspect_path(workspace, ".wcode", "directory")?;
        let project = inspect_path(workspace, design::PROJECT_FILE, "file")?;
        let design = inspect_path(workspace, design::DESIGN_ROOT, "directory")?;
        Ok((root, directory, project, design))
    })();
    let (root, directory, project, design_directory) = match inspection {
        Ok(paths) => paths,
        Err(_) => {
            return Ok(DesignInitialization::report(
                "blocked",
                Some("unsafe_or_unreadable_design_path"),
            ))
        }
    };
    let load = match design::load_design(workspace) {
        Ok(load) => load,
        Err(_) => {
            return Ok(DesignInitialization::report(
                "needs_repair",
                Some("design_load_failed"),
            ))
        }
    };
    if load.initialized || project.is_some() || design_directory.is_some() {
        let valid = project.is_some()
            && design_directory.is_some()
            && load.state.project.is_some()
            && load.state.product.is_some()
            && load.error_count() == 0;
        return Ok(DesignInitialization::report(
            if valid { "existing" } else { "needs_repair" },
            (!valid).then_some("existing_design_requires_review"),
        ));
    }
    if dry_run {
        return Ok(DesignInitialization::report("planned", None));
    }
    if !workspace.write_enabled()
        || root.readonly
        || directory.as_ref().is_some_and(|info| info.readonly)
    {
        return Ok(DesignInitialization::report(
            "blocked",
            Some("design_writes_disabled"),
        ));
    }
    let name = workspace
        .root()
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
        .map(|name| name.chars().take(200).collect::<String>())
        .unwrap_or_else(|| "Project".into());
    let metadata = ProjectDesign {
        schema_version: 1,
        name: name.clone(),
        description: String::new(),
        acceptance_policy: None,
    };
    let product = design::ProductDesign {
        schema_version: 1,
        id: "product:project".into(),
        name,
        vision: String::new(),
        principles: Vec::new(),
    };
    let project_content = serde_yaml::to_string(&metadata)?;
    let product_content = serde_yaml::to_string(&product)?;
    let constraints_content = serde_yaml::to_string(&design::baseline_constraints())?;
    let write = (|| -> Result<()> {
        workspace.ensure_directory(".wcode")?;
        workspace.ensure_directory(design::DESIGN_ROOT)?;
        workspace.create_file(design::PROJECT_FILE, &project_content)?;
        workspace.create_file(".wcode/design/product.yaml", &product_content)?;
        workspace.create_file(".wcode/design/constraints.yaml", &constraints_content)?;
        Ok(())
    })();
    if write.is_err() {
        // Never overwrite or retry an uncertain write. Preserve any partial
        // result for explicit review, including a concurrent writer's metadata.
        return Ok(DesignInitialization::report(
            "needs_repair",
            Some("initialization_interrupted_no_overwrite"),
        ));
    }
    let verified = (|| -> Result<bool> {
        inspect_path(workspace, design::PROJECT_FILE, "file")?;
        inspect_path(workspace, design::DESIGN_ROOT, "directory")?;
        let load = design::load_design(workspace)?;
        Ok(load.error_count() == 0 && load.state.project.is_some() && load.state.product.is_some())
    })()
    .unwrap_or(false);
    Ok(DesignInitialization::report(
        if verified {
            "initialized"
        } else {
            "needs_repair"
        },
        (!verified).then_some("initialized_design_requires_review"),
    ))
}

fn inspect_path(workspace: &Workspace, path: &str, kind: &str) -> Result<Option<PathInfo>> {
    match fs::symlink_metadata(workspace.root().join(path)) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    // Native resolution checks root identity, containment, protected authority
    // paths and every symlink component; the metadata probe grants no access.
    let info = workspace.path_info(path)?;
    ensure!(info.kind == kind, "unexpected Design path type");
    ensure!(
        kind != "file" || info.hard_links.is_none_or(|links| links <= 1),
        "aliased Design file"
    );
    Ok(Some(info))
}

pub(super) fn print(report: &DesignInitialization, output: &mut impl std::io::Write) -> Result<()> {
    writeln!(
        output,
        "Project Design: {} (Policy not activated by setup).",
        report.status
    )?;
    for diagnostic in &report.diagnostics {
        writeln!(output, "  {diagnostic}")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/app/setup_design.rs"]
mod tests;
