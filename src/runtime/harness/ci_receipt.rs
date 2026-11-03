//! Native CI receipt import; observation only, never verification or Acceptance.
use super::*;

impl ToolHarness {
    /// Native CI receipt import. Not an MCP tool: callers must supply a
    /// captured receipt and the operator-declared repository binding.
    pub async fn verification_import_ci_receipt(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        receipt: &crate::git_provider::CheckReceipt,
        binding: &crate::verification::receipt::ReceiptBinding,
    ) -> Result<crate::evidence::Evidence> {
        let git_binding = self
            .execution_git_binding(workspace)
            .await?
            .ok_or_else(|| anyhow::anyhow!("CI receipt import requires a Git execution binding"))?;
        self.intelligence.verification_import_ci_receipt(
            workspace_id,
            workspace,
            plan_id,
            receipt,
            binding,
            Some(git_binding),
        )
    }
}
