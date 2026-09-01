use super::*;

impl ToolHarness {
    pub fn new(max_parallel: usize) -> Result<Self> {
        if !(1..=MAX_PARALLEL_TOOLS).contains(&max_parallel) {
            bail!("max parallel tools must be between 1 and {MAX_PARALLEL_TOOLS}");
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(max_parallel)),
            max_parallel,
            project_cache: Default::default(),
            repo_map_cache: Default::default(),
            code_index: CodeIndex::new()?,
            semantic_sessions: SemanticSessionPool::default(),
            intelligence: SoftwareIntelligenceRuntime::default(),
        })
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub fn intelligence_capability_count(&self) -> usize {
        SOFTWARE_INTELLIGENCE_CAPABILITIES.len()
    }

    pub fn capabilities(&self) -> Value {
        json!({
            "tools": QUALITY_HARNESS_TOOLS,
            "project_context": true,
            "context_cache": true,
            "review_changes": true,
            "parallel_change_review": true,
            "verify_project": true,
            "phased_parallel_verification": true,
            "verification_exec_without_risky_flag": true,
            "verification_levels": ["quick", "full"],
            "max_verification_checks": MAX_VERIFICATION_CHECKS,
            "max_review_files": MAX_REVIEW_FILES,
            "max_parallel_tools": self.max_parallel,
            "resource_governor": crate::resource::capabilities(),
            "software_intelligence": {
                "design_state": true,
                "software_graph": "composite-declared-syntax-external",
                "graph_history": graph_store::capabilities(),
                "graph_providers": graph_provider_store::capabilities(),
                "semantic_providers": {
                    "languages": 22,
                    "adapter": "warm-lsp-session-document-symbol-navigation",
                    "precision": "semantic-when-provider-runs-syntax-fallback-otherwise",
                    "mode": "automatic-hardened-providers-with-manual-trust-fallback",
                    "default_enabled": true,
                    "opt_out": "--no-semantic",
                    "requires_risky_exec": "non-automatic-providers-only",
                    "warm_sessions": true,
                    "incremental_document_sync": true,
                    "navigation": ["definition", "references", "implementations", "incoming_calls", "outgoing_calls", "hover"],
                    "routing": "syntax-for-localization-semantic-for-cross-file-relations",
                    "session_pool": self.semantic_sessions.status()
                },
                "traceability": true,
                "software_context": true,
                "drift": true,
                "impact_analysis": true,
                "risk": true,
                "reconciliation_plan": true,
                "verification_mesh": verification_store::capabilities(),
                "stage_executors": {
                    "builtin_discovery": true,
                    "config": ".wcode/executors.yaml",
                    "no_shell": true,
                    "languages": 22,
                    "stages": ["property", "mutation", "fuzz", "runtime_canary"],
                    "requires_risky_exec": true
                },
                "evidence": evidence_store::capabilities(),
                "semantics": semantic_store::capabilities(),
                "reconciliation": reconciliation_store::capabilities(),
                "reconciliation_execution": reconciliation_execution_store::capabilities(),
                "persistent_store": ["verification-state", "evidence", "semantics", "graph-providers", "graph-history", "reconciliation-plans", "reconciliation-execution"],
                "automatic_reconciliation": "orchestrated-safe-task-execution"
            },
            "code_index": self.code_index.capabilities(),
        })
    }

    pub fn convention_status(&self, workspace: &Workspace) -> Result<ConventionReport> {
        conventions::status(workspace)
    }

    pub fn design_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<DesignStatus> {
        self.intelligence.design_status(workspace_id, workspace)
    }

    pub fn design_init(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        name: &str,
        description: &str,
    ) -> Result<DesignStatus> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 200 {
            bail!("design project name must contain between 1 and 200 characters");
        }
        let existing = design::load_design(workspace)?;
        if existing.initialized {
            bail!("Design State is already initialized for this workspace");
        }
        let reserved_paths = [
            design::PROJECT_FILE,
            ".wcode/design/product.yaml",
            ".wcode/design/requirements.yaml",
            ".wcode/design/components.yaml",
            ".wcode/design/constraints.yaml",
            ".wcode/design/acceptance.yaml",
            ".wcode/design/decisions.yaml",
        ];
        if let Some(path) = reserved_paths
            .iter()
            .find(|path| workspace.root().join(path).exists())
        {
            bail!("cannot initialize Design State because {path} already exists");
        }
        workspace.ensure_directory(".wcode")?;
        workspace.ensure_directory(design::DESIGN_ROOT)?;
        let project = design::ProjectDesign {
            schema_version: 1,
            name: name.to_owned(),
            description: description.trim().to_owned(),
        };
        let product = design::ProductDesign {
            schema_version: 1,
            id: design_product_id(name),
            name: format!("{name} Software Intelligence"),
            vision:
                "Software continuously converges toward intended design with verifiable evidence."
                    .into(),
            principles: vec![
                "Design State is the desired software state.".into(),
                "Models are replaceable executors, not the source of truth.".into(),
                "Deterministic evidence outranks model consensus.".into(),
            ],
        };
        workspace.create_file(
            design::PROJECT_FILE,
            &serde_yaml::to_string(&project).context("cannot encode project Design State")?,
        )?;
        workspace.create_file(
            ".wcode/design/product.yaml",
            &serde_yaml::to_string(&product).context("cannot encode product Design State")?,
        )?;
        workspace.create_file(
            ".wcode/design/constraints.yaml",
            &serde_yaml::to_string(&design::baseline_constraints())
                .context("cannot encode baseline Design constraints")?,
        )?;
        // Other collection documents remain sparse and appear only when the project has
        // meaningful desired state to declare in that domain.
        self.design_status(workspace_id, workspace)
    }

    pub fn software_graph(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SoftwareGraphSnapshot> {
        let mut snapshot = self.code_index.software_graph(
            workspace_id,
            workspace,
            path,
            max_files,
            max_symbols,
        )?;
        let load = design::load_design(workspace)?;
        let mut composite = false;
        if load.initialized {
            overlay_design_graph(&mut snapshot, &load.state, &self.code_index, workspace)?;
            composite = true;
        }
        if graph_provider_store::overlay_latest(workspace, &mut snapshot)? > 0 {
            composite = true;
        }
        if composite {
            snapshot.provider = "wcode-composite".to_owned();
            snapshot.precision = GraphPrecision::Mixed;
        }
        snapshot.node_count = snapshot.graph.nodes.len();
        snapshot.edge_count = snapshot.graph.edges.len();
        snapshot.graph.validate()?;
        graph_store::persist(workspace, &snapshot)?;
        Ok(snapshot)
    }

    pub fn graph_provider_import(
        &self,
        workspace: &Workspace,
        import: GraphProviderImport,
    ) -> Result<StoredGraphProvider> {
        graph_provider_store::persist(workspace, &import)
    }

    pub fn graph_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<GraphProviderSummary>> {
        graph_provider_store::summaries(workspace)
    }

    pub fn semantic_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<SemanticProviderStatus>> {
        semantic_provider::status(workspace, Some(&self.semantic_sessions))
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

    pub fn graph_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<GraphHistoryEntry>> {
        graph_store::history(workspace, limit)
    }

    pub fn graph_query(
        &self,
        workspace: &Workspace,
        input: &GraphQueryInput,
    ) -> Result<GraphQueryResult> {
        graph_store::query(workspace, input)
    }

    pub fn graph_diff(
        &self,
        workspace: &Workspace,
        input: &GraphDiffInput,
    ) -> Result<GraphDiffResult> {
        graph_store::diff(workspace, input)
    }

    pub fn traceability_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<TraceabilityStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.traceability_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
        )
    }

    pub fn drift_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<DriftStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.drift_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn risk_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<RiskStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.risk_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn impact_analysis(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<ImpactAnalysis> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.impact_analysis(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn verification_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<VerificationPlan> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.create_verification_plan(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn software_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        intent: &str,
        budget: usize,
        requested_scopes: &[String],
    ) -> Result<SoftwareContext> {
        let known_checks = self.known_checks(workspace)?;
        let request = SoftwareContextRequest {
            query: query.to_owned(),
            intent: intent.to_owned(),
            budget,
            scopes: requested_scopes.to_vec(),
        };
        self.intelligence.software_context(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            &request,
        )
    }

    pub async fn semantic_navigation(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        request: &SemanticNavigationRequest,
    ) -> Result<Value> {
        let path = request.path.as_str();
        let resolved = request
            .symbol
            .as_deref()
            .map(|symbol| self.code_index.resolve_symbol(workspace, path, symbol))
            .transpose()?
            .flatten();
        if request.symbol.is_some() && resolved.is_none() {
            bail!("symbol is ambiguous or was not found in path; call find_symbol first and pass a unique name or qualified name");
        }
        let (line, character) = match resolved.as_ref() {
            Some(symbol) => (symbol.start_line, symbol.start_column),
            None => (
                request
                    .line
                    .ok_or_else(|| anyhow::anyhow!("line is required when symbol is omitted"))?,
                request.character.ok_or_else(|| {
                    anyhow::anyhow!("character is required when symbol is omitted")
                })?,
            ),
        };
        if semantic_provider::language_for_path(path).is_none() {
            bail!("semantic navigation does not support this source language");
        }
        if !semantic_provider::provider_available_for_path(workspace, path) {
            let syntax_context = resolved.as_ref().and_then(|symbol| {
                self.code_index
                    .symbol_context(workspace_id, workspace, &symbol.id, 120)
                    .ok()
            });
            return Ok(json!({
                "workspace": workspace_id,
                "path": path,
                "provider": "tree-sitter",
                "precision": "syntax",
                "routing": "syntax_fallback",
                "reason": "no trusted LSP provider is available; use find_symbol/search_code for localization and treat cross-file relationships conservatively",
                "selector": resolved.as_ref().map(|symbol| json!({
                    "name": symbol.name,
                    "qualified_name": symbol.qualified_name,
                    "kind": symbol.kind,
                    "line": symbol.start_line,
                    "character": symbol.start_column,
                    "revision": symbol.revision,
                })),
                "syntax_context": syntax_context,
            }));
        }
        let navigation = semantic_provider::navigate(
            &self.semantic_sessions,
            workspace,
            path,
            u64::try_from(line).unwrap_or(u64::MAX),
            u64::try_from(character).unwrap_or(u64::MAX),
            request.intent,
            request.max_results,
        )
        .await?;
        let mut value = serde_json::to_value(navigation)?;
        value["workspace"] = json!(workspace_id);
        if let Some(symbol) = resolved {
            value["selector"] = json!({
                "name": symbol.name,
                "qualified_name": symbol.qualified_name,
                "kind": symbol.kind,
                "line": symbol.start_line,
                "character": symbol.start_column,
                "revision": symbol.revision,
            });
        }
        Ok(value)
    }

    pub fn semantic_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<SemanticStatusView> {
        self.intelligence
            .semantic_status(workspace_id, workspace, limit)
    }

    pub fn semantic_query(
        &self,
        workspace: &Workspace,
        query: &str,
        requested_scopes: &[String],
        include_candidates: bool,
        limit: usize,
    ) -> Result<Vec<SemanticMatch>> {
        self.intelligence.semantic_query(
            workspace,
            query,
            requested_scopes,
            include_candidates,
            limit,
        )
    }

    pub fn semantic_record_candidate(
        &self,
        workspace: &Workspace,
        input: SemanticCandidateInput,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_record_candidate(workspace, input)
    }

    pub fn semantic_confirm(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_confirm(workspace, fact_id, attested_by)
    }

    pub fn semantic_retire(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_retire(workspace, fact_id, attested_by)
    }

    pub fn reconciliation_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<ReconciliationPlan> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.reconciliation_plan(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn reconciliation_status(
        &self,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationPlan> {
        self.intelligence.reconciliation_status(workspace, plan_id)
    }

    pub fn reconciliation_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<ReconciliationPlan>> {
        self.intelligence.reconciliation_history(workspace, limit)
    }

    pub fn reconciliation_execution_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationExecutionStatus> {
        self.intelligence
            .reconciliation_execution_status(workspace_id, workspace, plan_id)
    }

    pub fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence
            .reconciliation_claim(workspace_id, workspace, plan_id, executor, kinds)
    }

    pub fn reconciliation_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence.reconciliation_submit(
            workspace_id,
            workspace,
            plan_id,
            task_id,
            executor,
            submission,
        )
    }

    pub fn reconciliation_retry(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence
            .reconciliation_retry(workspace_id, workspace, plan_id, task_id)
    }

    pub fn evidence_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        subject: Option<&str>,
        limit: usize,
    ) -> Result<EvidenceStatus> {
        self.intelligence
            .evidence_status(workspace_id, workspace, subject, limit)
    }

    pub fn verification_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        reviewer: &str,
        capabilities: &[String],
        role: Option<ReviewerRole>,
    ) -> Result<VerificationJob> {
        self.intelligence
            .verification_claim(workspace_id, workspace, reviewer, capabilities, role)
    }

    pub fn verification_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        job_id: &str,
        reviewer: &str,
        submission: ReviewSubmission,
    ) -> Result<VerificationJob> {
        self.intelligence
            .verification_submit(workspace_id, workspace, job_id, reviewer, submission)
    }

    pub fn verification_executor_status(
        &self,
        workspace: &Workspace,
    ) -> Result<StageExecutorRegistry> {
        stage_executor::registry(workspace)
    }

    pub fn language_quality_status(
        &self,
        workspace: &Workspace,
    ) -> Result<LanguageQualityRegistry> {
        quality_provider::registry(workspace, Some(&self.semantic_sessions))
    }

    pub async fn language_quality_run(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        language: crate::semantic_provider::SemanticLanguage,
        provider_id: &str,
        timeout_seconds: u64,
    ) -> Result<LanguageQualityRun> {
        let started = Instant::now();
        let mut run =
            quality_provider::execute(workspace, language, provider_id, timeout_seconds).await?;
        let elapsed_ms = started.elapsed().as_millis();
        let check = VerificationCheck {
            id: format!("quality-{}-{}", run.capability.as_str(), run.provider_id),
            phase: 0,
            command: command_text(&run.command.program, &run.command.args),
            reason: format!(
                "Run the repository-declared {} provider for {}.",
                run.capability.as_str(),
                language.as_str()
            ),
            success: run.success,
            exit_code: run.command.exit_code,
            elapsed_ms,
            stdout_tail: tail_chars(&run.command.stdout, MAX_CHECK_OUTPUT_CHARS).0,
            stderr_tail: tail_chars(&run.command.stderr, MAX_CHECK_OUTPUT_CHARS).0,
            output_truncated: run.command.truncated,
        };
        let report = VerificationReport {
            workspace: workspace_id.to_owned(),
            level: "language-quality".to_owned(),
            execution: "repository-declared-check-only-provider".to_owned(),
            phases_run: 1,
            passed: run.success,
            checks_run: 1,
            checks_failed: usize::from(!run.success),
            elapsed_ms,
            summary: run.summary.clone(),
            checks: vec![check],
        };
        run.evidence_records = self
            .intelligence
            .record_verification_report(workspace_id, workspace, &report)?
            .len();
        Ok(run)
    }

    pub async fn verification_execute_stages(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<Value> {
        let before = self.verification_status(workspace_id, workspace, plan_id)?;
        let registry = stage_executor::registry(workspace)?;
        let mut required = Vec::new();
        if before.plan.require_property {
            required.push(crate::verification::VerificationStage::Property);
        }
        if before.plan.require_mutation {
            required.push(crate::verification::VerificationStage::Mutation);
        }
        if before.plan.require_fuzz {
            required.push(crate::verification::VerificationStage::Fuzz);
        }
        if before
            .plan
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate")
        {
            required.push(crate::verification::VerificationStage::RuntimeCanary);
        }

        let detected = registry
            .detected_languages
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut results = Vec::<StageExecutionResult>::new();
        let mut missing = Vec::new();
        let mut skipped_passing = Vec::new();
        let mut execution_errors = Vec::new();
        for stage in required {
            let key = format!("{stage:?}").to_ascii_lowercase();
            let stage_already_passed = before
                .stage_results
                .get(&key)
                .is_some_and(|result| *result == crate::evidence::EvidenceResult::Pass);
            let executors = registry
                .executors
                .iter()
                .filter(|executor| {
                    executor.available
                        && executor.spec.stage == stage
                        && (executor.spec.languages.is_empty()
                            || executor
                                .spec
                                .languages
                                .iter()
                                .any(|language| detected.contains(language)))
                })
                .collect::<Vec<_>>();
            if executors.is_empty() {
                if !stage_already_passed {
                    missing.push(key);
                }
                continue;
            }
            for executor in executors {
                let producer = format!("executor:{}", executor.spec.id);
                if before
                    .stage_producer_results
                    .get(&key)
                    .and_then(|results| results.get(&producer))
                    .is_some_and(|result| *result == crate::evidence::EvidenceResult::Pass)
                {
                    skipped_passing.push(executor.spec.id.clone());
                    continue;
                }
                let execution = match stage_executor::execute(workspace, &executor.spec).await {
                    Ok(execution) => execution,
                    Err(error) => {
                        execution_errors.push(json!({
                            "executor_id": executor.spec.id,
                            "stage": key,
                            "error": error.to_string(),
                        }));
                        continue;
                    }
                };
                self.intelligence.verification_stage_submit(
                    workspace_id,
                    workspace,
                    plan_id,
                    StageSubmission {
                        stage: execution.stage,
                        producer,
                        verdict: execution.verdict,
                        summary: execution.summary.clone(),
                        artifact_digest: execution.artifact_digest.clone(),
                        model: None,
                    },
                )?;
                results.push(execution);
            }
        }
        let after = self.verification_status(workspace_id, workspace, plan_id)?;
        Ok(json!({
            "workspace": workspace_id,
            "plan_id": plan_id,
            "results": results,
            "skipped_passing_executors": skipped_passing,
            "execution_errors": execution_errors,
            "missing_executors": missing,
            "status": after,
        }))
    }

    pub fn verification_stage_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
    ) -> Result<crate::evidence::Evidence> {
        self.intelligence
            .verification_stage_submit(workspace_id, workspace, plan_id, submission)
    }

    pub fn verification_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<crate::evidence::Evidence> {
        self.intelligence.verification_approve(
            workspace_id,
            workspace,
            plan_id,
            approver,
            statement,
        )
    }

    pub fn verification_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<VerificationStatus> {
        let status = self
            .intelligence
            .verification_status(workspace_id, workspace, plan_id)?;
        if status.plan.workspace != workspace_id {
            bail!("verification plan does not belong to the selected workspace");
        }
        Ok(status)
    }

    pub fn verification_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        self.intelligence
            .verification_history(workspace_id, workspace, limit)
    }

    fn known_checks(&self, workspace: &Workspace) -> Result<HashSet<String>> {
        let (profile, _) = self.load_project_profile(workspace)?;
        Ok(profile
            .recommended_checks
            .iter()
            .map(|check| check.id.clone())
            .collect())
    }

    pub fn file_outline(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_symbols: usize,
    ) -> Result<Value> {
        self.code_index
            .file_outline(workspace_id, workspace, path, max_symbols)
    }

    pub fn find_symbol(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        path: &str,
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Value> {
        self.code_index
            .find_symbol(workspace_id, workspace, query, path, kind, max_results)
    }

    pub fn symbol_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        self.code_index
            .symbol_context(workspace_id, workspace, symbol_id, max_body_lines)
    }

    pub fn invalidate_code_file(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate(workspace.root(), path);
        self.invalidate_repo_map_cache(workspace.root());
    }

    pub fn invalidate_code_prefix(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate_prefix(workspace.root(), path);
        self.invalidate_repo_map_cache(workspace.root());
    }

    fn invalidate_repo_map_cache(&self, root: &Path) {
        if let Ok(mut cache) = self.repo_map_cache.lock() {
            cache.retain(|(cached_root, _), _| cached_root != root);
        }
    }

    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "tool harness is shutting down".to_owned())?;
        // Re-evaluate pressure at the point work can actually start. A request
        // may have waited in the semaphore queue while memory climbed.
        crate::resource::global().admit_tool().await?;
        Ok(permit)
    }
}
