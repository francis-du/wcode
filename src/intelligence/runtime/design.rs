use super::*;

struct VerificationApprovalInput<'a> {
    approver: &'a str,
    statement: &'a str,
    authority: EvidenceAuthority,
    git_binding: Option<crate::verification::change::ExecutionGitBinding>,
}

impl SoftwareIntelligenceRuntime {
    pub(crate) fn current_revision(&self, workspace: &Workspace) -> Result<Revision> {
        let load = self.design_load(workspace)?;
        self.current_revision_from_load(workspace, load.as_ref())
    }

    pub(crate) fn current_revision_from_load(
        &self,
        workspace: &Workspace,
        load: &design::DesignLoad,
    ) -> Result<Revision> {
        workspace_revision_from_design_state(workspace, load.initialized)
    }

    pub fn design_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<DesignStatus> {
        let load = self.design_load(workspace)?;
        let errors = load.error_count();
        let warnings = load.warning_count();
        let state = &load.state;
        let valid = load.initialized && errors == 0;
        let mut operational_blockers = Vec::new();
        if load.initialized && state.product.is_none() {
            operational_blockers.push("missing_product".to_owned());
        }
        if load.initialized && state.components.is_empty() {
            operational_blockers.push("missing_components".to_owned());
        }
        if load.initialized
            && !state.components.is_empty()
            && !state
                .components
                .values()
                .any(|component| !component.implementation.is_empty())
        {
            operational_blockers.push("missing_implementation_ownership".to_owned());
        }
        let operational = valid && operational_blockers.is_empty();
        Ok(DesignStatus {
            workspace: workspace_id.into(),
            initialized: load.initialized,
            valid,
            operational,
            operational_blockers,
            schema_version: 1,
            design_root: load.design_root.clone(),
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
            diagnostics: load.diagnostics.clone(),
        })
    }

    pub(crate) fn traceability_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
    ) -> Result<TraceabilityStatus> {
        let (load, design_fingerprint) = self.design_load_with_fingerprint(workspace)?;
        self.traceability_status_from_load(
            workspace_id.into(),
            workspace,
            code_index,
            known_checks,
            load.as_ref(),
            design_fingerprint,
        )
    }

    pub(crate) fn traceability_status_from_load(
        &self,
        workspace_id: String,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        load: &design::DesignLoad,
        design_fingerprint: u64,
    ) -> Result<TraceabilityStatus> {
        let fingerprint = trace_cache::traceability_fingerprint(
            workspace,
            &load.state,
            design_fingerprint,
            known_checks,
        )?;
        if let Some(mut cached) = self.cached_traceability_status(workspace, fingerprint)? {
            cached.workspace = workspace_id;
            return Ok(cached);
        }
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
        let resolution_snapshot = TraceResolutionSnapshot::build(code_index, workspace, state);
        let mut component_resolutions = HashMap::<String, Vec<TraceReference>>::new();
        let mut acceptance_resolutions = HashMap::<String, Vec<TraceReference>>::new();

        for requirement in state.requirements.values() {
            let components_resolved = !requirement.implemented_by.is_empty()
                && requirement
                    .implemented_by
                    .iter()
                    .all(|id| state.components.contains_key(id));
            requirement_components_covered += usize::from(components_resolved);
            let mut implementation = Vec::new();
            for id in &requirement.implemented_by {
                let Some(component) = state.components.get(id) else {
                    continue;
                };
                let resolved = component_resolutions
                    .entry(component.id.clone())
                    .or_insert_with(|| {
                        resolution_snapshot
                            .code_references(&component.id, &component.implementation)
                    });
                implementation.extend(resolved.iter().cloned());
            }
            let mut verification = Vec::new();
            for id in &requirement.acceptance {
                let Some(criterion) = state.acceptance.get(id) else {
                    continue;
                };
                let resolved = acceptance_resolutions
                    .entry(criterion.id.clone())
                    .or_insert_with(|| {
                        resolution_snapshot.verification_references(
                            known_checks,
                            &criterion.id,
                            &criterion.verification,
                        )
                    });
                verification.extend(resolved.iter().cloned());
            }

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
        let status = TraceabilityStatus {
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
        };
        let confirmed_fingerprint = trace_cache::traceability_fingerprint(
            workspace,
            &load.state,
            design::fingerprint(workspace)?,
            known_checks,
        )?;
        if confirmed_fingerprint != fingerprint {
            return Err(anyhow!(
                "traceability inputs changed while resolving; retry the request"
            ));
        }
        self.cache_traceability_status(workspace, fingerprint, &status)?;
        Ok(status)
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
        let load = self.design_load(workspace)?;
        Ok(build_drift_status(
            workspace_id,
            &load.state,
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
        let load = self.design_load(workspace)?;
        let revision = self.current_revision_from_load(workspace, &load)?;
        let status = self.risk_status_from_snapshot(
            workspace_id,
            workspace,
            review,
            traceability,
            &load.state,
            None,
        )?;
        if status.revision != revision || self.current_revision(workspace)? != revision {
            return Err(anyhow!(
                "repository revision changed during risk assessment"
            ));
        }
        Ok(status)
    }

    pub(crate) fn risk_status_from_snapshot(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
        traceability: TraceabilityStatus,
        state: &design::DesignState,
        advanced: Option<&StageExecutorRegistry>,
    ) -> Result<RiskStatus> {
        let revision = self.current_revision(workspace)?;
        let workspace_id = workspace_id.into();
        let drift = build_drift_status(workspace_id.clone(), state, &traceability, review);
        let (level, mut risks) = assess_risk(&workspace_id, review, &traceability, &drift);
        let profile = VerificationProfile::for_risk(level);
        if !required_verification_stages(&profile).is_empty() {
            let fallback;
            let registry = match advanced {
                Some(registry) => registry,
                None => {
                    fallback = stage_executor::registry(workspace)?;
                    &fallback
                }
            };
            let stage_targets = verification_targets_for_review(review, registry, level);
            append_verification_automation_gap(
                &workspace_id,
                &profile,
                registry,
                &stage_targets,
                &mut risks,
            );
        }
        let bug_patterns = crate::risk::scan_bug_patterns(workspace)?;
        self.state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?
            .latest_risks
            .insert(workspace_id.clone(), risks.clone());
        Ok(RiskStatus {
            workspace: workspace_id,
            revision,
            level,
            profile,
            risks,
            bug_patterns,
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
        self.impact_analysis_from_risk(workspace_id, workspace, code_index, review, risk.level)
    }

    pub(crate) fn impact_analysis_from_risk(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        review: &ChangeReviewReport,
        risk_level: RiskLevel,
    ) -> Result<ImpactAnalysis> {
        let load = self.design_load(workspace)?;
        self.impact_analysis_from_snapshot(
            workspace_id,
            workspace,
            code_index,
            review,
            &load.state,
            risk_level,
        )
    }

    pub(crate) fn impact_analysis_from_snapshot(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        review: &ChangeReviewReport,
        state: &design::DesignState,
        risk_level: RiskLevel,
    ) -> Result<ImpactAnalysis> {
        self.impact_analysis_with_graph_from_snapshot(
            workspace_id,
            workspace,
            code_index,
            review,
            state,
            risk_level,
        )
        .map(|(impact, _)| impact)
    }

    /// Reuse the exact bounded Graph used by native impact analysis.
    pub(crate) fn impact_analysis_with_graph_from_snapshot(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        review: &ChangeReviewReport,
        state: &design::DesignState,
        risk_level: RiskLevel,
    ) -> Result<(ImpactAnalysis, SoftwareGraphSnapshot)> {
        let workspace_id = workspace_id.into();
        let mut graph = code_index.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_IMPACT_GRAPH_FILES,
            MAX_IMPACT_GRAPH_SYMBOLS,
        )?;
        graph_provider_store::overlay_latest(workspace, &mut graph)?;
        let impact = build_impact_analysis(workspace_id, state, review, risk_level, Some(&graph));
        Ok((impact, graph))
    }

    pub(crate) fn create_verification_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
        check_plans: &VerificationCheckPlans,
    ) -> Result<VerificationPlan> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let verification_risk = crate::execution::verification_risk_floor(workspace, risk.level)?;
        let registry = stage_executor::registry(workspace)?;
        let stage_targets = verification_targets_for_review(review, &registry, verification_risk);
        self.create_plan_for_risk_with_targets(
            &workspace_id,
            workspace,
            verification_risk,
            stage_targets,
            &registry,
            Some(
                if verification_risk >= RiskLevel::Medium {
                    &check_plans.full
                } else {
                    &check_plans.quick
                }
                .clone(),
            ),
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
        let revision = self.current_revision(workspace)?;
        let capabilities = capabilities.iter().cloned().collect::<BTreeSet<_>>();
        let (job, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state.verification.claim_for_revision(
                workspace_id,
                reviewer,
                &capabilities,
                role,
                &revision,
            )?;
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
        let revision = self.current_revision(workspace)?;
        let (job, produced, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state
                .verification
                .submit(workspace_id, job_id, reviewer, submission)?;
            let status = state.verification.status(&job.plan_id)?;
            let mut evidence = VerificationState::evidence_for_submission(
                &job,
                self.next_id("EV"),
                revision.clone(),
                status.plan.policy.clone(),
            )?;
            evidence.authority = EvidenceAuthority::SelfReported;
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
                    disagreement.authority = EvidenceAuthority::SelfReported;
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
        self.record_stage_submission(
            workspace_id,
            workspace,
            plan_id,
            submission,
            EvidenceAuthority::SelfReported,
            None,
        )
    }

    // Only native stage execution calls this entry point; it is not a tool input.
    #[cfg(test)]
    pub(crate) fn verification_stage_submit_native(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
    ) -> Result<Evidence> {
        self.record_stage_submission(
            workspace_id,
            workspace,
            plan_id,
            submission,
            EvidenceAuthority::NativeStage,
            None,
        )
    }

    pub(crate) fn verification_stage_submit_native_bound(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
        binding: Option<crate::verification::change::ExecutionGitBinding>,
    ) -> Result<Evidence> {
        self.record_stage_submission(
            workspace_id,
            workspace,
            plan_id,
            submission,
            EvidenceAuthority::NativeStage,
            binding,
        )
    }

    fn record_stage_submission(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
        authority: EvidenceAuthority,
        git_binding: Option<crate::verification::change::ExecutionGitBinding>,
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
        let revision = plan.revision.clone().ok_or_else(|| {
            anyhow!("verification plan has no bound revision; create a current plan")
        })?;
        if revision.code.ends_with(":partial") || self.current_revision(workspace)? != revision {
            return Err(anyhow!(
                "stale verification plan cannot authorize current evidence"
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
            revision.clone(),
            result,
            if authority == EvidenceAuthority::SelfReported {
                Confidence::Low
            } else {
                Confidence::High
            },
        )?;
        evidence.authority = authority;
        evidence.execution_git_binding = git_binding;
        evidence.execution_policy_binding = Some(plan.policy.clone());
        evidence.model = submission.model.clone();
        evidence.policy =
            Some(format!("{}/stage/{:?}", plan.policy, submission.stage).to_ascii_lowercase());
        evidence.artifact_digest = Some(submission.artifact_digest.clone());
        evidence.summary = Some(submission.summary.clone());
        evidence.targets = submission.targets.clone();
        evidence.validate()?;
        if self.current_revision(workspace)? != revision {
            return Err(anyhow!(
                "revision changed before stage evidence persistence"
            ));
        }
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        Ok(evidence)
    }

    #[cfg(test)]
    pub(crate) fn verification_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Evidence> {
        self.record_verification_approval(
            workspace_id,
            workspace,
            plan_id,
            VerificationApprovalInput {
                approver,
                statement,
                authority: EvidenceAuthority::LegacyUnknown,
                git_binding: None,
            },
        )
    }

    // Called only after the local operator's exact one-shot grant is consumed.
    #[cfg(test)]
    pub(crate) fn verification_approve_authorized(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Evidence> {
        self.record_verification_approval(
            workspace_id,
            workspace,
            plan_id,
            VerificationApprovalInput {
                approver,
                statement,
                authority: EvidenceAuthority::LocalOperator,
                git_binding: None,
            },
        )
    }

    pub(crate) fn verification_approve_authorized_bound(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
        binding: Option<crate::verification::change::ExecutionGitBinding>,
    ) -> Result<Evidence> {
        self.record_verification_approval(
            workspace_id,
            workspace,
            plan_id,
            VerificationApprovalInput {
                approver,
                statement,
                authority: EvidenceAuthority::LocalOperator,
                git_binding: binding,
            },
        )
    }

    fn record_verification_approval(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approval: VerificationApprovalInput<'_>,
    ) -> Result<Evidence> {
        let VerificationApprovalInput {
            approver,
            statement,
            authority,
            git_binding,
        } = approval;
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
        let revision = plan.revision.clone().ok_or_else(|| {
            anyhow!("verification plan has no bound revision; create a current plan")
        })?;
        if revision.code.ends_with(":partial") || self.current_revision(workspace)? != revision {
            return Err(anyhow!(
                "stale verification plan cannot authorize current evidence"
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
            revision.clone(),
            EvidenceResult::Pass,
            Confidence::High,
        )?;
        evidence.authority = authority;
        evidence.execution_git_binding = git_binding;
        evidence.execution_policy_binding = Some(plan.policy.clone());
        evidence.policy = Some(verification_snapshot::human_approval_policy(&plan)?);
        evidence.claims = vec![format!("approved-plan:{}", plan.id)];
        evidence.artifact_digest = Some(format!("sha256:{}", digest_text(statement)));
        evidence.summary = Some(statement.to_owned());
        evidence.validate()?;
        if self.current_revision(workspace)? != revision {
            return Err(anyhow!(
                "revision changed before human approval persistence"
            ));
        }
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
        let status = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.status(plan_id)?
        };
        if status.plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        let revision = self.current_revision(workspace)?;
        let evidence = self.evidence_records(workspace_id, workspace)?;
        Self::verification_status_from_snapshot(status, &revision, &evidence)
    }

    pub(super) fn verification_base_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        let mut plans = state.verification.plans_for_workspace(workspace_id);
        plans.reverse();
        plans
            .into_iter()
            .take(limit.clamp(1, 100))
            .map(|plan| state.verification.status(&plan.id).map_err(Into::into))
            .collect()
    }

    pub(crate) fn verification_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        let history = self.verification_base_history(workspace_id, workspace, limit)?;
        if history.is_empty() {
            return Ok(history);
        }
        // Capture once per request, not once per plan. Do not cache these inputs
        // across requests: edits and freshly persisted evidence must be visible.
        let revision = self.current_revision(workspace)?;
        let evidence = self.evidence_records(workspace_id, workspace)?;
        history
            .into_iter()
            .map(|status| Self::verification_status_from_snapshot(status, &revision, &evidence))
            .collect()
    }

    pub(crate) fn verification_history_from_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
        revision: &Revision,
        evidence: &[Evidence],
    ) -> Result<Vec<VerificationStatus>> {
        self.verification_base_history(workspace_id, workspace, limit)?
            .into_iter()
            .map(|status| Self::verification_status_from_snapshot(status, revision, evidence))
            .collect()
    }
}
