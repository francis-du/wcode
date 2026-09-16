use serde_json::Value;

pub(crate) fn failures(workspaces: &[Value]) -> Vec<String> {
    let mut failures = Vec::new();
    if workspaces.is_empty() {
        failures.push("No Workspace was audited".to_owned());
    }
    for workspace in workspaces {
        let id = workspace["workspace"].as_str().unwrap_or("workspace");
        let design = &workspace["design"];
        if design["initialized"].as_bool() != Some(true) {
            failures.push(format!("{id}: Design State is uninitialized"));
        } else if design["valid"].as_bool() != Some(true) {
            failures.push(format!("{id}: Design State is invalid"));
        }

        let traceability = &workspace["traceability"];
        for (key, label) in [
            ("requirement_to_component", "requirement→component"),
            ("design_to_implementation", "design→implementation"),
            (
                "acceptance_to_verification",
                "acceptance→verification mapping",
            ),
        ] {
            if traceability[key]["percent"].as_u64() != Some(100) {
                failures.push(format!("{id}: {label} traceability is incomplete"));
            }
        }

        if workspace["product_scope_required"].as_bool().is_none() {
            failures.push(format!("{id}: Product Scope policy is unavailable"));
        }
        if workspace["product_scope_required"].as_bool() == Some(true) {
            let scope_status = &workspace["scope_status"];
            if scope_status["truncated"].as_bool() != Some(false) {
                failures.push(format!(
                    "{id}: Product Scope audit was truncated or unavailable"
                ));
            }
            let source_files = scope_status["source_files"].as_u64();
            let mapped_files = scope_status["mapped_files"].as_u64();
            let unmapped_files = scope_status["unmapped_files"].as_array().map(Vec::len);
            if source_files.is_none()
                || mapped_files.is_none()
                || source_files != mapped_files
                || unmapped_files != Some(0)
            {
                failures.push(format!("{id}: Product Scope source mapping is incomplete"));
            }
        }

        let conventions = &workspace["conventions"];
        if conventions["truncated"].as_bool() != Some(false) {
            failures.push(format!(
                "{id}: Convention audit was truncated or unavailable"
            ));
        }
        if conventions["errors"].as_u64().is_none() {
            failures.push(format!("{id}: Convention audit error count is unavailable"));
        }
        if conventions["errors"]
            .as_u64()
            .is_some_and(|errors| errors > 0)
        {
            failures.push(format!(
                "{id}: Convention audit contains required-policy errors"
            ));
        }
    }
    failures
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/release_gate.rs"]
mod tests;
