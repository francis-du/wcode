use crate::workspace::Workspace;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
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
    pub launch_command: Option<String>,
    content: Option<String>,
    expected_sha256: Option<String>,
}

enum ServerMerge {
    Write(Value),
    Preserve(&'static str),
}

pub(crate) fn plan_json(
    workspace: &Workspace,
    target: &str,
    container: &str,
    server: &Value,
) -> Result<PlannedFile> {
    plan_json_with(workspace, target, container, false, |_| {
        Ok(ServerMerge::Write(server.clone()))
    })
}

pub(crate) fn plan_kimi_json(
    workspace: &Workspace,
    target: &str,
    server: &Value,
) -> Result<PlannedFile> {
    plan_json_with(workspace, target, "mcpServers", true, |existing| {
        let Some(existing) = existing else {
            return Ok(ServerMerge::Write(server.clone()));
        };
        let entry = existing
            .as_object()
            .ok_or_else(|| anyhow!("{target} mcpServers.wcode must be an object"))?;
        if entry.get("enabled") == Some(&Value::Bool(false)) {
            return Ok(ServerMerge::Preserve(
                "WCode is disabled in Kimi; enable it explicitly via /mcp-config if desired",
            ));
        }
        if entry.contains_key("url") {
            return Ok(ServerMerge::Preserve(
                "WCode points to a remote server in Kimi; review it via /mcp-config before switching to stdio",
            ));
        }
        let mut entry = entry.clone();
        for key in ["command", "args"] {
            entry.insert(key.to_owned(), server[key].clone());
        }
        Ok(ServerMerge::Write(Value::Object(entry)))
    })
}

fn plan_json_with(
    workspace: &Workspace,
    target: &str,
    container: &str,
    allow_empty: bool,
    merge_server: impl FnOnce(Option<&Value>) -> Result<ServerMerge>,
) -> Result<PlannedFile> {
    let existing = read_existing(workspace, target)?;
    let mut root = match existing.as_ref().filter(|(content, _)| {
        !allow_empty || !content.trim_matches([' ', '\t', '\r', '\n']).is_empty()
    }) {
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
    let current = servers.get("wcode");
    let server = match merge_server(current)? {
        ServerMerge::Write(server) => server,
        ServerMerge::Preserve(reason) => {
            return Ok(PlannedFile {
                outcome: FileOutcome::ManualConflict,
                ..unchanged(target, reason.to_owned())
            });
        }
    };
    if current == Some(&server) {
        return Ok(unchanged(target, format!("merge {container}.wcode")));
    }
    servers.insert("wcode".to_owned(), server);
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
    launch_args: &[String],
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
    let desired_args = launch_args;
    let already = wcode.get("command").and_then(Item::as_str) == Some("wcode")
        && wcode
            .get("args")
            .and_then(Item::as_array)
            .is_some_and(|args| {
                args.iter()
                    .filter_map(|item| item.as_str())
                    .eq(desired_args.iter().map(String::as_str))
                    && args.len() == desired_args.len()
            });
    if already {
        return Ok(unchanged(target, "merge mcp_servers.wcode".to_owned()));
    }
    wcode["command"] = value("wcode");
    let mut args = Array::new();
    for argument in desired_args {
        args.push(argument.as_str());
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
    launch_args: &[String],
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
    let server = serde_json::json!({
        "type": "local",
        "command": std::iter::once("wcode").chain(launch_args.iter().map(String::as_str)).collect::<Vec<_>>()
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
            launch_command: None,
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
            launch_command: None,
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
    let Some(path) = workspace.optional_merge_file(target)? else {
        return Ok(None);
    };
    let reader = fs::File::open(path).with_context(|| format!("cannot open {target}"))?;
    if reader.metadata()?.len() > MAX_CONFIG_BYTES {
        bail!("{target} exceeds the 1 MiB safe configuration merge bound");
    }
    let mut content = String::new();
    reader
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut content)
        .with_context(|| format!("cannot read {target}"))?;
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("{target} exceeds the 1 MiB safe configuration merge bound");
    }
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
        launch_command: None,
        content: Some(content),
        expected_sha256,
    }
}

fn unchanged(target: &str, method: String) -> PlannedFile {
    PlannedFile {
        target: target.to_owned(),
        method,
        outcome: FileOutcome::Already,
        launch_command: None,
        content: None,
        expected_sha256: None,
    }
}
