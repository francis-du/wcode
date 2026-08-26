use crate::scopes;
use serde_json::{json, Value};

const IMPLEMENT_PROMPT: &str = "Use wcode as the software-intelligence control layer. Start with workspace_info, design_status, and project_context, inspect the Product Scope registry, then call software_context for the requested behavior or subsystem. Pass scopes when the task is bounded to design, graph, semantics, traceability, risk, verification, evidence, reconciliation, workspace, integrations, runtime, or experience. Navigate with symbol tools before broad reads. Keep edits inside the workspace and preserve SHA preconditions. After implementation run review_changes, drift_status, impact_analysis, and risk_status; reconcile gaps, run the recommended verification, and finish with evidence_status. Never promote syntax facts to semantic precision or fabricate verification evidence.";
const REVIEW_PROMPT: &str = "Review the current change using wcode evidence rather than chat confidence. Inspect design_status and traceability_status, then review_changes, drift_status, impact_analysis, and risk_status. Check semantic/runtime graph provenance where available and treat stale or syntax-only facts conservatively. Run the recommended verify_project level. If a Verification Plan exists, preserve blind-review independence and surface disagreements instead of majority-voting them away.";
const VERIFY_PROMPT: &str = "Verify the selected workspace with wcode. Inspect verification_status and verification_executor_status. Run verify_project at the plan/recommended level and, when explicitly trusted, verification_execute_stages for required Property, Mutation, Fuzz, or Runtime-Canary stages. Readiness is fail-closed per producer: one runner's Pass cannot mask another runner's Fail. Never invent stage results or HumanApproval.";
const SECURITY_RESOURCE: &str = "wcode is workspace-scoped. Absolute paths, parent traversal, protected credential/VCS paths, symlink aliases, unsafe hard-link writes, stale SHA writes, unrestricted shell execution, and model-facing delete are denied. Repository-aware language servers and custom stage executors require explicit --allow-risky-exec. Agent plugins should prefer stdio MCP for local agents and must not bundle credentials, executable hooks, or hidden shell scripts by default.";

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
                "description":"Provider-neutral Software Intelligence workflow for coding agents.",
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
            }
        ]
    })
}

pub(crate) fn resource_read(uri: &str) -> Result<Value, String> {
    let text = match uri {
        "wcode://runtime/workflow" => format!(
            "# wcode agent workflow\n\n## Implement\n{IMPLEMENT_PROMPT}\n\n## Review\n{REVIEW_PROMPT}\n\n## Verify\n{VERIFY_PROMPT}\n"
        ),
        "wcode://runtime/security" => format!("# wcode security boundary\n\n{SECURITY_RESOURCE}\n"),
        "wcode://runtime/product-scopes" => product_scopes_markdown(),
        _ => return Err(format!("unknown resource URI: {uri}")),
    };
    Ok(json!({
        "contents": [{"uri":uri,"mimeType":"text/markdown","text":text}]
    }))
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
mod tests {
    use super::*;

    #[test]
    fn catalog_is_bounded_and_has_no_secret_or_hook_payloads() {
        let prompt =
            prompt_get("wcode-implement", Some(&json!({"goal":"x".repeat(2_000)}))).unwrap();
        let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.len() < 5_000);
        assert!(!SECURITY_RESOURCE.contains("API_KEY="));
        assert_eq!(resources_list()["resources"].as_array().unwrap().len(), 3);
        let scopes = resource_read("wcode://runtime/product-scopes").unwrap();
        let scopes_text = scopes["contents"][0]["text"].as_str().unwrap();
        assert!(scopes_text.contains("Software Graph (`graph`)"));
        assert!(scopes_text.contains("software_context.scopes"));
        assert_eq!(prompts_list()["prompts"].as_array().unwrap().len(), 3);
    }
}
