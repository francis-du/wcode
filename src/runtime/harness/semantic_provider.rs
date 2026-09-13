use super::*;

impl ToolHarness {
    pub fn semantic_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<SemanticProviderStatus>> {
        semantic_provider::status(workspace, Some(&self.semantic_sessions))
    }

    pub(super) fn semantic_provider_status_for_languages(
        &self,
        workspace: &Workspace,
        languages: &[crate::semantic_provider::SemanticLanguage],
    ) -> Result<Vec<SemanticProviderStatus>> {
        let languages = languages.iter().copied().collect::<BTreeSet<_>>();
        semantic_provider::status_for_languages(
            workspace,
            Some(&self.semantic_sessions),
            &languages,
        )
    }

    pub fn semantic_session_status(&self, workspace: &Workspace) -> SemanticSessionPoolStatus {
        self.semantic_sessions.status_for(workspace)
    }

    pub(crate) fn prune_semantic_sessions(&self) {
        self.semantic_sessions.prune_idle();
    }

    pub async fn semantic_provider_refresh(
        &self,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SemanticProviderRefresh> {
        self.semantic_provider_refresh_mode(workspace, path, max_files, max_symbols, false)
            .await
    }

    pub(crate) async fn semantic_provider_refresh_automatic(
        &self,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SemanticProviderRefresh> {
        self.semantic_provider_refresh_mode(workspace, path, max_files, max_symbols, true)
            .await
    }

    async fn semantic_provider_refresh_mode(
        &self,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
        automatic_only: bool,
    ) -> Result<SemanticProviderRefresh> {
        let existing = graph_provider_store::load_latest(workspace)?
            .into_iter()
            .map(|stored| (stored.import.provider.clone(), stored.import))
            .collect::<BTreeMap<_, _>>();
        let refresh = if automatic_only {
            semantic_provider::refresh_automatic(
                &self.semantic_sessions,
                workspace,
                path,
                max_files,
                max_symbols,
                &existing,
            )
            .await?
        } else {
            semantic_provider::refresh(
                &self.semantic_sessions,
                workspace,
                path,
                max_files,
                max_symbols,
                &existing,
            )
            .await?
        };
        for import in &refresh.imports {
            graph_provider_store::persist(workspace, import)?;
        }
        Ok(refresh)
    }
}
