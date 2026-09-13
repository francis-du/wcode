use super::*;

#[path = "quality/verification_run.rs"]
mod verification_run;

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
            islands: profile.islands.clone(),
            contracts: profile.contracts.clone(),
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

    pub(crate) fn verification_impact_summary(
        &self,
        workspace: &Workspace,
        snapshot: &Value,
    ) -> Result<Value> {
        let (profile, _) = self.load_project_profile(workspace)?;
        let impact = harness_profile::verification_impact_for_snapshot(&profile, Some(snapshot));
        Ok(compact_verification_impact(&impact))
    }

    pub async fn observatory_revision_signal(
        &self,
        workspace: &Workspace,
    ) -> Result<ObservatoryRevisionSignal> {
        if !workspace.exec_enabled() || !workspace.root().join(".git").exists() {
            let workspace = workspace.clone();
            return tokio::task::spawn_blocking(move || {
                let (files, truncated) =
                    workspace.source_files_background_with_stamps(".", MAX_OBSERVATORY_FILES)?;
                let mut hasher = Sha256::new();
                hasher.update(b"workspace-metadata-v1");
                for (path, (len, modified_nanos)) in files {
                    hasher.update([0]);
                    hasher.update(path.as_bytes());
                    hasher.update(len.to_le_bytes());
                    hasher.update(modified_nanos.to_le_bytes());
                }
                Ok(ObservatoryRevisionSignal {
                    fingerprint: Some(format!("{:x}", hasher.finalize())),
                    changed_files: 0,
                    truncated,
                    full_refresh_required: truncated,
                })
            })
            .await
            .map_err(|error| {
                anyhow::anyhow!("observatory metadata signal worker failed: {error}")
            })?;
        }

        let status_args = ["status", "--short", "--untracked-files=all"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let head_args = ["rev-parse", "HEAD"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let (status, head) = tokio::join!(
            workspace.run_command("git", &status_args, ".", 10),
            workspace.run_command("git", &head_args, ".", 10),
        );
        let status = status?;
        let head = head?;
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
        let changed_paths = files
            .iter()
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>();
        for advisory in harness_profile::contract_freshness_advisories(
            workspace.root(),
            &profile.contracts,
            &changed_paths,
        ) {
            let mut paths = vec![
                advisory.source.clone(),
                advisory.config.clone(),
                advisory.output.clone(),
            ];
            paths.sort();
            paths.dedup();
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "generated-artifact-freshness".to_owned(),
                message: format!(
                    "A {} contract/config changed for `{}` while existing generated output `{}` has no working-tree change. Regenerate or run the generator's native freshness check when generated artifacts are tracked. This structural advisory is not proof that the output is stale ({}).",
                    advisory.kind, advisory.consumer_island, advisory.output, advisory.evidence
                ),
                paths,
            });
        }

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
}

pub(super) fn compact_verification_impact(impact: &ProjectVerificationImpact) -> Value {
    const MAX_COMPACT_IMPACT_REASONS: usize = 6;
    const MAX_COMPACT_IMPACT_TEXT: usize = 160;
    json!({
        "selective": impact.selective,
        "affected_islands": impact.affected_islands,
        "reasons": impact.reasons.iter().take(MAX_COMPACT_IMPACT_REASONS).map(|reason| json!({
            "island": reason.island,
            "kind": reason.kind,
            "source": truncate_chars(&reason.source, MAX_COMPACT_IMPACT_TEXT).0,
            "relationship": reason.relationship,
            "evidence": truncate_chars(&reason.evidence, MAX_COMPACT_IMPACT_TEXT).0,
            "provider": reason.provider,
            "precision": reason.precision,
        })).collect::<Vec<_>>(),
        "truncated": impact.truncated || impact.reasons.len() > MAX_COMPACT_IMPACT_REASONS,
        "provider": impact.provider,
        "precision": impact.precision,
    })
}

pub(super) fn core_policy_check(report: &ConventionReport) -> Option<VerificationCheck> {
    if report.errors == 0 && !report.truncated {
        return None;
    }
    let errors = report
        .findings
        .iter()
        .filter(|finding| finding.severity == crate::conventions::ConventionSeverity::Error)
        .take(32)
        .map(|finding| {
            json!({
                "code": finding.code,
                "path": finding.path,
                "language": finding.language,
                "message": finding.message,
            })
        })
        .collect::<Vec<_>>();
    let encoded = json!({
        "provider": report.provider,
        "errors": report.errors,
        "warnings": report.warnings,
        "truncated": report.truncated,
        "findings": errors,
    })
    .to_string();
    let (stdout_tail, cut) = truncate_chars(&encoded, MAX_CHECK_OUTPUT_CHARS);
    Some(VerificationCheck {
        id: "core-policy".to_owned(),
        phase: 0,
        command: "wcode internal core policy".to_owned(),
        reason: "Enforce deterministic wcode core constraints before repository-specific verification commands.".to_owned(),
        success: false,
        reused: false,
        exit_code: None,
        elapsed_ms: 0,
        queue_wait_ms: 0,
        execution_ms: 0,
        stdout_tail,
        stderr_tail: if report.truncated {
            "core policy scan was truncated; verification fails closed".to_owned()
        } else {
            "deterministic wcode core constraints are violated".to_owned()
        },
        output_truncated: cut || report.truncated || report.errors > 32,
    })
}

fn polyglot_verification_gap_check(
    gap: &harness_profile::ProjectIslandVerificationGap,
) -> VerificationCheck {
    VerificationCheck {
        id: format!("polyglot-gap:{}", gap.island),
        phase: 0,
        command: "wcode internal polyglot verification coverage".to_owned(),
        reason: format!(
            "Project island {} has manifest ownership but no {} deterministic gate for {}.",
            gap.root,
            gap.level,
            gap.project_types.join(", ")
        ),
        success: false,
        reused: false,
        exit_code: None,
        elapsed_ms: 0,
        queue_wait_ms: 0,
        execution_ms: 0,
        stdout_tail: json!({
            "provider":"manifest-discovery",
            "precision":"structural",
            "island":gap.island,
            "root":gap.root,
            "requested_level":gap.level,
            "verification_gaps":gap.project_types,
        })
        .to_string(),
        stderr_tail: "polyglot verification coverage is incomplete".to_owned(),
        output_truncated: false,
    }
}

fn migration_audit_check(
    audit: &crate::migration_audit::MigrationAuditReport,
) -> VerificationCheck {
    let encoded = serde_json::to_string(audit).unwrap_or_else(|error| {
        json!({
            "provider": "wcode-migration-audit",
            "serialization_error": error.to_string(),
        })
        .to_string()
    });
    let (stdout_tail, cut) = truncate_chars(&encoded, MAX_CHECK_OUTPUT_CHARS);
    VerificationCheck {
        id: "migration-audit".into(),
        phase: 0,
        command: "wcode internal migration audit".into(),
        reason: "Verify declarative migration completeness before behavioral checks.".into(),
        success: audit.passed,
        reused: false,
        exit_code: None,
        elapsed_ms: audit.elapsed_ms,
        queue_wait_ms: 0,
        execution_ms: audit.elapsed_ms,
        stdout_tail,
        stderr_tail: if audit.passed {
            String::new()
        } else {
            audit.summary.clone()
        },
        output_truncated: cut || audit.findings_truncated,
    }
}

pub(super) fn verified_experience_paths(
    report: &VerificationReport,
    snapshot: Option<&Value>,
) -> Option<Vec<String>> {
    if !report.passed {
        return None;
    }
    let snapshot = snapshot?;
    if snapshot.get("available").and_then(Value::as_bool) != Some(true)
        || snapshot.get("truncated").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let mut paths = snapshot
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    (paths.len() >= 2).then_some(paths)
}
