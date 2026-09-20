use super::*;

impl ToolHarness {
    pub fn new(max_parallel: usize) -> Result<Self> {
        if !(1..=MAX_PARALLEL_TOOLS).contains(&max_parallel) {
            bail!("max parallel tools must be between 1 and {MAX_PARALLEL_TOOLS}");
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(max_parallel)),
            execution_slots: Arc::new(Semaphore::new(Self::execution_limit(max_parallel))),
            max_parallel,
            project_cache: Default::default(),
            project_flights: Default::default(),
            observatory_cache: Default::default(),
            observatory_refreshes: Default::default(),
            convention_cache: Default::default(),
            convention_flights: Default::default(),
            repo_map_cache: Default::default(),
            repo_map_flights: Default::default(),
            verification_cache: Default::default(),
            verification_run_flights: Default::default(),
            code_index: CodeIndex::new()?,
            semantic_sessions: SemanticSessionPool::default(),
            intelligence: SoftwareIntelligenceRuntime::default(),
        })
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub(crate) fn current_revision(
        &self,
        workspace: &Workspace,
    ) -> Result<crate::evidence::Revision> {
        self.intelligence.current_revision(workspace)
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
        let existing = self.intelligence.design_load(workspace)?;
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
            name: format!("{name} Engineering Control Plane"),
            vision: "Coding agents operate through an observable engineering control plane where software continuously converges toward intended design with verifiable evidence."
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
        self.intelligence.invalidate_design_cache(workspace.root());
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
        let load = self.intelligence.design_load(workspace)?;
        self.software_graph_from_design(
            workspace_id,
            workspace,
            path,
            max_files,
            max_symbols,
            load.as_ref(),
        )
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
        if semantic_provider::language_for_path(path).is_none() {
            bail!("semantic navigation does not support this source language");
        }
        if request.intent == SemanticNavigationIntent::OrganizeImportsPlan {
            if !semantic_provider::provider_available_for_path(workspace, path) {
                bail!(
                    "LSP organize imports is unavailable; mutation has no syntax fallback because wcode will not guess import edits"
                );
            }
            let mut value =
                semantic_provider::organize_imports_plan(&self.semantic_sessions, workspace, path)
                    .await?;
            value["workspace"] = json!(workspace_id);
            return Ok(value);
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
        if request.intent == SemanticNavigationIntent::QuickFixPlan {
            if !semantic_provider::provider_available_for_path(workspace, path) {
                bail!(
                    "LSP quick fix is unavailable; mutation has no syntax fallback because wcode will not invent provider diagnostics or edits"
                );
            }
            let mut value = semantic_provider::quick_fix_plan(
                &self.semantic_sessions,
                workspace,
                path,
                u64::try_from(line).unwrap_or(u64::MAX),
                u64::try_from(character).unwrap_or(u64::MAX),
                request.max_results,
            )
            .await?;
            value["workspace"] = json!(workspace_id);
            return Ok(value);
        }
        if request.intent == SemanticNavigationIntent::RenamePlan {
            let symbol = resolved.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "semantic rename requires symbol; call find_symbol first and pass a unique name or qualified name"
                )
            })?;
            let new_name = request
                .new_name
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("new_name is required for rename_plan"))?;
            if !semantic_provider::provider_available_for_path(workspace, path) {
                bail!(
                    "LSP semantic rename is unavailable; mutation has no syntax fallback because wcode will not guess cross-file rename targets"
                );
            }
            let mut value = semantic_provider::rename_plan(
                &self.semantic_sessions,
                workspace,
                semantic_provider::RenamePlanRequest {
                    path,
                    line: u64::try_from(line).unwrap_or(u64::MAX),
                    character: u64::try_from(character).unwrap_or(u64::MAX),
                    old_name: &symbol.name,
                    new_name,
                    max_files: request.max_files,
                },
            )
            .await?;
            value["workspace"] = json!(workspace_id);
            value["selector"] = json!({
                "name": symbol.name,
                "qualified_name": symbol.qualified_name,
                "kind": symbol.kind,
                "line": symbol.start_line,
                "character": symbol.start_column,
                "revision": symbol.revision,
            });
            return Ok(value);
        }
        let degraded = |reason: String| {
            let syntax_context = resolved.as_ref().and_then(|symbol| {
                self.code_index
                    .symbol_context(workspace_id, workspace, &symbol.id, 120)
                    .ok()
            });
            let candidate_limit = 500;
            let target_language = semantic_provider::language_for_path(path);
            let mut keyword_matches = request
                .symbol
                .as_deref()
                .and_then(|symbol| workspace.search(symbol, ".", candidate_limit).ok())
                .unwrap_or_default();
            keyword_matches.sort_by_key(|item| {
                let language = item
                    .get("path")
                    .and_then(Value::as_str)
                    .and_then(semantic_provider::language_for_path);
                match language {
                    Some(language) if Some(language) == target_language => 0,
                    Some(_) => 1,
                    None => 2,
                }
            });
            keyword_matches.truncate(request.max_results);
            let syntax_calls = resolved.as_ref().and_then(|symbol| {
                self.code_index
                    .syntax_call_navigation_from_matches(
                        workspace,
                        symbol,
                        &keyword_matches,
                        request.max_results,
                    )
                    .ok()
            });
            json!({
                "workspace": workspace_id,
                "path": path,
                "provider": "tree-sitter+search",
                "precision": "syntax",
                "routing": "degraded_syntax_and_keyword_search",
                "degraded": true,
                "degraded_from": "lsp",
                "reason": reason,
                "fallback_capabilities": ["tree_sitter_symbol_context", "syntax_call_graph", "exact_keyword_search"],
                "selector": resolved.as_ref().map(|symbol| json!({
                    "name": symbol.name,
                    "qualified_name": symbol.qualified_name,
                    "kind": symbol.kind,
                    "line": symbol.start_line,
                    "character": symbol.start_column,
                    "revision": symbol.revision,
                })),
                "syntax_context": syntax_context,
                "syntax_calls": syntax_calls,
                "keyword_matches": keyword_matches,
            })
        };
        if !semantic_provider::provider_available_for_path(workspace, path) {
            return Ok(degraded(
                "LSP semantic navigation is unavailable; degraded explicitly to Tree-sitter syntax, bounded syntax call-graph evidence, and exact keyword search."
                    .to_owned(),
            ));
        }
        let navigation = match semantic_provider::navigate(
            &self.semantic_sessions,
            workspace,
            path,
            u64::try_from(line).unwrap_or(u64::MAX),
            u64::try_from(character).unwrap_or(u64::MAX),
            request.intent,
            request.max_results,
        )
        .await
        {
            Ok(navigation) => navigation,
            Err(error) => {
                return Ok(degraded(format!(
                    "LSP semantic navigation failed ({error}); degraded explicitly to Tree-sitter syntax, bounded syntax call-graph evidence, and exact keyword search."
                )));
            }
        };
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

    pub fn reconciliation_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Value> {
        self.intelligence.reconciliation_approve(
            workspace_id,
            workspace,
            plan_id,
            approver,
            statement,
        )
    }

    pub fn reconciliation_approval_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<Value> {
        self.intelligence
            .reconciliation_approval_status(workspace_id, workspace, plan_id)
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

    pub(crate) fn intelligence_status_proof_reconciliation_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        evidence_limit: usize,
        verification_limit: usize,
        reconciliation_limit: usize,
    ) -> Result<(Value, Value, Value, Value)> {
        let (evidence, reconciliation, reconciliation_execution, verification) =
            self.intelligence.status_proof_reconciliation_snapshot(
                workspace_id,
                workspace,
                evidence_limit,
                verification_limit,
                reconciliation_limit,
            )?;
        Ok((
            serde_json::to_value(evidence)?,
            serde_json::to_value(reconciliation)?,
            serde_json::to_value(reconciliation_execution)?,
            serde_json::to_value(verification)?,
        ))
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
        let (profile, _) = self.load_project_profile(workspace)?;
        self.language_quality_status_from_profile(workspace, profile.as_ref())
    }

    pub async fn language_quality_run(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        language: crate::semantic_provider::SemanticLanguage,
        provider_id: &str,
        timeout_seconds: u64,
    ) -> Result<LanguageQualityRun> {
        let design = self.intelligence.design_load(workspace)?;
        let revision = self
            .intelligence
            .current_revision_from_load(workspace, design.as_ref())?;
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
            reused: false,
            exit_code: run.command.exit_code,
            elapsed_ms,
            queue_wait_ms: run.command.process_queue_wait_ms,
            execution_ms: elapsed_ms.saturating_sub(u128::from(run.command.process_queue_wait_ms)),
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
            checks_reused: 0,
            checks_failed: usize::from(!run.success),
            skipped_checks: Vec::new(),
            elapsed_ms,
            summary: run.summary.clone(),
            impact: None,
            cost_model: None,
            checks: vec![check],
        };
        run.evidence_records = self
            .intelligence
            .record_verification_report_from_design(
                workspace_id,
                workspace,
                &revision,
                Some(design.as_ref()),
                &report,
            )?
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

        let required_targets = before.plan.stage_targets.clone();
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
            let pending_targets = required_targets
                .iter()
                .filter(|target| {
                    before
                        .stage_target_results
                        .get(&key)
                        .and_then(|results| results.get(*target))
                        != Some(&crate::evidence::EvidenceResult::Pass)
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            let executors = registry
                .executors
                .iter()
                .filter(|executor| {
                    executor.available
                        && executor.spec.stage == stage
                        && (required_targets.is_empty()
                            || !stage_executor::executor_targets(&executor.spec, &required_targets)
                                .is_empty())
                })
                .collect::<Vec<_>>();
            if required_targets.is_empty() {
                if executors.is_empty() && !stage_already_passed {
                    missing.push(key.clone());
                }
            } else {
                for target in &pending_targets {
                    if !executors.iter().any(|executor| {
                        stage_executor::executor_targets(&executor.spec, &required_targets)
                            .iter()
                            .any(|covered| covered == target)
                    }) {
                        missing.push(format!("{key}:{target}"));
                    }
                }
            }
            for executor in executors {
                let targets = stage_executor::executor_targets(&executor.spec, &required_targets);
                if required_targets.is_empty() {
                    let producer = format!("executor:{}", executor.spec.id);
                    if stage_already_passed
                        && before
                            .stage_producer_results
                            .get(&key)
                            .and_then(|results| results.get(&producer))
                            .is_some_and(|result| *result == crate::evidence::EvidenceResult::Pass)
                    {
                        skipped_passing.push(executor.spec.id.clone());
                        continue;
                    }
                } else if !targets
                    .iter()
                    .any(|target| pending_targets.contains(target))
                {
                    skipped_passing.push(executor.spec.id.clone());
                    continue;
                }
                let producer = format!("executor:{}", executor.spec.id);
                let execution = match stage_executor::execute(workspace, &executor.spec).await {
                    Ok(execution) => execution,
                    Err(error) => {
                        execution_errors.push(json!({
                            "executor_id": executor.spec.id,
                            "stage": key,
                            "targets": targets,
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
                        targets,
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

    pub(super) fn known_checks(&self, workspace: &Workspace) -> Result<HashSet<String>> {
        let (profile, _) = self.load_project_profile(workspace)?;
        Ok(harness_profile::known_checks_from_profile(profile.as_ref()))
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
        self.invalidate_convention_cache(workspace.root());
        self.intelligence.invalidate_design_cache(workspace.root());
    }

    pub fn invalidate_code_prefix(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate_prefix(workspace.root(), path);
        self.invalidate_repo_map_cache(workspace.root());
        self.invalidate_convention_cache(workspace.root());
        self.intelligence.invalidate_design_cache(workspace.root());
    }

    fn invalidate_convention_cache(&self, root: &Path) {
        self.invalidate_convention_flights(Some(root));
        if let Ok(mut cache) = self.convention_cache.lock() {
            cache.remove(root);
        }
    }

    fn invalidate_repo_map_cache(&self, root: &Path) {
        self.invalidate_repo_map_flights(Some(root));
        if let Ok(mut cache) = self.repo_map_cache.lock() {
            cache.retain(|(cached_root, _), _| cached_root != root);
        }
    }
}
