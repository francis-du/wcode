use super::*;

impl ToolHarness {
    pub fn cached_project_observatory(
        &self,
        workspace: &Workspace,
    ) -> Option<crate::intelligence_types::ProjectObservatory> {
        let root = workspace.root().to_path_buf();
        let mut cache = self.observatory_cache.lock().ok()?;
        let cached = cache.get_mut(&root)?;
        cached.last_used = Instant::now();
        Some(cached.snapshot.as_ref().clone())
    }

    fn cache_project_observatory(
        &self,
        workspace: &Workspace,
        snapshot: &crate::intelligence_types::ProjectObservatory,
    ) {
        let Ok(mut cache) = self.observatory_cache.lock() else {
            return;
        };
        let root = workspace.root().to_path_buf();
        let limit = crate::resource::limits().project_cache_limit();
        if cache.len() >= limit && !cache.contains_key(&root) {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(path, _)| path.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedProjectObservatory {
                last_used: Instant::now(),
                snapshot: Arc::new(snapshot.clone()),
            },
        );
    }

    pub fn project_observatory(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: Option<&ChangeReviewReport>,
    ) -> Result<ProjectObservatory> {
        const MAX_OBSERVATORY_SYMBOLS: usize = 5_000;
        const MAX_OBSERVATORY_HISTORY: usize = 32;

        let workspace_id = workspace_id.into();
        let (design, known_checks) = rayon::join(
            || self.intelligence.design_load(workspace),
            || self.known_checks(workspace),
        );
        let design = design?;
        let known_checks = known_checks?;
        let (traceability, graph) = rayon::join(
            || {
                self.intelligence.traceability_status_from_load(
                    workspace_id.clone(),
                    workspace,
                    &self.code_index,
                    &known_checks,
                    design.as_ref(),
                )
            },
            || {
                self.software_graph_from_design(
                    workspace_id.clone(),
                    workspace,
                    ".",
                    MAX_OBSERVATORY_FILES,
                    MAX_OBSERVATORY_SYMBOLS,
                    design.as_ref(),
                )
            },
        );
        let traceability = traceability?;
        let graph = graph?;
        let (
            (advanced, evidence),
            ((revision, reconciliation), (history, (verified_learning, engineering_journal))),
        ) = rayon::join(
            || {
                rayon::join(
                    || stage_executor::registry(workspace),
                    || self.intelligence.evidence_records(&workspace_id, workspace),
                )
            },
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || {
                                self.intelligence
                                    .current_revision_from_load(workspace, &design)
                            },
                            || self.reconciliation_history(workspace, 100),
                        )
                    },
                    || {
                        rayon::join(
                            || self.graph_history(workspace, MAX_OBSERVATORY_HISTORY),
                            || {
                                rayon::join(
                                    || observatory_verified_learning(workspace),
                                    || observatory_engineering_journal(workspace),
                                )
                            },
                        )
                    },
                )
            },
        );
        let advanced = advanced?;
        let evidence = evidence?;
        let revision = revision?;
        let reconciliation = reconciliation?;
        let history = history?;
        let (language_quality, review_analysis) = rayon::join(
            || {
                quality_provider::registry_from_advanced(
                    workspace,
                    Some(&self.semantic_sessions),
                    &advanced,
                )
            },
            || -> Result<_> {
                let risk = review
                    .map(|review| {
                        self.intelligence.risk_status_from_snapshot(
                            workspace_id.clone(),
                            workspace,
                            review,
                            traceability.clone(),
                            &design.state,
                            Some(&advanced),
                        )
                    })
                    .transpose()?;
                let impact = match (review, risk.as_ref()) {
                    (Some(review), Some(risk)) => {
                        Some(self.intelligence.impact_analysis_from_snapshot(
                            workspace_id.clone(),
                            workspace,
                            &self.code_index,
                            review,
                            &design.state,
                            risk.level,
                        )?)
                    }
                    _ => None,
                };
                let (verification_impact, adaptive_verification) = if let Some(review) =
                    review.filter(|review| !review.files.is_empty())
                {
                    let status_available = review
                        .probes
                        .iter()
                        .find(|probe| probe.id == "status")
                        .is_some_and(|probe| probe.success);
                    let snapshot = json!({
                        "available": status_available,
                        "truncated": review.truncated,
                        "files": review.files.iter().map(|file| json!({"path": file.path})).collect::<Vec<_>>(),
                    });
                    let (profile, _) = self.load_project_profile(workspace)?;
                    let raw_impact = harness_profile::verification_impact_for_snapshot(
                        &profile,
                        Some(&snapshot),
                    );
                    let adaptive = observatory_adaptive_verification(
                        self,
                        &workspace_id,
                        workspace,
                        &profile,
                        &snapshot,
                        &raw_impact,
                        &design,
                    );
                    (Some(observatory_verification_impact(raw_impact)), adaptive)
                } else {
                    let reason = if review.is_some() {
                        "no_current_changes"
                    } else {
                        "review_unavailable"
                    };
                    (None, static_adaptive_verification(reason))
                };
                Ok((risk, impact, verification_impact, adaptive_verification))
            },
        );
        let language_quality = language_quality?;
        let (risk, impact, verification_impact, adaptive_verification) = review_analysis?;
        let acceptance = acceptance_proof_summary(&design, &traceability, &evidence, &revision);
        let current_evidence = evidence
            .iter()
            .filter(|item| item.revision == revision)
            .collect::<Vec<_>>();
        let current_subject = format!("change:{}", revision.code);
        let verification = self.intelligence.verification_history_from_snapshot(
            &workspace_id,
            workspace,
            100,
            &revision,
            &evidence,
        )?;
        let current_verification = verification
            .iter()
            .filter(|status| {
                status.plan.subject == current_subject
                    && status.plan.revision.as_ref() == Some(&revision)
            })
            .collect::<Vec<_>>();
        let mut effective = crate::evidence::latest_current(&evidence, &revision);
        effective.sort_by_key(|item| {
            (
                std::cmp::Reverse(crate::evidence::result_severity(item.result)),
                std::cmp::Reverse(item.timestamp_ms),
            )
        });
        let sanitize = |text: &str| crate::workspace::redact_sensitive_text(text).0;
        let effective = crate::intelligence_types::ProjectEffectiveProofSummary {
            total: effective.len(),
            passed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Pass)
                .count(),
            failed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Fail)
                .count(),
            inconclusive: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Inconclusive)
                .count(),
            disagreed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Disagree)
                .count(),
            items: effective
                .iter()
                .take(24)
                .map(|item| crate::intelligence_types::ProjectEvidenceView {
                    subject: sanitize(&item.subject),
                    producer: sanitize(&item.producer),
                    policy: item.policy.as_deref().map(sanitize),
                    kind: item.kind,
                    confidence: item.confidence,
                    result: item.result,
                    timestamp_ms: item.timestamp_ms,
                    summary: item
                        .summary
                        .as_deref()
                        .map(|text| sanitize(text).chars().take(500).collect()),
                })
                .collect(),
            truncated: effective.len() > 24,
        };
        let proof = ProjectProofSummary {
            acceptance,
            effective,
            revision_code: revision.code.clone(),
            revision_design: revision.design.clone(),
            current_evidence: current_evidence.len(),
            current_passed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Pass)
                .count(),
            current_failed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Fail)
                .count(),
            current_inconclusive: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Inconclusive)
                .count(),
            current_disagreed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Disagree)
                .count(),
            current_verification_plans: current_verification.len(),
            current_verification_ready: current_verification
                .iter()
                .filter(|status| status.ready)
                .count(),
            current_verification_blocked: current_verification
                .iter()
                .filter(|status| !status.ready)
                .count(),
            latest_current_evidence_at_ms: current_evidence
                .iter()
                .map(|item| item.timestamp_ms)
                .max(),
            evidence_scan_truncated: evidence.len() >= 4_096,
        };
        let latest_reconciliation_plan = reconciliation.first().map(|plan| plan.id.clone());
        let graph_diff = if history.len() >= 2 {
            self.graph_diff(
                workspace,
                &GraphDiffInput {
                    from_snapshot_id: None,
                    to_snapshot_id: None,
                    limit: 200,
                },
            )
            .ok()
        } else {
            None
        };

        let snapshot = build_project_observatory(ObservatoryInput {
            workspace: workspace_id,
            root: workspace.root().display().to_string(),
            design: design.as_ref().clone(),
            traceability,
            graph: &graph,
            review,
            impact,
            verification_impact,
            risk,
            history: &history,
            graph_diff: graph_diff.as_ref(),
            language_quality,
            proof,
            adaptive_verification,
            verified_learning,
            engineering_journal,
            reconciliation_plans: reconciliation.len(),
            latest_reconciliation_plan,
        });
        self.cache_project_observatory(workspace, &snapshot);
        Ok(snapshot)
    }
}
