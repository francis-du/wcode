use super::*;

impl ToolHarness {
    pub fn capabilities(&self) -> Value {
        let repository_scanning = json!({
            "gitignore": true,
            "dot_ignore": true,
            "git_global": true,
            "git_exclude": true,
            "generated_directories_pruned": true,
            "explicit_ignored_paths_queryable": true,
        });
        let observatory = json!({
            "cached_snapshots": true,
            "background_single_flight_refresh": true,
            "revision_stamped_cache": true,
            "code_graph_lazy_loaded": true,
        });
        let limits = crate::resource::limits();
        let execution_admission = json!({
            "limit": Self::execution_limit(self.max_parallel),
            "process_capacity": limits.child_processes,
            "subspace_process_capacity": limits.child_processes,
            "host_process_capacity": limits.host_child_process_limit(),
            "queued_process_headroom": Self::execution_limit(self.max_parallel)
                .saturating_sub(limits.host_child_process_limit()),
            "read_headroom": self.max_parallel - Self::execution_limit(self.max_parallel),
            "total_limit": self.max_parallel,
            "tool_slot_wait_cap_ms": u64::try_from(TOOL_SLOT_WAIT_CAP.as_millis()).unwrap_or(u64::MAX),
        });
        let digital_twin = json!({
            "code_graph": true,
            "modes": ["calls", "impact", "all"],
            "max_depth": 4,
            "max_nodes": 240,
            "precision": [
                "declared",
                "syntax",
                "semantic",
                "runtime",
                "deterministic",
                "heuristic",
                "mixed"
            ],
            "design_ownership": true,
            "test_and_proof_edges": true,
            "runtime_provider_edges": true,
            "history_navigation": true,
        });
        let decision_plane = json!({
            "schema_version": crate::decision::DECISION_SCHEMA_VERSION,
            "provider": "wcode-deterministic-v1",
            "local_decision_plane": true,
            "jev_optional": "jev",
            "question_set": "wcode.agent_context@3",
            "typed_distributions": true,
            "concentration_metrics": ["top1_margin", "normalized_entropy"],
            "authority": "advisory_only",
            "shadow_ab": true,
            "independent_fitness_calibration": true,
            "primitives": ["probability", "choice", "score"],
        });
        let semantic_providers = json!({
            "languages": 22,
            "adapter": "warm-lsp-session-document-symbol-navigation",
            "precision": "semantic-when-lsp-is-live-syntax-otherwise",
            "mode": "automatic-hardened-lsp-with-explicit-trust-for-others",
            "default_enabled": true,
            "opt_out": "--no-semantic",
            "requires_risky_exec": "non-automatic-providers-only",
            "warm_sessions": true,
            "incremental_document_sync": true,
            "navigation": [
                "definition",
                "references",
                "implementations",
                "incoming_calls",
                "outgoing_calls",
                "hover"
            ],
            "routing": "tree-sitter-for-localization-lsp-for-cross-file-relations",
            "session_pool": self.semantic_sessions.status()
        });
        let stage_executors = json!({
            "builtin_discovery": true,
            "config": ".wcode/executors.yaml",
            "no_shell": true,
            "languages": 22,
            "stages": ["property", "mutation", "fuzz", "runtime_canary"],
            "execution_policy": "bounded-no-shell-repository-executors-autonomous",
            "requires_risky_exec": false
        });
        let software_intelligence = json!({
            "design_state": true,
            "software_graph": "composite-declared-syntax-external",
            "graph_history": graph_store::capabilities(),
            "graph_providers": graph_provider_store::capabilities(),
            "engineering_digital_twin": digital_twin,
            "decision_plane": decision_plane,
            "semantic_providers": semantic_providers,
            "traceability": true,
            "software_context": true,
            "drift": true,
            "impact_analysis": true,
            "risk": true,
            "reconciliation_plan": true,
            "verification_mesh": verification_store::capabilities(),
            "migration_audit": crate::migration_audit::capabilities(),
            "stage_executors": stage_executors,
            "evidence": evidence_store::capabilities(),
            "experience": crate::experience_store::capabilities(),
            "semantics": semantic_store::capabilities(),
            "reconciliation": reconciliation_store::capabilities(),
            "reconciliation_execution": reconciliation_execution_store::capabilities(),
            "persistent_store": [
                "verification-state",
                "evidence",
                "experience",
                "semantics",
                "graph-providers",
                "graph-history",
                "reconciliation-plans",
                "reconciliation-execution"
            ],
            "automatic_reconciliation": "orchestrated-safe-task-execution"
        });
        json!({
            "tools": QUALITY_HARNESS_TOOLS,
            "project_context": true,
            "context_cache": true,
            "review_changes": true,
            "parallel_change_review": true,
            "adversarial_review": true,
            "adversarial_review_policy": "challenge-packet-not-evidence",
            "verify_project": true,
            "phased_parallel_verification": true,
            "verification_exec_without_risky_flag": true,
            "verification_levels": ["quick", "full"],
            "max_verification_checks": MAX_VERIFICATION_CHECKS,
            "max_review_files": MAX_REVIEW_FILES,
            "max_parallel_tools": self.max_parallel,
            "repository_scanning": repository_scanning,
            "observatory": observatory,
            "execution_admission": execution_admission,
            "resource_governor": crate::resource::capabilities(),
            "software_intelligence": software_intelligence,
            "code_index": self.code_index.capabilities(),
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/capabilities.rs"]
mod tests;
