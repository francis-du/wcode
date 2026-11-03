use super::*;
use crate::verification::receipt::ReceiptBinding;
use anyhow::{anyhow, ensure};
use std::io::Write;

/// Import one captured authoritative CI receipt into the evidence ledger.
///
/// The receipt is read from the operator's bounded local store and must match
/// the operator-declared repository, App and check. The result is
/// observation-only evidence: it never satisfies a verification stage, never
/// becomes deterministic verification and never reaches Acceptance.
pub(super) async fn run_import_ci_receipt(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    command: &ControlCommand,
) -> Result<()> {
    let ControlCommand::ImportCiReceipt {
        plan_id,
        store,
        check_id,
        repository,
        repository_id,
        app_id,
        check_name,
        json,
    } = command
    else {
        bail!("not a CI receipt import");
    };
    ensure!(
        // Discovered packages belong to the explicitly selected repository;
        // they are not independent CLI candidates.
        workspaces.configured_roots().len() == 1,
        "CI receipt import accepts one --workspace; inspect each selected project separately"
    );
    let (workspace_id, workspace) = workspaces.select(None)?;
    ensure!(
        workspace.write_enabled(),
        "CI receipt import is disabled in read-only mode"
    );
    ensure!(
        workspace.exec_enabled(),
        "CI receipt import requires bounded Git metadata commands; execution is disabled"
    );
    let receipts = crate::git_provider::github::load_receipts(store)?;
    let receipt = receipts
        .into_iter()
        .find(|entry| entry.check.id == *check_id)
        .ok_or_else(|| anyhow!("check receipt {check_id} is not present in the selected store"))?;
    let binding = ReceiptBinding {
        repository: repository.clone(),
        repository_id: *repository_id,
        app_id: *app_id,
        check_name: check_name.clone(),
    };
    let evidence = harness
        .verification_import_ci_receipt(&workspace_id, &workspace, plan_id, &receipt, &binding)
        .await?;
    let mut output = io::stdout().lock();
    if *json {
        writeln!(output, "{}", serde_json::to_string_pretty(&evidence)?)?;
        return Ok(());
    }
    let captured = evidence
        .external_receipt
        .as_ref()
        .ok_or_else(|| anyhow!("imported evidence has no captured receipt"))?;
    writeln!(
        output,
        "WCode CI receipt {} imported as observation-only evidence {}",
        captured.check_id, evidence.id
    )?;
    writeln!(
        output,
        "  Workspace: {}  Plan: {}  Subject: {}",
        workspace_id, plan_id, evidence.subject
    )?;
    writeln!(
        output,
        "  Provider: {}  Repository: {}  Check: {}",
        captured.provider, captured.repository, captured.check_name
    )?;
    writeln!(
        output,
        "  Result: {:?}  Confidence: {:?}  Digest: {}",
        evidence.result, evidence.confidence, captured.digest
    )?;
    writeln!(
        output,
        "  Imported receipts never satisfy a stage or an acceptance gate."
    )?;
    output.flush()?;
    Ok(())
}
