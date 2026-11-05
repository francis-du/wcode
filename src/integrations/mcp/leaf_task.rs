//! How one leaf tool call is identified and recorded for operator observation.
//!
//! `task_detail` is a free-form summary of the tool's arguments. The local TUI
//! renders it; the protected HTTP projection deliberately does not, so an
//! argument-bearing summary cannot leave the machine. `task_target` is the one
//! structured identity that projection may carry: a single repository path,
//! bounded and single-line, used only to link a task to the code it touched.
//! Neither value is an authorization input, and neither widens what a tool may
//! access.

use serde_json::Value;

use super::mcp_tools::{string_arg, task_detail};
use crate::monitor::{TaskMonitor, TaskTicket};

pub(super) fn task_target(name: &str, args: &Value) -> Option<String> {
    let path = || string_arg(args, "path").map(str::to_owned);
    match name {
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
    }
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
