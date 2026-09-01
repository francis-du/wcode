#[path = "integrations/agent_install/mod.rs"]
mod agent_install;
#[path = "integrations/agent_plugin/mod.rs"]
mod agent_plugin;
#[path = "integrations/auth/mod.rs"]
mod auth;
#[path = "integrations/auth/origin.rs"]
mod auth_origin;
#[path = "workspace/authorization.rs"]
mod authorization;
#[path = "graph/code_index/mod.rs"]
mod code_index;
#[path = "workspace/conventions.rs"]
mod conventions;
#[path = "design/mod.rs"]
pub mod design;
#[path = "evidence/mod.rs"]
pub mod evidence;
#[path = "evidence/store.rs"]
mod evidence_store;
#[path = "graph/mod.rs"]
pub mod graph;
#[path = "graph/provider_store.rs"]
mod graph_provider_store;
#[path = "graph/graph_store.rs"]
mod graph_store;
#[path = "runtime/harness/mod.rs"]
mod harness;
#[path = "intelligence/mod.rs"]
pub mod intelligence;
#[path = "intelligence/types.rs"]
mod intelligence_types;
#[path = "ui/intelligence_web.rs"]
mod intelligence_web;
#[path = "integrations/mcp/mod.rs"]
mod mcp;
#[path = "integrations/mcp/catalog.rs"]
mod mcp_catalog;
#[path = "integrations/mcp/sse.rs"]
mod mcp_legacy_sse;
#[path = "integrations/mcp/stdio.rs"]
mod mcp_stdio;
#[path = "integrations/mcp/tasks.rs"]
mod mcp_tasks;
#[path = "ui/monitor/mod.rs"]
mod monitor;
#[path = "runtime/power.rs"]
mod power;
#[path = "verification/quality_catalog.rs"]
mod quality_catalog;
#[path = "verification/quality_catalog/extended.rs"]
mod quality_catalog_extended;
#[path = "verification/quality_provider.rs"]
mod quality_provider;
#[path = "reconciliation/mod.rs"]
pub mod reconcile;
#[path = "reconciliation/execution_store.rs"]
mod reconciliation_execution_store;
#[path = "reconciliation/store.rs"]
mod reconciliation_store;
#[path = "runtime/resource.rs"]
mod resource;
#[path = "intelligence/risk.rs"]
pub mod risk;
#[path = "runtime/control.rs"]
mod runtime_control;
#[path = "workspace/scheduler.rs"]
mod scheduler;
#[path = "scopes/mod.rs"]
mod scopes;
#[path = "semantics/mod.rs"]
pub mod semantic;
#[path = "semantics/provider/mod.rs"]
mod semantic_provider;
#[path = "runtime/semantic.rs"]
mod semantic_runtime;
#[path = "semantics/store.rs"]
mod semantic_store;
#[path = "verification/stage_executor.rs"]
mod stage_executor;
#[path = "integrations/mcp/store.rs"]
mod task_store;
#[path = "runtime/tunnel/mod.rs"]
mod tunnel;
#[path = "verification/mod.rs"]
pub mod verification;
#[path = "verification/store.rs"]
mod verification_store;
#[path = "workspace/mod.rs"]
mod workspace;

pub(crate) const CHATGPT_CONNECTOR_SETUP_URL: &str =
    "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins";
pub(crate) const GROK_CONNECTOR_SETUP_URL: &str = "https://grok.com/connectors";
pub(crate) const CLAUDE_CONNECTOR_SETUP_URL: &str = "https://claude.ai/customize/connectors";
pub(crate) const MISTRAL_CONNECTOR_SETUP_URL: &str = "https://chat.mistral.ai/";
pub(crate) const PROJECT_URL: &str = "https://github.com/francis-du/wcode";
pub(crate) const DOCS_URL: &str = "https://francis-du.github.io/wcode/";
pub(crate) const AUTHOR_URL: &str = "https://github.com/francis-du";
pub(crate) const AUTHOR_HANDLE: &str = "@francis-du";

pub mod app;
pub use app::run;

pub fn run_main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(resource::TOKIO_WORKER_THREADS)
        .max_blocking_threads(resource::TOKIO_MAX_BLOCKING_THREADS)
        .thread_keep_alive(std::time::Duration::from_secs(10))
        .enable_all()
        .build()?
        .block_on(run())
}
