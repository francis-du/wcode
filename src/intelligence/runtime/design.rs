use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn current_revision(&self, workspace: &Workspace) -> Result<Revision> {
        workspace_revision(workspace)
    }

    pub fn design_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<DesignStatus> {
        let load = design::load_design(workspace)?;
        let errors = load.error_count();
        let warnings = load.warning_count();
        let state = load.state;
        Ok(DesignStatus {
            workspace: workspace_id.into(),
            initialized: load.initialized,
            valid: load.initialized && errors == 0,
            schema_version: 1,
            design_root: load.design_root,
            files_loaded: load.files_loaded,
            project: state.project.as_ref().map(|project| project.name.clone()),
            requirements: state.requirements.len(),
            components: state.components.len(),
            constraints: state.constraints.len(),
            decisions: state.decisions.len(),
            acceptance_criteria: state.acceptance.len(),
            design_nodes: state.node_count(),
            errors,
            warnings,
            diagnostics: load.diagnostics,
        })
    }

    pub(crate) fn traceability_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
    ) -> Result<TraceabilityStatus> {
        let load = design::load_design(workspace)?;
        self.traceability_status_from_load(
            workspace_id.into(),
            workspace,
            code_index,
            known_checks,
            &load,
        )
    }

    pub(super) fn traceability_status_from_load(
        &self,
        workspace_id: String,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        load: &design::DesignLoad,
    ) -> Result<TraceabilityStatus> {
        let errors = load.error_count();
        let initialized = load.initialized;
        let mut diagnostics = load.diagnostics.clone();
        let state = &load.state;
        let requirements_total = state.requirements.len();
        let mut requirement_components_covered = 0usize;
        let mut implementation_total = 0usize;
        let mut implementation_resolved = 0usize;
        let mut verification_total = 0usize;
        let mut verification_resolved = 0usize;
        let mut complete_requirements = 0usize;
        let mut partial_requirements = 0usize;
        let mut missing_requirements = 0usize;
        let mut requirements = Vec::new();

        for requirement in state.requirements.values() {
            let components_resolved = !requirement.implemented_by.is_empty()
                && requirement
                    .implemented_by
                    .iter()
                    .all(|id| state.components.contains_key(id));
            requirement_components_covered += usize::from(components_resolved);
            let implementation = requirement
                .implemented_by
                .iter()
                .filter_map(|id| state.components.get(id))
                .flat_map(|component| {
                    component.implementation.iter().map(|reference| {
                        resolve_code_reference(code_index, workspace, &component.id, reference)
                    })
                })
                .collect::<Vec<_>>();
            let verification = requirement
                .acceptance
                .iter()
                .filter_map(|id| state.acceptance.get(id))
                .flat_map(|criterion| {
                    criterion.verification.iter().map(|reference| {
                        resolve_verification_reference(
                            code_index,
                            workspace,
                            known_checks,
                            &criterion.id,
                            reference,
                        )
                    })
                })
                .collect::<Vec<_>>();

            implementation_total += implementation.len();
            implementation_resolved += implementation.iter().filter(|item| item.resolved).count();
            verification_total += verification.len();
            verification_resolved += verification.iter().filter(|item| item.resolved).count();
            let status =
                requirement_trace_status(components_resolved, &implementation, &verification);
            match status {
                RequirementTraceStatus::Complete => complete_requirements += 1,
                RequirementTraceStatus::Partial => partial_requirements += 1,
                RequirementTraceStatus::Missing => missing_requirements += 1,
            }
            if requirements.len() < MAX_TRACE_REQUIREMENTS {
                requirements.push(RequirementTrace {
                    id: requirement.id.clone(),
                    title: requirement.title.clone(),
                    priority: requirement.priority,
                    components: requirement.implemented_by.clone(),
                    acceptance_criteria: requirement.acceptance.clone(),
                    implementation,
                    verification,
                    status,
                });
            }
        }

        let truncated =
            requirements_total > requirements.len() || diagnostics.len() > MAX_TRACE_DIAGNOSTICS;
        diagnostics.truncate(MAX_TRACE_DIAGNOSTICS);
        Ok(TraceabilityStatus {
            workspace: workspace_id,
            initialized,
            valid_design: initialized && errors == 0,
            requirements_total,
            requirements_returned: requirements.len(),
            truncated,
            requirement_to_component: CoverageDimension::new(
                requirement_components_covered,
                requirements_total,
            ),
            design_to_implementation: CoverageDimension::new(
                implementation_resolved,
                implementation_total,
            ),
            acceptance_to_verification: CoverageDimension::new(
                verification_resolved,
                verification_total,
            ),
            complete_requirements,
            partial_requirements,
            missing_requirements,
            requirements,
            diagnostics,
        })
    }

    pub(crate) fn drift_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<DriftStatus> {
        let workspace_id = workspace_id.into();
        let traceability =
            self.traceability_status(workspace_id.clone(), workspace, code_index, known_checks)?;
        let state = design::load_design(workspace)?.state;
        Ok(build_drift_status(
            workspace_id,
            &state,
            &traceability,
            review,
        ))
    }

    pub(crate) fn risk_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<RiskStatus> {
        let workspace_id = workspace_id.into();
        let traceability =
            self.traceability_status(workspace_id.clone(), workspace, code_index, known_checks)?;
        let state = design::load_design(workspace)?.state;
        let drift = build_drift_status(workspace_id.clone(), &state, &traceability, review);
        let (level, mut risks) = assess_risk(&workspace_id, review, &traceability, &drift);
        let profile = VerificationProfile::for_risk(level);
        if !required_verification_stages(&profile).is_empty() {
            let registry = stage_executor::registry(workspace)?;
            let stage_targets = verification_targets_for_review(review, &registry);
            append_verification_automation_gap(
                &workspace_id,
                &profile,
                &registry,
                &stage_targets,
                &mut risks,
            );
        }
        self.state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?
            .latest_risks
            .insert(workspace_id.clone(), risks.clone());
        Ok(RiskStatus {
            workspace: workspace_id,
            level,
            profile,
            risks,
            drift,
            traceability,
        })
    }

    pub(crate) fn impact_analysis(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<ImpactAnalysis> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let state = design::load_design(workspace)?.state;
        let mut graph = code_index.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_IMPACT_GRAPH_FILES,
            MAX_IMPACT_GRAPH_SYMBOLS,
        )?;
        graph_provider_store::overlay_latest(workspace, &mut graph)?;
        Ok(build_impact_analysis(
            workspace_id,
            &state,
            review,
            risk.level,
            Some(&graph),
        ))
    }

    pub(crate) fn create_verification_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<VerificationPlan> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let registry = stage_executor::registry(workspace)?;
        let stage_targets = verification_targets_for_review(review, &registry);
        self.create_plan_for_risk_with_targets(
            &workspace_id,
            workspace,
            risk.level,
            stage_targets,
            &registry,
        )
    }

    pub(crate) fn verification_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        reviewer: &str,
        capabilities: &[String],
        role: Option<ReviewerRole>,
    ) -> Result<VerificationJob> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let capabilities = capabilities.iter().cloned().collect::<BTreeSet<_>>();
        let (job, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state
                .verification
                .claim(workspace_id, reviewer, &capabilities, role)?;
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (job, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        Ok(job)
    }

    pub(crate) fn verification_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        job_id: &str,
        reviewer: &str,
        submission: ReviewSubmission,
    ) -> Result<VerificationJob> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = workspace_revision(workspace)?;
        let (job, produced, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state
                .verification
                .submit(workspace_id, job_id, reviewer, submission)?;
            let status = state.verification.status(&job.plan_id)?;
            let evidence = VerificationState::evidence_for_submission(
                &job,
                self.next_id("EV"),
                revision.clone(),
                status.plan.policy.clone(),
            )?;
            let mut produced = vec![evidence.clone()];
            push_evidence(&mut state.evidence, workspace_id, evidence);
            if status.disagreements > 0 {
                let producer = format!("verification-mesh:{}", status.plan.id);
                let already_recorded = state.evidence.iter().any(|stored| {
                    stored.workspace == workspace_id
                        && stored.evidence.producer == producer
                        && stored.evidence.result == EvidenceResult::Disagree
                });
                if !already_recorded {
                    let mut disagreement = Evidence::new(
                        self.next_id("EV"),
                        status.plan.subject.clone(),
                        EvidenceKind::ModelReview,
                        producer,
                        revision,
                        EvidenceResult::Disagree,
                        Confidence::High,
                    )?;
                    disagreement.policy = Some(status.plan.policy.clone());
                    disagreement.artifact_digest = Some(digest_text(&format!(
                        "plan={};submitted={};disagreements={}",
                        status.plan.id, status.submitted, status.disagreements
                    )));
                    disagreement.validate()?;
                    produced.push(disagreement.clone());
                    push_evidence(&mut state.evidence, workspace_id, disagreement);
                }
            }
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (job, produced, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        for evidence in &produced {
            evidence_store::persist(workspace, evidence)?;
        }
        Ok(job)
    }

    pub(crate) fn verification_stage_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
    ) -> Result<Evidence> {
        submission.validate()?;
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let plan = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.status(plan_id)?.plan
        };
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        let required = match submission.stage {
            VerificationStage::Property => plan.require_property,
            VerificationStage::Mutation => plan.require_mutation,
            VerificationStage::Fuzz => plan.require_fuzz,
            VerificationStage::RuntimeCanary => plan
                .deterministic_checks
                .iter()
                .any(|check| check == "runtime-gate"),
        };
        if !required {
            return Err(anyhow!("verification stage is not required by this plan"));
        }
        if !plan.stage_targets.is_empty()
            && (submission.targets.is_empty()
                || submission
                    .targets
                    .iter()
                    .any(|target| !plan.stage_targets.contains(target)))
        {
            return Err(anyhow!(
                "stage evidence must explicitly cover only targets declared by this verification plan"
            ));
        }
        let kind = match submission.stage {
            VerificationStage::Property => EvidenceKind::Property,
            VerificationStage::Mutation => EvidenceKind::Mutation,
            VerificationStage::Fuzz => EvidenceKind::Fuzz,
            VerificationStage::RuntimeCanary => EvidenceKind::Runtime,
        };
        let result = match submission.verdict {
            crate::verification::ReviewVerdict::Pass => EvidenceResult::Pass,
            crate::verification::ReviewVerdict::Fail => EvidenceResult::Fail,
            crate::verification::ReviewVerdict::Inconclusive => EvidenceResult::Inconclusive,
        };
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            plan.subject.clone(),
            kind,
            submission.producer.clone(),
            workspace_revision(workspace)?,
            result,
            Confidence::High,
        )?;
        evidence.model = submission.model.clone();
        evidence.policy =
            Some(format!("{}/stage/{:?}", plan.policy, submission.stage).to_ascii_lowercase());
        evidence.artifact_digest = Some(submission.artifact_digest.clone());
        evidence.summary = Some(submission.summary.clone());
        evidence.targets = submission.targets.clone();
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn verification_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Evidence> {
        let approver = approver.trim();
        let statement = statement.trim();
        if approver.is_empty()
            || approver.len() > 256
            || statement.is_empty()
            || statement.len() > 2_000
        {
            return Err(anyhow!("human approval identity or statement is invalid"));
        }
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let plan = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.status(plan_id)?.plan
        };
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        if !plan.require_human_approval {
            return Err(anyhow!("verification plan does not require human approval"));
        }
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            plan.subject.clone(),
            EvidenceKind::HumanApproval,
            format!("human:{approver}"),
            workspace_revision(workspace)?,
            EvidenceResult::Pass,
            Confidence::High,
        )?;
        evidence.policy = Some(format!("{}/human-approval", plan.policy));
        evidence.artifact_digest = Some(format!("sha256:{}", digest_text(statement)));
        evidence.summary = Some(statement.to_owned());
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn verification_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<VerificationStatus> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let (mut status, memory_evidence) = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let status = state.verification.status(plan_id)?;
            let memory_evidence = state
                .evidence
                .iter()
                .filter(|stored| stored.workspace == status.plan.workspace)
                .map(|stored| stored.evidence.clone())
                .collect::<Vec<_>>();
            (status, memory_evidence)
        };
        let current_revision = workspace_revision(workspace)?;
        if let Some(plan_revision) = status.plan.revision.as_ref() {
            if current_revision.code != plan_revision.code {
                status
                    .blockers
                    .push("workspace-revision-changed-since-plan".into());
            }
            if current_revision.design != plan_revision.design {
                status
                    .blockers
                    .push("design-revision-changed-since-plan".into());
            }
        } else {
            let current_subject = format!("change:{}", current_revision.code);
            if current_subject != status.plan.subject {
                status
                    .blockers
                    .push("workspace-revision-changed-since-plan".into());
            }
        }
        let mut evidence = evidence_store::load(workspace)?;
        evidence.extend(memory_evidence);
        status.deterministic_result = evidence
            .iter()
            .filter(|record| {
                evidence_matches_plan_revision(record, &status.plan)
                    && record.kind == EvidenceKind::Verification
            })
            .max_by_key(|record| record.timestamp_ms)
            .map(|record| record.result);
        match status.deterministic_result {
            Some(EvidenceResult::Pass) => {}
            Some(EvidenceResult::Fail) => status
                .blockers
                .push("deterministic-verification-failed".into()),
            Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree) => status
                .blockers
                .push("deterministic-verification-inconclusive".into()),
            None => status
                .blockers
                .push("deterministic-verification-missing".into()),
        }
        let require_property = status.plan.require_property;
        let require_mutation = status.plan.require_mutation;
        let require_fuzz = status.plan.require_fuzz;
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Property,
            EvidenceKind::Property,
            require_property,
        );
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Mutation,
            EvidenceKind::Mutation,
            require_mutation,
        );
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Fuzz,
            EvidenceKind::Fuzz,
            require_fuzz,
        );
        let runtime_required = status
            .plan
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate");
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::RuntimeCanary,
            EvidenceKind::Runtime,
            runtime_required,
        );
        status.human_approval = evidence
            .iter()
            .filter(|record| {
                evidence_matches_plan_revision(record, &status.plan)
                    && record.kind == EvidenceKind::HumanApproval
            })
            .max_by_key(|record| record.timestamp_ms)
            .is_some_and(|record| record.result == EvidenceResult::Pass);
        if status.plan.require_human_approval && !status.human_approval {
            status.blockers.push("human-approval-required".into());
        }
        status.blockers.sort();
        status.blockers.dedup();
        status.ready = status.blockers.is_empty()
            && status.submitted == status.plan.job_ids.len()
            && status.deterministic_result == Some(EvidenceResult::Pass);
        Ok(status)
    }

    pub(crate) fn verification_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let mut plans = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.plans_for_workspace(workspace_id)
        };
        plans.reverse();
        let mut history = Vec::new();
        for plan in plans.into_iter().take(limit.clamp(1, 100)) {
            history.push(self.verification_status(workspace_id, workspace, &plan.id)?);
        }
        Ok(history)
    }

    pub(crate) fn record_verification_report(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        report: &VerificationReport,
    ) -> Result<Vec<Evidence>> {
        let revision = workspace_revision(workspace)?;
        let design = design::load_design(workspace)?;
        let mut produced = Vec::new();
        for check in &report.checks {
            let mut evidence = Evidence::new(
                self.next_id("EV"),
                format!("verification:{}", check.id),
                evidence_kind_for_check(&check.id),
                check.command.clone(),
                revision.clone(),
                if check.success {
                    EvidenceResult::Pass
                } else {
                    EvidenceResult::Fail
                },
                Confidence::Deterministic,
            )?;
            evidence.policy = Some(format!("deterministic/{}/v1", report.level));
            evidence.artifact_digest = Some(format!(
                "sha256:{}",
                digest_text(&format!(
                    "{}\n{:?}\n{}\n{}",
                    check.command, check.exit_code, check.stdout_tail, check.stderr_tail
                ))
            ));
            produced.push(evidence);
        }
        let mut aggregate = Evidence::new(
            self.next_id("EV"),
            format!("change:{}", revision.code),
            EvidenceKind::Verification,
            "verify_project".into(),
            revision.clone(),
            if report.passed {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
            Confidence::Deterministic,
        )?;
        aggregate.policy = Some(format!("deterministic/{}/v1", report.level));
        aggregate.artifact_digest = Some(format!(
            "sha256:{}",
            digest_text(&format!(
                "{}\n{}\n{}\n{}",
                report.level, report.checks_run, report.checks_failed, report.summary
            ))
        ));
        produced.push(aggregate);
        for criterion in design.state.acceptance.values() {
            let outcomes = criterion
                .verification
                .iter()
                .filter_map(|reference| verification_reference_outcome(reference, report))
                .collect::<Vec<_>>();
            if outcomes.is_empty() {
                continue;
            }
            let passed = outcomes.iter().all(|outcome| *outcome);
            let mut evidence = Evidence::new(
                self.next_id("EV"),
                criterion.id.clone(),
                EvidenceKind::IntegrationTest,
                "deterministic-verification-mesh".into(),
                revision.clone(),
                if passed {
                    EvidenceResult::Pass
                } else {
                    EvidenceResult::Fail
                },
                Confidence::Deterministic,
            )?;
            evidence.policy = Some(format!("acceptance/{}/v1", report.level));
            produced.push(evidence);
        }
        for evidence in &produced {
            evidence_store::persist(workspace, evidence)?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        for evidence in &produced {
            push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        }
        Ok(produced)
    }
}
