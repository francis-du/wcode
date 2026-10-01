use super::*;

impl ToolHarness {
    pub async fn verify_project<T: TaskTelemetry>(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        level: &str,
        timeout_seconds: u64,
        monitor: &T,
    ) -> Result<VerificationReport> {
        self.verify_project_mode(
            workspace_id,
            workspace,
            (level, true),
            timeout_seconds,
            monitor,
        )
        .await
    }

    pub(crate) async fn verify_project_mode<T: TaskTelemetry>(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        mode: (&str, bool),
        timeout_seconds: u64,
        monitor: &T,
    ) -> Result<VerificationReport> {
        self.verify_project_candidate(
            workspace_id,
            workspace,
            mode,
            timeout_seconds,
            monitor,
            None,
        )
        .await
    }

    pub(crate) async fn verify_project_candidate<T: TaskTelemetry>(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        mode: (&str, bool),
        timeout_seconds: u64,
        monitor: &T,
        candidate: Option<(&str, crate::verification::change::GitChangeTarget)>,
    ) -> Result<VerificationReport> {
        let (level, fail_fast) = mode;
        if !workspace.exec_enabled() {
            bail!("project verification requires command execution; restart without --no-exec");
        }
        if !matches!(level, "quick" | "full") {
            bail!("verification level must be quick or full");
        }

        let workspace_id = workspace_id.into();
        let design = self.intelligence.design_load(workspace)?;
        let revision = self
            .intelligence
            .current_revision_from_load(workspace, design.as_ref())?;
        let execution_git_binding = self.execution_git_binding(workspace).await?;
        let policy_authority =
            harness_policy_select::policy_authority_fingerprint(workspace, &workspace_id)?;
        let reuse_context = harness_verification_cache::VerificationReuseContext::new(
            workspace,
            &revision,
            level,
            fail_fast,
            timeout_seconds,
        )
        .with_git_binding(execution_git_binding.clone())
        .with_policy_authority(format!("{policy_authority}:candidate={candidate:?}"));
        let command_revision_key = (!revision.code.ends_with(":partial")
            && !revision
                .design
                .as_deref()
                .is_some_and(|value| value.ends_with(":partial")))
        .then(|| {
            format!(
                "code={};design={};git={}",
                revision.code,
                revision.design.as_deref().unwrap_or("none"),
                reuse_context.git_identity_key()
            )
        });
        let flight_started = Instant::now();
        let mut leader = match self.claim_verification_run(workspace, &reuse_context) {
            harness_verification_cache::VerificationRunClaim::Leader(leader) => leader,
            harness_verification_cache::VerificationRunClaim::Follower(flight) => {
                let mut report = flight.wait().await?;
                self.ensure_execution_git_binding(workspace, &execution_git_binding)
                    .await?;
                if self.intelligence.current_revision(workspace)? != revision {
                    bail!("verification revision changed while awaiting an in-flight run; results are stale");
                }
                harness_policy_select::ensure_policy_authority(
                    workspace,
                    &workspace_id,
                    &policy_authority,
                )?;
                report.elapsed_ms = flight_started.elapsed().as_millis();
                if !report.execution.contains("in-flight-coalesced") {
                    report.execution = format!("{}+in-flight-coalesced", report.execution);
                }
                report.summary = format!(
                    "Coalesced with an in-flight exact-revision verification; no duplicate checks were started. {}",
                    report.summary
                );
                return Ok(report);
            }
        };
        let result = async {
        let started = Instant::now();
        // Freeze the observable change set before checks run. Experience is
        // learned only if this exact revision later passes verification.
        let experience_snapshot = self.worktree_status_snapshot(workspace).await.ok();
        let (profile, _) = self.load_project_profile(workspace)?;
        let native_policy = if crate::verification::policy_store::load(workspace, &workspace_id)?.is_some() {
            let native_profile = harness_profile::capture_policy_profile(workspace)?;
            let (base, target) = candidate.clone().unwrap_or(("HEAD", crate::verification::change::GitChangeTarget::Worktree));
            let git = self.git_change_snapshot(workspace, base, target).await?;
            if !git.complete { bail!("Acceptance verification requires a complete current Git candidate"); }
            let review = harness_policy_select::candidate_review(&workspace_id, &git);
            self.select_native_policy(&workspace_id, workspace, &git, &review, &native_profile)?
        } else {
            None
        };
        let execution_policy_binding = native_policy.as_ref().map(|selected| selected.plan_binding()).transpose()?;
        let impact = harness_profile::verification_impact_for_snapshot(
            &profile,
            experience_snapshot.as_ref(),
        );
        let mut plan = harness_profile::verification_checks_for_impact(&profile, &impact, level);
        if let Some(selected) = &native_policy {
            if level == "quick" && (selected.selection.requirements.minimum_level == crate::design::PolicyLevel::Full
                || selected.required_checks.iter().any(|binding| profile.recommended_checks.iter()
                    .any(|check| check.id == binding.id && check.level == "full"))) {
                bail!("approved Acceptance Policy requires full verification; rerun verify_project with level full");
            }
            for required in &selected.required_checks {
                let check = profile.recommended_checks.iter()
                    .find(|check| verification_check_binding(check) == *required)
                    .context("approved Policy check is no longer available")?;
                if !plan.iter().any(|planned| verification_check_binding(planned) == *required) {
                    plan.push(check.clone());
                }
            }
            sort_checks(&mut plan);
        }
        if let Some(blocker) = harness_verification::discovery_completeness_check(&profile, level) {
            plan.push(blocker);
            sort_checks(&mut plan);
        }
        let required_checks = Some(plan.iter().map(verification_check_binding).collect::<Vec<_>>());
        let polyglot_gaps = harness_profile::verification_gaps_for_impact(&profile, &impact, level);
        if plan.len() > MAX_VERIFICATION_CHECKS {
            bail!(
                "verification plan contains {} checks, exceeding the {MAX_VERIFICATION_CHECKS}-check bound; no checks executed; select a narrower workspace",
                plan.len()
            );
        }

        let conventions = self.convention_status(workspace)?;
        if let Some(check) = core_policy_check(&conventions) {
            let summary = if conventions.truncated {
                format!(
                    "wcode core policy verification is incomplete: convention scanning was truncated with {} deterministic error(s) observed. Narrow the workspace or resolve the reported violations before verification can pass.",
                    conventions.errors
                )
            } else {
                format!(
                    "wcode core policy rejected the workspace with {} deterministic convention error(s). Resolve them before project checks run.",
                    conventions.errors
                )
            };
            let report = VerificationReport {
                execution_git_binding: execution_git_binding.clone(),
                required_checks: required_checks.clone(),
                workspace: workspace_id.clone(),
                level: level.to_owned(),
                execution: "core-policy".to_owned(),
                phases_run: 1,
                passed: false,
                checks_run: 1,
                checks_reused: 0,
                checks_failed: 1,
                skipped_checks: plan.iter().map(|check| check.id.clone()).collect(),
                elapsed_ms: started.elapsed().as_millis(),
                summary,
                impact: Some(impact.clone()),
                cost_model: None,
                checks: vec![check],
            };
            self.ensure_execution_git_binding(workspace, &execution_git_binding).await?;
            harness_policy_select::ensure_policy_authority(workspace, &workspace_id, &policy_authority)?;
            self.intelligence.record_verification_report_policy_bound(
                &workspace_id,
                workspace,
                &revision,
                Some(design.as_ref()),
                &report,
                execution_policy_binding.as_deref(),
            )?;
            return Ok(report);
        }

        let migration_audit = crate::migration_audit::audit(workspace, &revision)?;
        if let Some(audit) = migration_audit.as_ref().filter(|audit| !audit.passed) {
            let check = migration_audit_check(audit);
            let report = VerificationReport {
                execution_git_binding: execution_git_binding.clone(),
                required_checks: required_checks.clone(),
                workspace: workspace_id.clone(),
                level: level.to_owned(),
                execution: "migration-audit".to_owned(),
                phases_run: 1,
                passed: false,
                checks_run: 1,
                checks_reused: 0,
                checks_failed: 1,
                skipped_checks: plan.iter().map(|check| check.id.clone()).collect(),
                elapsed_ms: started.elapsed().as_millis(),
                summary: audit.summary.clone(),
                impact: Some(impact.clone()),
                cost_model: None,
                checks: vec![check],
            };
            self.ensure_execution_git_binding(workspace, &execution_git_binding).await?;
            harness_policy_select::ensure_policy_authority(workspace, &workspace_id, &policy_authority)?;
            self.intelligence.record_verification_report_policy_bound(
                &workspace_id,
                workspace,
                &revision,
                Some(design.as_ref()),
                &report,
                execution_policy_binding.as_deref(),
            )?;
            return Ok(report);
        }

        if !polyglot_gaps.is_empty() {
            let mut checks = migration_audit
                .as_ref()
                .map(migration_audit_check)
                .into_iter()
                .collect::<Vec<_>>();
            checks.extend(polyglot_gaps.iter().map(polyglot_verification_gap_check));
            let summary = format!(
                "{} polyglot project island(s) lack {level} deterministic verification coverage for: {}. Add a bounded toolchain gate, run a stronger supported level, or narrow the change before treating verification as complete.",
                polyglot_gaps.len(),
                polyglot_gaps
                    .iter()
                    .map(|gap| format!("{} [{}]", gap.root, gap.project_types.join(",")))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let report = VerificationReport {
                execution_git_binding: execution_git_binding.clone(),
                required_checks: required_checks.clone(),
                workspace: workspace_id.clone(),
                level: level.to_owned(),
                execution: if migration_audit.is_some() {
                    "migration-audit+polyglot-gap".to_owned()
                } else {
                    "polyglot-gap".to_owned()
                },
                phases_run: 1,
                passed: false,
                checks_run: checks.len(),
                checks_reused: 0,
                checks_failed: polyglot_gaps.len(),
                skipped_checks: plan.iter().map(|check| check.id.clone()).collect(),
                elapsed_ms: started.elapsed().as_millis(),
                summary,
                impact: Some(impact.clone()),
                cost_model: None,
                checks,
            };
            self.ensure_execution_git_binding(workspace, &execution_git_binding).await?;
            harness_policy_select::ensure_policy_authority(workspace, &workspace_id, &policy_authority)?;
            self.intelligence.record_verification_report_policy_bound(
                &workspace_id,
                workspace,
                &revision,
                Some(design.as_ref()),
                &report,
                execution_policy_binding.as_deref(),
            )?;
            return Ok(report);
        }

        if level == "quick" && plan.len() < MAX_VERIFICATION_CHECKS {
            if let Some(focused) = harness_test_focus::focused_quick_test(
                self,
                &workspace_id,
                workspace,
                &profile,
                experience_snapshot.as_ref(),
            ) {
                let duplicate = plan.iter().any(|check| {
                    check.cwd == focused.cwd
                        && check.program == focused.program
                        && check.args == focused.args
                });
                if !duplicate {
                    plan.push(focused);
                    sort_checks(&mut plan);
                }
            }
        }
        let (plan, cost_model) =
            harness_cost::apply_historical_cost_model(workspace, &plan, fail_fast);
        let mut phases_run = usize::from(migration_audit.is_some());
        let mut skipped_checks = Vec::new();
        let mut checks = Vec::with_capacity(plan.len() + usize::from(migration_audit.is_some()));
        if let Some(audit) = migration_audit.as_ref() {
            checks.push(migration_audit_check(audit));
        }
        let mut start = 0usize;
        // One bounded persisted snapshot for this run. Missing/unreadable proof
        // disables reuse; final receipt validation still checks sources again.
        let reuse_proof = crate::evidence_store::load(workspace).unwrap_or_default();

        while start < plan.len() {
            let phase = plan[start].phase;
            let end = plan[start..]
                .iter()
                .position(|check| check.phase != phase)
                .map(|offset| start + offset)
                .unwrap_or(plan.len());
            let mut tasks = JoinSet::new();
            let mut executed_phase = false;
            for check in plan[start..end].iter().cloned() {
                if let Some(reused) =
                    self.cached_verification_check(workspace, &reuse_context, &check, &reuse_proof)
                {
                    checks.push(reused);
                    continue;
                }
                executed_phase = true;
                let harness = self.clone();
                let monitor = (*monitor).clone();
                let workspace = workspace.clone();
                let workspace_id = workspace_id.clone();
                let revision_key = command_revision_key.clone();
                tasks.spawn(async move {
                    run_verification_check(
                        harness,
                        monitor,
                        workspace_id,
                        workspace,
                        check,
                        revision_key,
                        timeout_seconds,
                    )
                    .await
                });
            }
            while let Some(joined) = tasks.join_next().await {
                checks.push(match joined {
                    Ok(check) => check,
                    Err(error) => VerificationCheck {
                        id: format!("internal-join-error:{phase}:{}", checks.len()),
                        phase,
                        command: "verification task".to_owned(),
                        reason: "A verification worker failed before returning its result."
                            .to_owned(),
                        success: false,
                        reused: false,
                        execution: crate::evidence::VerificationCheckExecution::Unavailable,
                        exit_code: None,
                        elapsed_ms: 0,
                        queue_wait_ms: 0,
                        execution_ms: 0,
                        stdout_tail: String::new(),
                        stderr_tail: error.to_string(),
                        output_truncated: false,
                        signature: None,
                        evidence_id: None,
                    },
                });
            }
            phases_run += usize::from(executed_phase);
            start = end;
            // Finish the current independent phase, but do not pay for later
            // compilation/test/build phases after a known failed gate.
            if fail_fast && checks.iter().any(|check| !check.success) {
                skipped_checks.extend(plan[end..].iter().map(|check| check.id.clone()));
                break;
            }
        }

        checks.sort_by(|left, right| {
            left.phase
                .cmp(&right.phase)
                .then_with(|| left.id.cmp(&right.id))
        });
        let checks_failed = checks.iter().filter(|check| !check.success).count();
        let checks_run = checks.len();
        let checks_reused = checks.iter().filter(|check| check.reused).count();
        let checks_executed = checks_run.saturating_sub(checks_reused);
        let passed = checks_run > 0 && checks_failed == 0 && skipped_checks.is_empty();
        let summary = if checks_run == 0 {
            "No verification commands could be inferred for this project; inspect its guidance and manifests manually."
                .to_owned()
        } else if passed && checks_reused > 0 {
            format!(
                "All {checks_run} inferred {level} checks passed: {checks_executed} executed and {checks_reused} reused from exact-revision static evidence across {phases_run} execution phase(s)."
            )
        } else if passed {
            format!(
                "All {checks_run} inferred {level} checks passed across {phases_run} execution phase(s)."
            )
        } else {
            format!(
                "{checks_failed} of {checks_run} executed {level} checks failed across {phases_run} phase(s); {} later checks skipped. Fix failures before retrying; use fail_fast=false for exhaustive diagnostics.",
                skipped_checks.len()
            )
        };

        let report = VerificationReport {
                execution_git_binding: execution_git_binding.clone(),
            required_checks: Some(plan.iter().map(verification_check_binding).collect()),
            workspace: workspace_id.clone(),
            level: level.to_owned(),
            execution: {
                let base = match (migration_audit.is_some(), cost_model.is_some()) {
                    (true, true) => "migration-audit+adaptive-sentinel+phased-parallel",
                    (true, false) => "migration-audit+phased-parallel",
                    (false, true) => "adaptive-sentinel+phased-parallel",
                    (false, false) => "phased-parallel",
                };
                if checks_reused > 0 {
                    format!("{base}+exact-revision-static-reuse")
                } else {
                    base.to_owned()
                }
            },
            phases_run,
            passed,
            checks_run,
            checks_reused,
            checks_failed,
            skipped_checks,
            elapsed_ms: started.elapsed().as_millis(),
            summary,
            impact: Some(impact),
            cost_model,
            checks,
        };
        self.ensure_execution_git_binding(workspace, &execution_git_binding).await?;
        harness_policy_select::ensure_policy_authority(workspace, &workspace_id, &policy_authority)?;
        self.intelligence.record_verification_report_policy_bound(
            &workspace_id,
            workspace,
            &revision,
            Some(design.as_ref()),
            &report,
            execution_policy_binding.as_deref(),
        )?;
        self.cache_successful_verification_checks(workspace, &reuse_context, &plan, &report);
        if checks_executed > 0 {
            if let Some(paths) = verified_experience_paths(&report, experience_snapshot.as_ref()) {
                if let Err(error) = crate::experience_store::persist_verified_change(
                    workspace,
                    &revision,
                    &report.level,
                    &paths,
                ) {
                    // Historical retrieval is an optional optimization. A store
                    // problem must never turn deterministic verification into a
                    // false failure or weaken its fail-closed evidence contract.
                    tracing::warn!(%error, "verified retrieval experience was not persisted");
                }
            }
        }
        Ok(report)
        }
        .await;
        leader.complete(&result);
        result
    }
}
