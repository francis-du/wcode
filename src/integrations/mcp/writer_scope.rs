use crate::workspace::Workspace;
use serde_json::Value;
use std::path::{Component, Path};

pub(crate) fn mutation_domain_scopes(
    workspace: &Workspace,
    domain: &Path,
    scopes: &[String],
) -> Result<Vec<String>, String> {
    if scopes.is_empty() {
        return Ok(Vec::new());
    }
    scopes
        .iter()
        .map(|scope| mutation_domain_path(workspace, domain, scope))
        .collect()
}

pub(crate) fn mutation_domain_path(
    workspace: &Workspace,
    domain: &Path,
    path: &str,
) -> Result<String, String> {
    let relative = Workspace::normalize_relative_scope(path).map_err(|error| error.to_string())?;
    let prefix = workspace
        .root()
        .strip_prefix(domain)
        .map_err(|_| "workspace root is outside its mutation domain".to_owned())?;
    let prefix = portable_path(prefix);
    Ok(match (prefix.is_empty(), relative.is_empty()) {
        (true, _) => relative,
        (_, true) => prefix,
        _ => format!("{prefix}/{relative}"),
    })
}

pub(crate) fn scopes_allow_path(scopes: &[String], path: &str) -> bool {
    scopes.is_empty()
        || (!path.is_empty()
            && scopes
                .iter()
                .any(|scope| crate::reconcile::scope_contains(scope, path)))
}

pub(crate) fn mutation_paths(tool_name: &str, args: &Value) -> Result<Option<Vec<String>>, String> {
    match tool_name {
        "replace_text" | "apply_edits" | "write_file" | "create_directory" | "create_file"
        | "delete_path" => Ok(Some(vec![required_string(args, "path")?.to_owned()])),
        "create_files" | "apply_file_edits" => Ok(Some(array_paths(args, "files", &["path"])?)),
        "move_path" => Ok(Some(vec![
            required_string(args, "source")?.to_owned(),
            required_string(args, "destination")?.to_owned(),
        ])),
        "move_paths" => Ok(Some(array_paths(
            args,
            "moves",
            &["source", "destination"],
        )?)),
        "design_init" => Ok(Some(vec![".wcode".to_owned()])),
        "run_command" => Ok(None),
        _ => Err(format!(
            "writer_scope_unknown: mutating tool '{tool_name}' has no bounded path classifier"
        )),
    }
}

fn array_paths(args: &Value, key: &str, fields: &[&str]) -> Result<Vec<String>, String> {
    let items = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("writer scope classification requires array argument '{key}'"))?;
    let mut paths = Vec::with_capacity(items.len().saturating_mul(fields.len()));
    for item in items {
        for field in fields {
            paths.push(required_string(item, field)?.to_owned());
        }
    }
    Ok(paths)
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("writer scope classification requires string argument '{key}'"))
}

fn portable_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}
