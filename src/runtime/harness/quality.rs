use super::*;

impl ToolHarness {
    pub fn project_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<ProjectContext> {
        let (profile, cache_hit) = self.load_project_profile(workspace)?;
        let (conventions, language_quality) = rayon::join(
            || self.convention_status(workspace),
            || self.language_quality_status(workspace),
        );
        let conventions = conventions?;
        let language_quality = language_quality?;
        Ok(ProjectContext {
            workspace: workspace_id.into(),
            cache_hit,
            root: profile.root.clone(),
            project_types: profile.project_types.clone(),
            manifests: profile.manifests.clone(),
            guidance: profile.guidance.clone(),
            recommended_checks: profile.recommended_checks.clone(),
            workflow: profile.workflow.clone(),
            write_enabled: profile.write_enabled,
            exec_enabled: profile.exec_enabled,
            product_scopes: scopes::registry(),
            conventions,
            language_quality,
        })
    }

    pub async fn observatory_revision_signal(
        &self,
        workspace: &Workspace,
    ) -> Result<ObservatoryRevisionSignal> {
        if !workspace.exec_enabled() || !workspace.root().join(".git").is_dir() {
            return Ok(ObservatoryRevisionSignal {
                fingerprint: None,
                changed_files: 0,
                truncated: false,
                full_refresh_required: true,
            });
        }

        let status_args = ["status", "--short", "--untracked-files=all"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let head_args = ["rev-parse", "HEAD"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let status = workspace.run_command("git", &status_args, ".", 10).await?;
        let head = workspace.run_command("git", &head_args, ".", 10).await?;
        if !status.success {
            bail!(
                "git status failed while building observatory revision signal: {}",
                probe_failure_text(&status).unwrap_or_else(|| "unknown error".to_owned())
            );
        }
        if !head.success {
            bail!(
                "git rev-parse failed while building observatory revision signal: {}",
                probe_failure_text(&head).unwrap_or_else(|| "unknown error".to_owned())
            );
        }

        let (changed, parsed_truncated) = parse_git_status(&status.stdout);
        let truncated = parsed_truncated || status.truncated || head.truncated;
        let mut hasher = Sha256::new();
        hasher.update(head.stdout.trim().as_bytes());
        hasher.update([0]);
        hasher.update(status.stdout.as_bytes());
        for path in changed.keys() {
            hasher.update([0]);
            hasher.update(path.as_bytes());
            if let Ok((len, modified_nanos)) = workspace.source_metadata_stamp(path) {
                hasher.update(len.to_le_bytes());
                hasher.update(modified_nanos.to_le_bytes());
            }
        }
        Ok(ObservatoryRevisionSignal {
            fingerprint: Some(format!("{:x}", hasher.finalize())),
            changed_files: changed.len(),
            truncated,
            full_refresh_required: truncated,
        })
    }

    pub async fn review_changes(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<ChangeReviewReport> {
        if !workspace.exec_enabled() {
            bail!("change review requires command execution; restart without --no-exec");
        }
        if !workspace.root().join(".git").exists() {
            bail!(
                "change review requires the configured workspace root to be a Git repository root"
            );
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut tasks = JoinSet::new();
        for spec in review_probe_specs() {
            let harness = self.clone();
            let monitor = monitor.clone();
            let workspace = workspace.clone();
            let workspace_id = workspace_id.clone();
            tasks.spawn(async move {
                run_review_probe(
                    harness,
                    monitor,
                    workspace_id,
                    workspace,
                    spec,
                    timeout_seconds,
                )
                .await
            });
        }

        let mut outputs = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            outputs.push(match joined {
                Ok(output) => output,
                Err(error) => ReviewProbeOutput {
                    id: "internal-join-error".to_owned(),
                    result: None,
                    elapsed_ms: 0,
                    error: Some(error.to_string()),
                },
            });
        }
        outputs.sort_by(|left, right| left.id.cmp(&right.id));

        let status = outputs
            .iter()
            .find(|output| output.id == "status")
            .ok_or_else(|| anyhow::anyhow!("change review did not receive Git status output"))?;
        let status_result = status.result.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "git status failed: {}",
                status
                    .error
                    .as_deref()
                    .unwrap_or("the probe returned no result")
            )
        })?;
        if !status_result.success {
            bail!(
                "git status failed: {}",
                probe_failure_text(status_result).unwrap_or_else(|| "unknown error".to_owned())
            );
        }

        let (mut changed, mut truncated) = parse_git_status(&status_result.stdout);
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(result) = outputs
                .iter()
                .find(|output| output.id == probe_id)
                .and_then(|output| output.result.as_ref())
            {
                if result.success {
                    truncated |= merge_numstat(&mut changed, &result.stdout);
                }
            }
        }

        let mut findings = Vec::new();
        for probe_id in ["unstaged-check", "staged-check"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                append_diff_check_findings(&mut findings, output);
            }
        }
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                if output.result.as_ref().is_some_and(|result| !result.success)
                    || output.error.is_some()
                {
                    findings.push(ReviewFinding {
                        severity: "warning".to_owned(),
                        code: "incomplete-diff-metrics".to_owned(),
                        message: format!(
                            "The {probe_id} probe failed; line counts may be incomplete."
                        ),
                        paths: Vec::new(),
                    });
                }
            }
        }

        let mut files = Vec::with_capacity(changed.len());
        let mut security_paths = Vec::new();
        let mut manifest_paths = Vec::new();
        let mut deleted_test_paths = Vec::new();
        let mut source_changed = false;
        let mut tests_changed = false;
        let mut docs_only = !changed.is_empty();
        let mut additions = 0u64;
        let mut deletions = 0u64;
        let mut binary_files = 0usize;

        for (path, change) in changed {
            let category = file_category(&path).to_owned();
            source_changed |= category == "source";
            tests_changed |= category == "test";
            docs_only &= category == "docs";
            additions = additions.saturating_add(change.additions);
            deletions = deletions.saturating_add(change.deletions);
            binary_files += usize::from(change.binary);

            let mut risk_reasons = Vec::new();
            if security_sensitive_path(&path) {
                risk_reasons.push("security-sensitive path".to_owned());
                security_paths.push(path.clone());
            }
            if category == "manifest" {
                risk_reasons.push("dependency or build metadata".to_owned());
                manifest_paths.push(path.clone());
            }
            if category == "migration" {
                risk_reasons.push("data migration".to_owned());
            }
            if category == "workflow" {
                risk_reasons.push("automation or release workflow".to_owned());
            }
            if change.status == "deleted" {
                risk_reasons.push("deleted file".to_owned());
                if category == "test" {
                    deleted_test_paths.push(path.clone());
                }
            }

            files.push(ChangedFileReview {
                path,
                status: change.status,
                staged: change.staged,
                unstaged: change.unstaged,
                untracked: change.untracked,
                category,
                additions: change.has_numstat.then_some(change.additions),
                deletions: change.has_numstat.then_some(change.deletions),
                binary: change.binary,
                risk_reasons,
            });
        }

        let staged_files = files.iter().filter(|file| file.staged).count();
        let unstaged_files = files.iter().filter(|file| file.unstaged).count();
        let untracked_files = files.iter().filter(|file| file.untracked).count();
        let files_changed = files.len();
        let total_lines = additions.saturating_add(deletions);
        append_maintainability_findings(workspace, &files, &mut findings);

        if source_changed && !tests_changed {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "source-without-test-change".to_owned(),
                message: "Source files changed without a corresponding test-file change; confirm existing coverage or add a focused regression test."
                    .to_owned(),
                paths: files
                    .iter()
                    .filter(|file| file.category == "source")
                    .take(8)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if !security_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "security-sensitive-change".to_owned(),
                message: "Authentication, authorization, token, crypto, or security-related files changed; review trust boundaries and failure paths explicitly."
                    .to_owned(),
                paths: security_paths.clone(),
            });
        }
        if !manifest_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "manifest-change".to_owned(),
                message: "Dependency or build metadata changed; verify lockfiles and perform a full project check."
                    .to_owned(),
                paths: manifest_paths.clone(),
            });
        }
        if !deleted_test_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "deleted-tests".to_owned(),
                message: "Test files were deleted; confirm coverage was intentionally relocated or removed."
                    .to_owned(),
                paths: deleted_test_paths,
            });
        }
        if files_changed > 25 || total_lines > 1_000 {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "large-change-set".to_owned(),
                message: format!(
                    "The change set spans {files_changed} files and approximately {total_lines} changed lines; consider splitting independent concerns."
                ),
                paths: Vec::new(),
            });
        }
        if untracked_files > 0 {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "untracked-files".to_owned(),
                message: format!(
                    "{untracked_files} untracked file(s) are part of the working tree review."
                ),
                paths: files
                    .iter()
                    .filter(|file| file.untracked)
                    .take(12)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if docs_only {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "docs-only".to_owned(),
                message: "Only documentation files changed; a quick verification gate is normally sufficient."
                    .to_owned(),
                paths: Vec::new(),
            });
        }
        if truncated {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "review-truncated".to_owned(),
                message: format!(
                    "The review reached its {MAX_REVIEW_FILES}-file bound; inspect the remaining change set separately."
                ),
                paths: Vec::new(),
            });
        }
        findings.truncate(MAX_REVIEW_FINDINGS);

        let high_risk = !security_paths.is_empty()
            || files.iter().any(|file| file.category == "migration")
            || findings.iter().any(|finding| finding.severity == "high")
            || files_changed > 50
            || total_lines > 2_000;
        let moderate_risk = source_changed
            || tests_changed
            || !manifest_paths.is_empty()
            || files.iter().any(|file| file.category == "workflow")
            || files_changed > 10
            || findings
                .iter()
                .any(|finding| matches!(finding.severity.as_str(), "warning" | "error"));
        let risk_level = if high_risk {
            "high"
        } else if moderate_risk {
            "moderate"
        } else {
            "low"
        };
        let recommended_verification = if high_risk
            || tests_changed
            || !manifest_paths.is_empty()
            || total_lines > 500
            || files_changed > 10
        {
            "full"
        } else {
            "quick"
        };
        let recommended_checks = profile
            .recommended_checks
            .iter()
            .filter(|check| recommended_verification == "full" || check.level == "quick")
            .map(|check| check.id.clone())
            .collect::<Vec<_>>();
        let clean = files_changed == 0;
        let summary = if clean {
            "No staged, unstaged, or untracked files were detected.".to_owned()
        } else {
            format!(
                "Reviewed {files_changed} changed file(s): {staged_files} staged, {unstaged_files} unstaged, {untracked_files} untracked; risk {risk_level}, recommend {recommended_verification} verification."
            )
        };
        let probes = outputs.iter().map(review_probe_summary).collect::<Vec<_>>();

        Ok(ChangeReviewReport {
            workspace: workspace_id,
            execution: "parallel-git-probes".to_owned(),
            clean,
            files_changed,
            staged_files,
            unstaged_files,
            untracked_files,
            additions,
            deletions,
            binary_files,
            source_changed,
            tests_changed,
            docs_only,
            risk_level: risk_level.to_owned(),
            recommended_verification: recommended_verification.to_owned(),
            recommended_checks,
            summary,
            files,
            findings,
            probes,
            truncated,
        })
    }

    pub async fn verify_project(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        level: &str,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<VerificationReport> {
        if !workspace.exec_enabled() {
            bail!("project verification requires command execution; restart without --no-exec");
        }
        if !matches!(level, "quick" | "full") {
            bail!("verification level must be quick or full");
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut plan = profile
            .recommended_checks
            .iter()
            .filter(|check| level == "full" || check.level == "quick")
            .take(MAX_VERIFICATION_CHECKS)
            .cloned()
            .collect::<Vec<_>>();
        sort_checks(&mut plan);

        let phases_run = plan
            .iter()
            .map(|check| check.phase)
            .collect::<HashSet<_>>()
            .len();
        let started = Instant::now();
        let mut checks = Vec::with_capacity(plan.len());
        let mut start = 0usize;

        while start < plan.len() {
            let phase = plan[start].phase;
            let end = plan[start..]
                .iter()
                .position(|check| check.phase != phase)
                .map(|offset| start + offset)
                .unwrap_or(plan.len());
            let mut tasks = JoinSet::new();
            for check in plan[start..end].iter().cloned() {
                let harness = self.clone();
                let monitor = monitor.clone();
                let workspace = workspace.clone();
                let workspace_id = workspace_id.clone();
                tasks.spawn(async move {
                    run_verification_check(
                        harness,
                        monitor,
                        workspace_id,
                        workspace,
                        check,
                        timeout_seconds,
                    )
                    .await
                });
            }
            while let Some(joined) = tasks.join_next().await {
                checks.push(match joined {
                    Ok(check) => check,
                    Err(error) => VerificationCheck {
                        id: "internal-join-error".to_owned(),
                        phase,
                        command: "verification task".to_owned(),
                        reason: "A verification worker failed before returning its result."
                            .to_owned(),
                        success: false,
                        exit_code: None,
                        elapsed_ms: 0,
                        stdout_tail: String::new(),
                        stderr_tail: error.to_string(),
                        output_truncated: false,
                    },
                });
            }
            start = end;
        }

        checks.sort_by(|left, right| {
            left.phase
                .cmp(&right.phase)
                .then_with(|| left.id.cmp(&right.id))
        });
        let checks_failed = checks.iter().filter(|check| !check.success).count();
        let checks_run = checks.len();
        let passed = checks_run > 0 && checks_failed == 0;
        let summary = if checks_run == 0 {
            "No verification commands could be inferred for this project; inspect its guidance and manifests manually."
                .to_owned()
        } else if passed {
            format!(
                "All {checks_run} inferred {level} checks passed across {phases_run} execution phase(s)."
            )
        } else {
            format!(
                "{checks_failed} of {checks_run} inferred {level} checks failed across {phases_run} execution phase(s)."
            )
        };

        let report = VerificationReport {
            workspace: workspace_id.clone(),
            level: level.to_owned(),
            execution: "phased-parallel".to_owned(),
            phases_run,
            passed,
            checks_run,
            checks_failed,
            elapsed_ms: started.elapsed().as_millis(),
            summary,
            checks,
        };
        self.intelligence
            .record_verification_report(&workspace_id, workspace, &report)?;
        Ok(report)
    }
}
