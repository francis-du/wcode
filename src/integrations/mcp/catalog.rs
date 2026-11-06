use crate::scopes;
use serde_json::{json, Value};

const IMPLEMENT_PROMPT: &str = "Use wcode as the repository control layer. Call agent_context first with the coding goal and known Product Scope(s). Treat capabilities.recommended_actions, or recommended_tools when the pack is compacted, as the active model tool set instead of choosing from the complete tools/list catalog. Load another tool only when explicit user intent or readiness/recovery requires it. Keep MCP calls compact: pass only required arguments and omit default Workspace/path/limit/timeout/budget values. Treat core_constraints as mandatory, then follow readiness/next_actions without rereading hot_source. Use symbol_context only for missing bodies, apply_edits for one direct target file or apply_file_edits for multiple, then review_changes and the recommended verify_project level. When Agent Context marks parallelism required, concurrent execution is mandatory. Prefer one-traversal bulk primitives such as read_files, search_many and apply_file_edits; otherwise launch independent top-level calls concurrently or use parallel_tools for compact fanout. Do not repeat project_context or source already supplied by agent_context unless a specific gap remains. Reuse existing components before adding abstractions. Use deeper design, drift, risk, reconciliation, or language-quality tools only when required. Never promote syntax facts to semantic precision or fabricate verification evidence.";
const REVIEW_PROMPT: &str = "Review the current change using wcode evidence rather than chat confidence. Inspect design_status and traceability_status, then call review_changes with adversarial=true before drift_status, impact_analysis, and risk_status. Treat the adversarial QA packet as questions to falsify, never as Evidence. Check semantic/runtime graph provenance where available and treat stale or syntax-only facts conservatively. Run the recommended verify_project level. If a Verification Plan exists, preserve blind-review independence and surface disagreements instead of majority-voting them away.";
const VERIFY_PROMPT: &str = "Verify the selected workspace with wcode. Inspect verification_status and verification_executor_status. Run verify_project at the plan/recommended level and, when explicitly trusted, verification_execute_stages for required Property, Mutation, Fuzz, or Runtime-Canary stages. Readiness is fail-closed per producer: one runner's Pass cannot mask another runner's Fail. Never invent stage results or HumanApproval.";
const SECURITY_RESOURCE: &str = "wcode is workspace-scoped. Absolute paths, parent traversal, protected credential/VCS paths, symlink aliases, unsafe hard-link writes, stale SHA writes, unrestricted shell execution, and unapproved deletion are denied. delete_path permits only a regular file or empty directory after exact one-shot human approval. Hardened first-party LSP servers may run automatically through the bounded LSP lane; repository-local LSP executables are rejected and --no-semantic disables the lane. Other repository-aware LSP servers and custom stage executors still require explicit RiskyExecution trust. Agent plugins should prefer stdio MCP for local agents and must not bundle credentials, executable hooks, or hidden shell scripts by default.";

pub(crate) fn prompts_list() -> Value {
    json!({
        "prompts": [
            {
                "name": "wcode-implement",
                "description": "Implement a change using Design State, graph/context, risk, verification, and evidence.",
                "arguments": [{"name":"goal","description":"The behavior, requirement, or subsystem to implement.","required":false}]
            },
            {
                "name": "wcode-review",
                "description": "Review the current working tree against design, impact, risk, and verification evidence.",
                "arguments": []
            },
            {
                "name": "wcode-verify",
                "description": "Run evidence-driven verification without fabricating stage or approval results.",
                "arguments": [{"name":"plan_id","description":"Optional Verification Plan identifier.","required":false}]
            }
        ]
    })
}

pub(crate) fn prompt_get(name: &str, arguments: Option<&Value>) -> Result<Value, String> {
    let text = match name {
        "wcode-implement" => append_argument(IMPLEMENT_PROMPT, arguments, "goal", "Goal"),
        "wcode-review" => REVIEW_PROMPT.to_owned(),
        "wcode-verify" => append_argument(VERIFY_PROMPT, arguments, "plan_id", "Plan"),
        _ => return Err(format!("unknown prompt: {name}")),
    };
    Ok(json!({
        "description": format!("wcode workflow prompt: {name}"),
        "messages": [{"role":"user","content":{"type":"text","text":text}}]
    }))
}

pub(crate) fn resources_list() -> Value {
    json!({
        "resources": [
            {
                "uri":"wcode://runtime/workflow",
                "name":"wcode workflow",
                "description":"Provider-neutral engineering control-plane workflow for coding agents.",
                "mimeType":"text/markdown"
            },
            {
                "uri":"wcode://runtime/security",
                "name":"wcode security boundary",
                "description":"Security invariants that plugins and MCP clients must preserve.",
                "mimeType":"text/markdown"
            },
            {
                "uri":"wcode://runtime/product-scopes",
                "name":"wcode product scopes",
                "description":"Canonical wcode capability scopes used by source architecture, semantic filtering, software_context, project_context, and MCP tool metadata.",
                "mimeType":"text/markdown"
            },
            {
                "uri":"ui://wcode/observatory",
                "name":"wcode Observatory",
                "description":"Live project status for the selected workspace: changes, verification evidence, risk and convergence. Clients that render UI resources open this beside the conversation.",
                "mimeType":"text/html"
            }
        ]
    })
}

/// Legacy protocols have no cache metadata; keep the two response shapes in one
/// place so resource arms do not grow a formatted conditional each.
pub(crate) fn cacheable(value: Value, modern: bool) -> Value {
    if modern {
        crate::mcp::modern_cacheable_result(value)
    } else {
        value
    }
}

/// The authenticated Observatory address for the running runtime.
pub(crate) fn observatory_url(state: &crate::mcp::AppState) -> String {
    state.auth.observatory_url()
}

pub(crate) fn resource_read(uri: &str, observatory_url: Option<&str>) -> Result<Value, String> {
    let (text, mime) = match uri {
        "wcode://runtime/workflow" => (
            format!(
                "# wcode agent workflow\n\n## Implement\n{IMPLEMENT_PROMPT}\n\n## Review\n{REVIEW_PROMPT}\n\n## Verify\n{VERIFY_PROMPT}\n"
            ),
            "text/markdown",
        ),
        "wcode://runtime/security" => (format!("# wcode security boundary\n\n{SECURITY_RESOURCE}\n"), "text/markdown"),
        "wcode://runtime/product-scopes" => (product_scopes_markdown(), "text/markdown"),
        "ui://wcode/observatory" => (observatory_resource(observatory_url), "text/html"),
        _ => return Err(format!("unknown resource URI: {uri}")),
    };
    Ok(json!({
        "contents": [{"uri":uri,"mimeType":mime,"text":text}]
    }))
}

/// A UI resource must render without a build step, so it embeds the
/// authenticated Observatory address in a bounded page that frames it. Clients
/// without UI support still receive the address as text.
fn observatory_resource(url: Option<&str>) -> String {
    let Some(url) = url else {
        return "<!doctype html><meta charset=\"utf-8\"><title>wcode Observatory</title><p>The Observatory address is unavailable until the runtime endpoint is settled.</p>".to_owned();
    };
    let escaped = url
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>wcode Observatory</title><style>html,body{{margin:0;height:100%;background:#0b0812;color:#e8e6f0;font:14px/1.4 system-ui}}header{{padding:10px 14px;border-bottom:1px solid #2a2438;background:#12101c}}iframe{{border:0;width:100%;height:calc(100% - 42px);display:block}}</style></head><body><header><strong>wcode Observatory</strong> · live project status</header><iframe title=\"wcode Observatory\" src=\"{escaped}\"></iframe></body></html>"
    )
}

fn product_scopes_markdown() -> String {
    let mut text = String::from(
        "# wcode Product Scopes\n\nThese scopes describe wcode's product/control-plane capabilities, not model vendors. Semantic facts may also use freeform business scopes.\n\n",
    );
    for scope in scopes::registry() {
        text.push_str(&format!(
            "## {} (`{}`)\n{}\n\nSource roots: `{}`\n\n",
            scope.title,
            scope.id,
            scope.purpose,
            scope.source_roots.join("`, `")
        ));
    }
    text.push_str("Use `software_context.scopes` to narrow source navigation and semantic expansion. Use `semantic_query.scopes` to filter scoped facts. `tools/list` also exposes `dev.wcode/productScopes` in each Tool `_meta`.\n");
    text
}

fn append_argument(base: &str, arguments: Option<&Value>, key: &str, label: &str) -> String {
    let value = arguments
        .and_then(Value::as_object)
        .and_then(|arguments| arguments.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match value {
        Some(value) => format!("{base}\n\n{label}: {}", bounded_text(value, 1_000)),
        None => base.to_owned(),
    }
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/catalog.rs"]
mod tests;
