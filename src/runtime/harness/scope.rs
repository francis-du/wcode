use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct ProductScopeStatus {
    pub provider: &'static str,
    pub scopes: Vec<ProductScopeDescriptor>,
    pub source_files: usize,
    pub mapped_files: usize,
    pub unmapped_files: Vec<String>,
    pub counts: BTreeMap<String, usize>,
    pub truncated: bool,
}

impl ToolHarness {
    pub fn product_scope_status(&self, workspace: &Workspace) -> Result<ProductScopeStatus> {
        const MAX_SCOPE_FILES: usize = 10_000;
        const MAX_UNMAPPED_FILES: usize = 128;
        let (files, scan_truncated) = workspace.source_files("src", MAX_SCOPE_FILES)?;
        let mut counts = BTreeMap::<String, usize>::new();
        let mut unmapped_files = Vec::new();
        let mut source_files = 0usize;
        let mut mapped_files = 0usize;

        for path in files {
            if semantic_provider::language_for_path(&path).is_none() {
                continue;
            }
            source_files = source_files.saturating_add(1);
            if let Some(scope) = scopes::source_scope(&path) {
                mapped_files = mapped_files.saturating_add(1);
                *counts.entry(scope.as_str().to_owned()).or_default() += 1;
            } else if unmapped_files.len() < MAX_UNMAPPED_FILES {
                unmapped_files.push(path);
            }
        }

        let truncated =
            scan_truncated || source_files.saturating_sub(mapped_files) > unmapped_files.len();
        Ok(ProductScopeStatus {
            provider: "wcode-product-scopes",
            scopes: scopes::registry(),
            source_files,
            mapped_files,
            unmapped_files,
            counts,
            truncated,
        })
    }
}
