//! How one leaf tool call is identified and recorded for operator observation.
//!
//! `task_detail` is a free-form summary of the tool's arguments. The local TUI
//! renders it; the protected HTTP projection deliberately does not, so an
//! argument-bearing summary cannot leave the machine. `task_target` is the one
//! structured identity that projection may carry: a single workspace-relative
//! path, bounded and single-line, used only to link a task to the code it
//! touched. Neither value is an authorization input, and neither widens what a
//! tool may access.

use serde_json::Value;

use super::mcp_tools::{string_arg, task_detail};
use crate::monitor::{TaskMonitor, TaskTicket};
use crate::workspace::Workspace;

pub(super) fn task_target(name: &str, args: &Value) -> Option<String> {
    let path = || string_arg(args, "path").map(str::to_owned);
    let target = match name {
        "move_path" => string_arg(args, "source").map(str::to_owned),
        "apply_edits"
        | "replace_text"
        | "write_file"
        | "create_file"
        | "read_file"
        | "read_media"
        | "path_info"
        | "file_outline"
        | "find_symbol"
        | "list_files"
        | "search_code"
        | "search_many"
        | "scan_patterns"
        | "search_syntax"
        | "semantic_navigation"
        | "semantic_provider_refresh"
        | "delete_path"
        | "create_directory" => path(),
        _ => None,
    }?;
    // The target is displayed in an HTTP projection, so it must be exactly the
    // kind of path the tools themselves accept. Absolute paths, traversal and
    // protected locations are dropped rather than rendered. A directory or a
    // filter such as "." is allowed; it simply never matches a changed file.
    if Workspace::validate_relative(&target).is_err() {
        return None;
    }
    // Defence in depth for the display bound: `validate_relative` already
    // rejects control characters, but the projection must stay bounded even if
    // that validator ever widens.
    if target.trim().is_empty() || target.len() > 256 {
        return None;
    }
    Some(target)
}

pub(super) fn queue_leaf_task(
    monitor: &TaskMonitor,
    workspace: String,
    name: &str,
    args: &Value,
    request_bytes: u64,
) -> TaskTicket {
    monitor.queue_with_target(
        workspace,
        name,
        task_detail(name, args),
        task_target(name, args),
        request_bytes,
    )
}
