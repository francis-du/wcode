use crate::workspace::Workspace;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use toml_edit::{value, Array, DocumentMut, Item, Table};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileOutcome {
    Create,
    Update,
    Already,
    ManualConflict,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedFile {
    pub target: String,
    pub method: String,
    pub outcome: FileOutcome,
    content: Option<String>,
    expected_sha256: Option<String>,
}

pub(crate) fn plan_json(
    workspace: &Workspace,
    target: &str,
    container: &str,
    server: &Value,
) -> Result<PlannedFile> {
    let existing = read_existing(workspace, target)?;
    let mut root = match existing.as_ref() {
        Some((content, _)) => serde_json::from_str::<Value>(content)
            .with_context(|| format!("{target} is not valid JSON"))?,
        None => Value::Object(Map::new()),
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{target} must contain a JSON object"))?;
    if object
        .get("$schema")
        .is_some_and(|schema| !schema.is_string())
    {
        bail!("{target} has an unsupported non-string $schema field");
    }
    let servers = object
        .entry(container.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{target} field {container} must be a JSON object"))?;
    if servers.get("wcode") == Some(server) {
        return Ok(unchanged(target, format!("merge {container}.wcode")));
    }
    servers.insert("wcode".to_owned(), server.clone());
    let mut content = serde_json::to_string_pretty(&root)?;
    content.push('\n');
    Ok(changed(
        target,
        format!("merge {container}.wcode"),
        content,
        existing,
    ))
}

pub(crate) fn plan_codex_toml(
    workspace: &Workspace,
    target: &str,
    workspace_root: &Path,
) -> Result<PlannedFile> {
    let existing = read_existing(workspace, target)?;
    let mut document = existing
        .as_ref()
        .map(|(content, _)| content.parse::<DocumentMut>())
        .transpose()
        .with_context(|| format!("{target} is not valid TOML"))?
        .unwrap_or_default();
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{target} field mcp_servers must be a TOML table"))?;
    if servers.get("wcode").is_none() {
        servers["wcode"] = Item::Table(Table::new());
    }
    let wcode = servers["wcode"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{target} field mcp_servers.wcode must be a TOML table"))?;
    let root = workspace_root
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    let desired_args = ["--workspace", root, "mcp-stdio"];
    let already = wcode.get("command").and_then(Item::as_str) == Some("wcode")
        && wcode
            .get("args")
            .and_then(Item::as_array)
            .is_some_and(|args| {
                args.iter()
                    .filter_map(|item| item.as_str())
                    .eq(desired_args)
                    && args.len() == desired_args.len()
            });
    if already {
        return Ok(unchanged(target, "merge mcp_servers.wcode".to_owned()));
    }
    wcode["command"] = value("wcode");
    let mut args = Array::new();
    for argument in desired_args {
        args.push(argument);
    }
    wcode["args"] = value(args);
    Ok(changed(
        target,
        "merge mcp_servers.wcode".to_owned(),
        document.to_string(),
        existing,
    ))
}

pub(crate) fn plan_opencode(
    workspace: &Workspace,
    target: &str,
    workspace_root: &Path,
) -> Result<PlannedFile> {
    let existing = read_existing(workspace, target)?;
    let mut root = match existing.as_ref() {
        Some((content, _)) => serde_json::from_str::<Value>(content).with_context(|| {
            format!("{target} is not valid JSON; JSONC must be merged manually")
        })?,
        None => Value::Object(Map::new()),
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{target} must contain a JSON object"))?;
    if object
        .get("$schema")
        .is_some_and(|schema| !schema.is_string())
    {
        bail!("{target} has an unsupported non-string $schema field");
    }
    let mcp = object
        .entry("mcp".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{target} field mcp must be a JSON object"))?;
    let v2 = mcp.contains_key("servers");
    let servers = if v2 {
        mcp.get_mut("servers")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| anyhow!("{target} field mcp.servers must be a JSON object"))?
    } else {
        mcp
    };
    let root_path = workspace_root
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    let server = serde_json::json!({
        "type": "local",
        "command": ["wcode", "--workspace", root_path, "mcp-stdio"]
    });
    if servers.get("wcode") == Some(&server) {
        return Ok(unchanged(
            target,
            if v2 {
                "merge mcp.servers.wcode"
            } else {
                "merge mcp.wcode"
            }
            .to_owned(),
        ));
    }
    servers.insert("wcode".to_owned(), server);
    let mut content = serde_json::to_string_pretty(&root)?;
    content.push('\n');
    Ok(changed(
        target,
        if v2 {
            "merge mcp.servers.wcode"
        } else {
            "merge mcp.wcode"
        }
        .to_owned(),
        content,
        existing,
    ))
}

pub(crate) fn plan_canonical_text(
    workspace: &Workspace,
    target: &str,
    content: &str,
) -> Result<PlannedFile> {
    let existing = read_existing(workspace, target)?;
    match existing {
        None => Ok(PlannedFile {
            target: target.to_owned(),
            method: "install canonical portable skill".to_owned(),
            outcome: FileOutcome::Create,
            content: Some(content.to_owned()),
            expected_sha256: None,
        }),
        Some((current, _)) if current == content => Ok(unchanged(
            target,
            "install canonical portable skill".to_owned(),
        )),
        Some(_) => Ok(PlannedFile {
            target: target.to_owned(),
            method: "preserve locally modified skill".to_owned(),
            outcome: FileOutcome::ManualConflict,
            content: None,
            expected_sha256: None,
        }),
    }
}

pub(crate) fn apply(workspace: &Workspace, plan: &PlannedFile) -> Result<()> {
    let Some(content) = plan.content.as_deref() else {
        return Ok(());
    };
    if let Some(parent) = Path::new(&plan.target).parent() {
        let parent = parent.to_string_lossy();
        if !parent.is_empty() {
            workspace.ensure_directory(&parent)?;
        }
    }
    match plan.outcome {
        FileOutcome::Create => workspace.create_file(&plan.target, content).map(|_| ()),
        FileOutcome::Update => workspace
            .write_file(&plan.target, content, plan.expected_sha256.as_deref())
            .map(|_| ()),
        FileOutcome::Already | FileOutcome::ManualConflict => Ok(()),
    }
}

fn read_existing(workspace: &Workspace, target: &str) -> Result<Option<(String, String)>> {
    let path = workspace.root().join(target);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{target} must be a regular non-symlink file");
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        bail!("{target} exceeds the 1 MiB safe configuration merge bound");
    }
    let content = fs::read_to_string(&path).with_context(|| format!("cannot read {target}"))?;
    let sha = format!("{:x}", Sha256::digest(content.as_bytes()));
    Ok(Some((content, sha)))
}

fn changed(
    target: &str,
    method: String,
    content: String,
    existing: Option<(String, String)>,
) -> PlannedFile {
    let expected_sha256 = existing.as_ref().map(|(_, sha)| sha.clone());
    PlannedFile {
        target: target.to_owned(),
        method,
        outcome: if existing.is_some() {
            FileOutcome::Update
        } else {
            FileOutcome::Create
        },
        content: Some(content),
        expected_sha256,
    }
}

fn unchanged(target: &str, method: String) -> PlannedFile {
    PlannedFile {
        target: target.to_owned(),
        method,
        outcome: FileOutcome::Already,
        content: None,
        expected_sha256: None,
    }
}
