use super::*;

pub(super) async fn run_intelligence_cli(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    monitor: &TaskMonitor,
    refresh_semantic: bool,
    automatic_semantic: bool,
    enforce_check: bool,
    emit_json: bool,
) -> Result<()> {
    let automatic_workspaces = workspaces
        .semantic_workspaces()
        .into_iter()
        .map(|(workspace_id, _)| workspace_id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut entries = Vec::new();
    for (workspace_id, root) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&workspace_id))?;
        let semantic_refresh = if refresh_semantic {
            Some(
                harness
                    .semantic_provider_refresh(&workspace, ".", 128, 1_000)
                    .await?,
            )
        } else if automatic_semantic && automatic_workspaces.contains(&workspace_id) {
            Some(
                harness
                    .semantic_provider_refresh_automatic(&workspace, ".", 128, 1_000)
                    .await?,
            )
        } else {
            None
        };
        let design = harness.design_status(workspace_id.clone(), &workspace)?;
        let design_load = design::load_design(&workspace)?;
        let product_scope_required = design_load
            .state
            .constraints
            .contains_key("CONSTRAINT-PRODUCT-SCOPE-CANONICAL");
        let traceability = harness.traceability_status(workspace_id.clone(), &workspace)?;
        let scope_status = harness.product_scope_status(&workspace)?;
        let conventions = harness.convention_status(&workspace)?;
        let semantics = harness.semantic_status(&workspace_id, &workspace, 100)?;
        let graph_history = harness.graph_history(&workspace, 10)?;
        let graph_diff = if graph_history.len() >= 2 {
            harness
                .graph_diff(
                    &workspace,
                    &crate::graph_store::GraphDiffInput {
                        from_snapshot_id: None,
                        to_snapshot_id: None,
                        limit: 20,
                    },
                )
                .ok()
        } else {
            None
        };
        let providers = harness.graph_provider_status(&workspace)?;
        let semantic_providers = harness.semantic_provider_status(&workspace)?;
        let verification_executors = harness.verification_executor_status(&workspace)?;
        let evidence = harness.evidence_status(&workspace_id, &workspace, None, 100)?;
        let reconciliation = harness.reconciliation_history(&workspace, 20)?;
        let verification = harness.verification_history(&workspace_id, &workspace, 20)?;
        let risk = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
            match harness
                .review_changes(workspace_id.clone(), &workspace, 30, monitor)
                .await
            {
                Ok(review) => harness
                    .risk_status(workspace_id.clone(), &workspace, &review)
                    .ok()
                    .and_then(|status| serde_json::to_value(status).ok()),
                Err(_) => None,
            }
        } else {
            None
        };
        entries.push(json!({
            "workspace": workspace_id,
            "root": root,
            "design": design,
            "traceability": traceability,
            "product_scope_required": product_scope_required,
            "scope_status": scope_status,
            "conventions": conventions,
            "semantics": semantics,
            "graph_history": graph_history,
            "graph_diff": graph_diff,
            "graph_providers": providers,
            "semantic_providers": semantic_providers,
            "semantic_refresh": semantic_refresh,
            "verification_executors": verification_executors,
            "risk": risk,
            "evidence": evidence,
            "reconciliation": reconciliation,
            "verification": verification,
        }));
    }
    let check_failures = intelligence_check_failures(&entries);
    let check_passed = check_failures.is_empty();
    let check_error = (!check_passed).then(|| check_failures.join("; "));
    let value = json!({
        "runtime": "wcode-software-intelligence",
        "version": env!("CARGO_PKG_VERSION"),
        "check": {
            "passed": check_passed,
            "failures": check_failures,
        },
        "workspaces": entries,
    });
    if emit_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        if enforce_check {
            if let Some(error) = check_error {
                bail!("Software Intelligence check failed: {error}");
            }
        }
        return Ok(());
    }
    println!(
        "wcode Software Intelligence Runtime {}",
        env!("CARGO_PKG_VERSION")
    );
    for workspace in value["workspaces"].as_array().into_iter().flatten() {
        let id = workspace["workspace"].as_str().unwrap_or("workspace");
        let root = workspace["root"].as_str().unwrap_or(".");
        let design = &workspace["design"];
        let trace = &workspace["traceability"];
        let scope_status = &workspace["scope_status"];
        let conventions = &workspace["conventions"];
        let semantics = &workspace["semantics"];
        let evidence = &workspace["evidence"];
        let verification = workspace["verification"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let reconciliation = workspace["reconciliation"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        let graph_history = workspace["graph_history"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        println!("\n{id}  {root}");
        println!(
            "  Design        {} · {} req · {} components",
            if design["valid"].as_bool().unwrap_or(false) {
                "valid"
            } else if design["initialized"].as_bool().unwrap_or(false) {
                "invalid"
            } else {
                "uninitialized"
            },
            design["requirements"].as_u64().unwrap_or(0),
            design["components"].as_u64().unwrap_or(0)
        );
        println!(
            "  Traceability  implementation {}% · verification {}%",
            trace["design_to_implementation"]["percent"]
                .as_u64()
                .unwrap_or(0),
            trace["acceptance_to_verification"]["percent"]
                .as_u64()
                .unwrap_or(0)
        );
        println!(
            "  Product Scope {}/{} mapped · {} unmapped",
            scope_status["mapped_files"].as_u64().unwrap_or(0),
            scope_status["source_files"].as_u64().unwrap_or(0),
            scope_status["unmapped_files"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0)
        );
        println!(
            "  Conventions   {} errors · {} warnings",
            conventions["errors"].as_u64().unwrap_or(0),
            conventions["warnings"].as_u64().unwrap_or(0)
        );
        println!(
            "  Semantics     {} confirmed · {} candidates",
            semantics["confirmed"].as_u64().unwrap_or(0),
            semantics["candidates"].as_u64().unwrap_or(0)
        );
        let provider_count = workspace["semantic_providers"]
            .as_array()
            .map(|providers| {
                providers
                    .iter()
                    .filter(|provider| provider["runnable"].as_bool() == Some(true))
                    .count()
            })
            .unwrap_or(0);
        let executor_count = workspace["verification_executors"]["executors"]
            .as_array()
            .map(|executors| {
                executors
                    .iter()
                    .filter(|executor| executor["available"].as_bool() == Some(true))
                    .count()
            })
            .unwrap_or(0);
        println!(
            "  Providers     {provider_count} semantic LSP · {executor_count} verification executors"
        );
        if let Some(diff) = workspace["graph_diff"].as_object() {
            println!(
                "  Graph Δ       nodes +{}/-{}/~{} · edges +{}/-{}/~{}",
                diff.get("added_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("removed_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("changed_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("added_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("removed_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("changed_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        }
        if let Some(risk) = workspace["risk"].as_object() {
            println!(
                "  Risk          {} · {} findings",
                risk.get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                risk.get("risks")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            );
        } else {
            println!("  Risk          unavailable (Git/exec review not available)");
        }
        println!(
            "  Evidence      {} total · {} failed · {} disagreed",
            evidence["total"].as_u64().unwrap_or(0),
            evidence["failed"].as_u64().unwrap_or(0),
            evidence["disagreed"].as_u64().unwrap_or(0)
        );
        let ready = verification
            .iter()
            .filter(|status| status["ready"].as_bool() == Some(true))
            .count();
        println!(
            "  Runtime       {graph_history} graph revisions · {reconciliation} reconciliation plans · {}/{} verification ready",
            ready,
            verification.len()
        );
    }
    println!(
        "\n  Check         {}",
        if check_passed { "PASS" } else { "FAIL" }
    );
    if enforce_check {
        if let Some(error) = check_error {
            bail!("Software Intelligence check failed: {error}");
        }
    }
    Ok(())
}

fn intelligence_check_failures(workspaces: &[Value]) -> Vec<String> {
    let mut failures = Vec::new();
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
            ("acceptance_to_verification", "acceptance→verification"),
        ] {
            if traceability[key]["percent"].as_u64() != Some(100) {
                failures.push(format!("{id}: {label} traceability is incomplete"));
            }
        }

        if workspace["product_scope_required"].as_bool() == Some(true) {
            let scope_status = &workspace["scope_status"];
            if scope_status["truncated"].as_bool() == Some(true) {
                failures.push(format!("{id}: Product Scope audit was truncated"));
            }
            let source_files = scope_status["source_files"].as_u64();
            let mapped_files = scope_status["mapped_files"].as_u64();
            let unmapped_files = scope_status["unmapped_files"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            if source_files.is_none()
                || mapped_files.is_none()
                || source_files != mapped_files
                || unmapped_files > 0
            {
                failures.push(format!("{id}: Product Scope source mapping is incomplete"));
            }
        }

        let conventions = &workspace["conventions"];
        if conventions["truncated"].as_bool() == Some(true) {
            failures.push(format!("{id}: Convention audit was truncated"));
        }
        if conventions["errors"].as_u64().unwrap_or(0) > 0 {
            failures.push(format!(
                "{id}: Convention audit contains required-policy errors"
            ));
        }
    }
    failures
}

pub(super) async fn run_verification_cli(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    requested_plan: Option<&str>,
    execute_stages: bool,
    emit_json: bool,
) -> Result<()> {
    if execute_stages && requested_plan.is_none() {
        bail!("--execute-stages requires --plan-id so wcode never guesses which plan to run");
    }
    let mut entries = Vec::new();
    for (workspace_id, root) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&workspace_id))?;
        let mut execution = None;
        let statuses = if let Some(plan_id) = requested_plan {
            match harness.verification_status(&workspace_id, &workspace, plan_id) {
                Ok(_) => {
                    if execute_stages {
                        execution = Some(
                            harness
                                .verification_execute_stages(&workspace_id, &workspace, plan_id)
                                .await?,
                        );
                    }
                    vec![harness.verification_status(&workspace_id, &workspace, plan_id)?]
                }
                Err(_) => Vec::new(),
            }
        } else {
            harness.verification_history(&workspace_id, &workspace, 50)?
        };
        if requested_plan.is_none() || !statuses.is_empty() {
            entries.push(json!({
                "workspace": workspace_id,
                "root": root,
                "execution": execution,
                "plans": statuses,
            }));
        }
    }
    if requested_plan.is_some() && entries.is_empty() {
        bail!("verification plan was not found in any configured workspace");
    }
    let value = json!({
        "runtime": "wcode-verification",
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": entries,
    });
    if emit_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("wcode Verification Mesh {}", env!("CARGO_PKG_VERSION"));
    for workspace in value["workspaces"].as_array().into_iter().flatten() {
        println!(
            "\n{}  {}",
            workspace["workspace"].as_str().unwrap_or("workspace"),
            workspace["root"].as_str().unwrap_or(".")
        );
        let plans = workspace["plans"].as_array().cloned().unwrap_or_default();
        if plans.is_empty() {
            println!("  no persisted verification plans");
            continue;
        }
        for status in plans {
            let plan = &status["plan"];
            let blockers = status["blockers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            println!(
                "  {}  risk={}  ready={}  reviewers={}/{}  blockers={}",
                plan["id"].as_str().unwrap_or("plan"),
                plan["risk_level"].as_str().unwrap_or("unknown"),
                status["ready"].as_bool().unwrap_or(false),
                status["submitted"].as_u64().unwrap_or(0),
                plan["job_ids"].as_array().map(Vec::len).unwrap_or(0),
                if blockers.is_empty() {
                    "none".to_owned()
                } else {
                    blockers.join(",")
                }
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/app/intelligence.rs"]
mod tests;
