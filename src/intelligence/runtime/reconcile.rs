use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn software_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        request: &SoftwareContextRequest,
    ) -> Result<SoftwareContext> {
        let workspace_id = workspace_id.into();
        let query = request.query.trim();
        if query.is_empty() {
            return Err(anyhow!("software context query must not be empty"));
        }
        let design_load = design::load_design(workspace)?;
        let state = &design_load.state;
        let budget = request.budget.clamp(1_000, 64_000);
        let item_cap = (budget / 900).clamp(4, MAX_CONTEXT_ITEMS);
        let requested_scopes = scopes::canonicalize(&request.scopes);
        let semantic_matches = semantic::query_scoped(
            semantic_store::load(workspace)?,
            query,
            &requested_scopes,
            true,
            item_cap,
        );
        let mut semantic_expansion = query.to_owned();
        for scope in &requested_scopes {
            semantic_expansion.push(' ');
            semantic_expansion.push_str(scope);
        }
        for matched in semantic_matches
            .iter()
            .filter(|matched| matched.fact.status == SemanticStatus::Confirmed)
            .take(8)
        {
            for term in matched.fact.expansion_terms() {
                semantic_expansion.push(' ');
                semantic_expansion.push_str(&term);
            }
        }
        let tokens = context_tokens(&semantic_expansion);
        let requirements = ranked_context_ids(
            state.requirements.values().map(|requirement| {
                (
                    requirement.id.clone(),
                    format!(
                        "{} {} {}",
                        requirement.id, requirement.title, requirement.intent
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let components = ranked_context_ids(
            state.components.values().map(|component| {
                (
                    component.id.clone(),
                    format!(
                        "{} {} {}",
                        component.id,
                        component.name,
                        component.responsibilities.join(" ")
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let constraints = ranked_context_ids(
            state.constraints.values().map(|constraint| {
                (
                    constraint.id.clone(),
                    format!(
                        "{} {} {}",
                        constraint.id, constraint.title, constraint.statement
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let acceptance_criteria = ranked_context_ids(
            state.acceptance.values().map(|criterion| {
                (
                    criterion.id.clone(),
                    format!(
                        "{} {} {}",
                        criterion.id, criterion.title, criterion.statement
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let decisions = ranked_context_ids(
            state.decisions.values().map(|decision| {
                (
                    decision.id.clone(),
                    format!(
                        "{} {} {} {}",
                        decision.id, decision.title, decision.decision, decision.rationale
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let design_items = design_context_items(
            state,
            &requirements,
            &components,
            &constraints,
            &acceptance_criteria,
            &decisions,
            item_cap,
        );

        let symbol_cap = item_cap.min(24);
        let mut symbols = Vec::new();
        let mut symbol_ids = HashSet::new();
        let mut symbol_queries = tokens
            .iter()
            .filter(|token| token.len() >= 3)
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        if symbol_queries.is_empty() {
            symbol_queries.push(query.to_owned());
        }
        let source_roots = scopes::source_roots_for(&requested_scopes);
        for source_root in &source_roots {
            let search = code_index.find_symbols_many(
                workspace_id.clone(),
                workspace,
                &symbol_queries,
                source_root,
                None,
                symbol_cap,
            )?;
            for symbol in search
                .get("results")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let key = symbol
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| symbol.to_string());
                if symbol_ids.insert(key) {
                    symbols.push(symbol.clone());
                    if symbols.len() >= symbol_cap {
                        break;
                    }
                }
            }
            if symbols.len() >= symbol_cap {
                break;
            }
        }
        let graph_context = provider_graph_context(
            workspace,
            &semantic_expansion,
            &tokens,
            &symbols,
            item_cap.min(32),
        )?;
        let known_risks = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?
            .latest_risks
            .get(&workspace_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(item_cap)
            .collect::<Vec<_>>();
        let mut coverage = self.traceability_status_from_load(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            &design_load,
        )?;
        let requirement_rank = requirements
            .iter()
            .enumerate()
            .map(|(rank, id)| (id.as_str(), rank))
            .collect::<HashMap<_, _>>();
        let relevant_components = components
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if !requirement_rank.is_empty() || !relevant_components.is_empty() {
            coverage.requirements.retain(|requirement| {
                requirement_rank.contains_key(requirement.id.as_str())
                    || requirement
                        .components
                        .iter()
                        .any(|component| relevant_components.contains(component.as_str()))
            });
            coverage.requirements.sort_by_key(|requirement| {
                requirement_rank
                    .get(requirement.id.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        if coverage.requirements.len() > item_cap {
            coverage.requirements.truncate(item_cap);
        }
        coverage.requirements_returned = coverage.requirements.len();
        coverage.truncated |= coverage.requirements_returned < coverage.requirements_total;
        if coverage.diagnostics.len() > item_cap {
            coverage.diagnostics.truncate(item_cap);
            coverage.truncated = true;
        }
        Ok(SoftwareContext {
            workspace: workspace_id,
            query: query.to_owned(),
            intent: request.intent.clone(),
            budget,
            scopes: requested_scopes,
            requirements,
            components,
            constraints,
            acceptance_criteria,
            decisions,
            design_items,
            semantic_matches,
            symbols,
            graph_context,
            known_risks,
            coverage,
        })
    }

    pub(crate) fn reconciliation_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<ReconciliationPlan> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let design_state = design::load_design(workspace)?.state;
        let mut graph = code_index.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_IMPACT_GRAPH_FILES,
            MAX_IMPACT_GRAPH_SYMBOLS,
        )?;
        graph_provider_store::overlay_latest(workspace, &mut graph)?;
        let impact = build_impact_analysis(
            workspace_id.clone(),
            &design_state,
            review,
            risk.level,
            Some(&graph),
        );
        let verification_plan = self.create_plan_for_risk(&workspace_id, workspace, risk.level)?;
        let mut tasks = Vec::new();
        let mut intents = Vec::new();
        for finding in &risk.drift.findings {
            let task_id = self.next_id("RT");
            let (kind, intent) = match finding.kind {
                DriftKind::ImplementationDrift => (
                    ReconciliationTaskKind::Implementation,
                    ChangeIntent::ChangeBehavior {
                        target: finding.subject.clone(),
                        desired: serde_json::json!({"state":"conform_to_design"}),
                        constraints: Vec::new(),
                    },
                ),
                DriftKind::DesignDrift => (
                    ReconciliationTaskKind::Design,
                    ChangeIntent::UpdateDesign {
                        subject: finding.subject.clone(),
                        reason: finding.message.clone(),
                    },
                ),
            };
            tasks.push(ReconciliationTask {
                id: task_id,
                kind,
                subject: finding.subject.clone(),
                description: finding.message.clone(),
                depends_on: Vec::new(),
            });
            intents.push(intent);
        }
        let prior_tasks = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
        tasks.push(ReconciliationTask {
            id: self.next_id("RT"),
            kind: ReconciliationTaskKind::Verification,
            subject: verification_plan.subject.clone(),
            description: format!(
                "Run {} deterministic verification and complete {} blind reviewer job(s).",
                verification_plan.deterministic_level,
                verification_plan.job_ids.len()
            ),
            depends_on: prior_tasks,
        });
        if verification_plan.require_human_approval {
            let verification_task = tasks
                .last()
                .map(|task| task.id.clone())
                .into_iter()
                .collect();
            tasks.push(ReconciliationTask {
                id: self.next_id("RT"),
                kind: ReconciliationTaskKind::HumanApproval,
                subject: verification_plan.subject.clone(),
                description: "Critical-risk reconciliation requires explicit human approval."
                    .into(),
                depends_on: verification_task,
            });
        }
        let impacted_tests = risk
            .traceability
            .requirements
            .iter()
            .filter(|requirement| impact.impacted_requirements.contains(&requirement.id))
            .flat_map(|requirement| requirement.verification.iter())
            .filter(|reference| reference.kind == TraceReferenceKind::Test)
            .map(|reference| reference.target.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let plan = ReconciliationPlan {
            id: self.next_id("RP"),
            workspace: workspace_id,
            risk_level: risk.level,
            design_changes: design_changes_from_review(review),
            drift_ids: risk
                .drift
                .findings
                .iter()
                .map(|finding| finding.id.clone())
                .collect(),
            impacted_components: impact.impacted_components,
            impacted_symbols: impact.impacted_symbols,
            impacted_tests,
            implementation_tasks: tasks,
            change_intents: intents,
            verification_plan,
        };
        plan.validate()?;
        reconciliation_store::persist(workspace, &plan)?;
        if reconciliation_execution_store::load(workspace, &plan.id)?.is_none() {
            let execution = ReconciliationExecution::from_plan(&plan)?;
            reconciliation_execution_store::persist(workspace, &execution)?;
        }
        Ok(plan)
    }

    pub(crate) fn reconciliation_status(
        &self,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationPlan> {
        reconciliation_store::load(workspace, plan_id)?
            .ok_or_else(|| anyhow!("reconciliation plan does not exist"))
    }

    pub(crate) fn reconciliation_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<ReconciliationPlan>> {
        reconciliation_store::recent(workspace, limit)
    }

    pub(crate) fn reconciliation_execution_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationExecutionStatus> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let verification =
            self.verification_status(workspace_id, workspace, &plan.verification_plan.id)?;
        let mut changed = execution.set_system_task(
            ReconciliationTaskKind::Verification,
            verification.ready,
            if verification.ready {
                "Verification Plan is ready with all required evidence.".into()
            } else {
                format!(
                    "Verification blockers: {}",
                    verification.blockers.join(", ")
                )
            },
        );
        if plan.verification_plan.require_human_approval {
            changed |= execution.set_system_task(
                ReconciliationTaskKind::HumanApproval,
                verification.human_approval,
                if verification.human_approval {
                    "Explicit HumanApproval Evidence is present.".into()
                } else {
                    "Explicit HumanApproval Evidence is still required.".into()
                },
            );
        }
        if changed || reconciliation_execution_store::load(workspace, plan_id)?.is_none() {
            reconciliation_execution_store::persist(workspace, &execution)?;
        }
        Ok(execution.status())
    }

    pub(crate) fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun> {
        self.reconciliation_execution_status(workspace_id, workspace, plan_id)?;
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .ok_or_else(|| anyhow!("reconciliation execution state does not exist"))?;
        let run = execution.claim(executor, kinds)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        Ok(run)
    }

    pub(crate) fn reconciliation_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let run = execution.submit(task_id, executor, submission)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            format!("reconciliation-task:{}", run.task.id),
            EvidenceKind::Reconciliation,
            format!("executor:{executor}"),
            workspace_revision(workspace)?,
            if run.status == ReconciliationRunStatus::Completed {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
            Confidence::High,
        )?;
        evidence.policy = Some(format!("reconciliation/{plan_id}"));
        evidence.summary = run.summary.clone();
        evidence.artifact_digest = run.artifact_digest.clone();
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence);
        Ok(run)
    }

    pub(crate) fn reconciliation_retry(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
    ) -> Result<ReconciliationTaskRun> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let run = execution.retry(task_id)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        Ok(run)
    }
}
