use super::*;

fn literal_symbol_queries(query: &str) -> Vec<String> {
    let code_literals = crate::intelligence::code_query_literals(query);
    if !code_literals.is_empty() {
        return code_literals.into_iter().take(4).collect();
    }
    query
        .split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | ':' | '.'))
        .map(|word| word.trim_matches([':', '.']))
        .filter(|word| !word.is_empty() && *word == query.trim())
        .take(1)
        .map(str::to_ascii_lowercase)
        .collect()
}

fn is_context_symbol_stopword(token: &str) -> bool {
    matches!(
        token,
        "find"
            | "where"
            | "which"
            | "show"
            | "please"
            | "check"
            | "inspect"
            | "understand"
            | "explain"
            | "code"
            | "function"
            | "method"
            | "class"
            | "file"
            | "module"
            | "behavior"
            | "logic"
            | "implementation"
            | "implement"
            | "change"
            | "modify"
            | "update"
            | "fix"
            | "issue"
            | "bug"
            | "performance"
            | "optimize"
            | "fast"
            | "faster"
            | "call"
            | "calls"
            | "caller"
            | "callee"
            | "reference"
            | "references"
            | "usage"
            | "impact"
    )
}

fn context_symbol_rank(
    symbol: &serde_json::Value,
    literals: &[String],
    queries: &[String],
    prefer_test_symbols: bool,
) -> (u8, usize, usize, usize) {
    let name = symbol["name"].as_str().unwrap_or("").to_ascii_lowercase();
    let qualified = symbol["qualified_name"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = symbol["path"].as_str().unwrap_or("").to_ascii_lowercase();
    let signature = symbol["signature"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let query_hits = queries
        .iter()
        .filter(|term| {
            name.contains(term.as_str())
                || qualified.contains(term.as_str())
                || signature.contains(term.as_str())
        })
        .count();
    let test_penalty = usize::from(!prefer_test_symbols && context_symbol_test_path(&path));
    let coverage_rank = usize::MAX.saturating_sub(query_hits);
    for (class, terms, exact) in [
        (0, literals, true),
        (1, queries, true),
        (2, literals, false),
        (3, queries, false),
    ] {
        if let Some(index) = terms.iter().position(|term| {
            if exact {
                name == *term || qualified == *term
            } else {
                name.contains(term.as_str())
                    || qualified.contains(term.as_str())
                    || signature.contains(term.as_str())
                    || path.contains(term.as_str())
            }
        }) {
            let class = if literals.is_empty() && matches!(class, 1 | 3) {
                1
            } else {
                class
            };
            return (class, test_penalty, coverage_rank, index);
        }
    }
    (4, test_penalty, coverage_rank, usize::MAX)
}

fn context_symbol_test_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.starts_with("tests/")
        || path.starts_with("test/")
        || path.starts_with("__tests__/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
}

fn context_query_prefers_tests(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "test"
                | "tests"
                | "testing"
                | "regression"
                | "verify"
                | "verification"
                | "测试"
                | "回归"
                | "验证"
        )
    })
}

impl SoftwareIntelligenceRuntime {
    pub(crate) fn software_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        request: &SoftwareContextRequest,
    ) -> Result<SoftwareContext> {
        self.software_context_with_symbols(
            workspace_id,
            workspace,
            code_index,
            known_checks,
            request,
            None,
        )
    }

    pub(crate) fn software_context_with_symbols(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        request: &SoftwareContextRequest,
        seeded_symbols: Option<&[serde_json::Value]>,
    ) -> Result<SoftwareContext> {
        let workspace_id = workspace_id.into();
        let query = request.query.trim();
        if query.is_empty() {
            return Err(anyhow!("software context query must not be empty"));
        }
        let (design_load, design_fingerprint) = self.design_load_with_fingerprint(workspace)?;
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
        let mut symbols = seeded_symbols.unwrap_or_default().to_vec();
        let mut symbol_ids = symbols
            .iter()
            .map(|symbol| {
                symbol
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| symbol.to_string())
            })
            .collect::<HashSet<_>>();
        let literals = literal_symbol_queries(query);
        let literal_parts = literals
            .iter()
            .flat_map(|literal| literal.split(['_', ':', '.']))
            .filter(|part| part.chars().count() >= 2)
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let mut symbol_queries = tokens
            .iter()
            .filter(|token| token.len() >= 3)
            .filter(|token| !is_context_symbol_stopword(token))
            .filter(|token| !literal_parts.contains(token.as_str()))
            // The final symbol query budget is capped at eight; keep shorter module/path terms such as `session` alive.
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        symbol_queries.splice(0..0, literals.iter().cloned());
        let mut seen_queries = HashSet::new();
        symbol_queries.retain(|term| seen_queries.insert(term.clone()));
        symbol_queries.truncate(8);
        if symbol_queries.is_empty() {
            symbol_queries.push(query.to_owned());
        }
        let cached_exact_hit =
            if seeded_symbols.is_none() && requested_scopes.is_empty() && !literals.is_empty() {
                let candidates =
                    code_index.cached_exact_symbols_many(workspace, &literals, None, symbol_cap)?;
                // A warm first target must not hide another requested target. Only
                // skip discovery when every literal has a revalidated exact hit;
                // partial seeds still participate in the normal deduplicated merge.
                let complete = literals.iter().all(|literal| {
                    candidates.iter().any(|symbol| {
                        ["name", "qualified_name"].iter().any(|field| {
                            symbol
                                .get(*field)
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|name| name.eq_ignore_ascii_case(literal))
                        })
                    })
                });
                for symbol in candidates {
                    let key = symbol
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| symbol.to_string());
                    if symbol_ids.insert(key) {
                        symbols.push(symbol);
                    }
                }
                // Exact-cache hits are sufficient only for a genuinely explicit
                // identifier lookup. Natural-language qualifiers such as a module,
                // path, lifecycle or domain term may have contributed additional
                // cold candidates; skipping discovery in that case makes warm
                // context strictly poorer than cold context.
                complete
                    && symbol_queries.iter().all(|query| {
                        literals
                            .iter()
                            .any(|literal| literal.eq_ignore_ascii_case(query))
                    })
            } else {
                false
            };
        let source_roots = if seeded_symbols.is_some() || cached_exact_hit {
            Vec::new()
        } else {
            scopes::source_roots_for(&requested_scopes)
        };
        let searches = crate::resource::parallel_io(&source_roots, |source_root| {
            let source_root = *source_root;
            match code_index.find_symbols_many(
                workspace_id.clone(),
                workspace,
                &symbol_queries,
                source_root,
                None,
                symbol_cap.saturating_mul(symbol_queries.len()).min(200),
            ) {
                Ok(search) => Ok(Some(search)),
                Err(error)
                    if source_root != "."
                        && error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    // Product Scope roots are optional in smaller repositories.
                    // Do not mistake a vanished/replaced Workspace for an absent scope.
                    workspace.path_info(".")?;
                    Ok(None)
                }
                Err(error) => Err(error),
            }
        })?;
        // parallel_io preserves input order, so merging remains deterministic
        // before the existing global rank/truncation step.
        for search in searches {
            let Some(search) = search? else {
                continue;
            };
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
                }
            }
        }
        // Merge bounded candidates from every scope before truncating: an
        // earlier directory's helpers must not evict a later exact definition.
        // Re-rank warm exact-cache candidates too. Cache insertion order is an
        // implementation detail and must not change Hot Source/body selection
        // relative to a cold discovery of the same unchanged revision.
        let prefer_test_symbols = context_query_prefers_tests(&tokens);
        symbols.sort_by_key(|symbol| {
            context_symbol_rank(symbol, &literals, &symbol_queries, prefer_test_symbols)
        });
        // Reserve one definition for each requested literal before duplicate
        // matches consume the bounded candidate slots. Prefer an implementation
        // over its same-named module declaration; module-only requests still
        // reserve the module. Stable ordering retains the existing test intent.
        let prefer_module = query.split_whitespace().any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "module" | "modules" | "namespace" | "模块"
            )
        });
        let mut representatives = HashSet::new();
        for literal in &literals {
            if let Some(symbol) = symbols
                .iter()
                .filter(|symbol| {
                    ["name", "qualified_name"].iter().any(|field| {
                        symbol[*field]
                            .as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(literal))
                    })
                })
                .min_by_key(|symbol| (symbol["kind"].as_str() == Some("module")) != prefer_module)
            {
                if let Some(id) = symbol["id"].as_str() {
                    representatives.insert(id.to_owned());
                }
            }
        }
        symbols.sort_by_key(|symbol| {
            !symbol["id"]
                .as_str()
                .is_some_and(|id| representatives.contains(id))
        });
        symbols.truncate(symbol_cap);
        // Provider overlays and Design traceability are independent after the
        // symbol candidates are fixed. Run them together so a cold graph-store
        // read does not serialize behind Tree-sitter resolution (or vice versa).
        let (graph_context, coverage) = rayon::join(
            || {
                provider_graph_context(
                    workspace,
                    &semantic_expansion,
                    &tokens,
                    &symbols,
                    item_cap.min(32),
                )
            },
            || {
                self.traceability_status_from_load(
                    workspace_id.clone(),
                    workspace,
                    code_index,
                    known_checks,
                    design_load.as_ref(),
                    design_fingerprint,
                )
            },
        );
        let graph_context = graph_context?;
        let mut coverage = coverage?;
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
        let design_load = self.design_load(workspace)?;
        let design_state = &design_load.state;
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
            design_state,
            review,
            risk.level,
            Some(&graph),
        );
        let verification_risk = crate::execution::verification_risk_floor(workspace, risk.level)?;
        let registry = stage_executor::registry(workspace)?;
        let stage_targets = verification_targets_for_review(review, &registry, verification_risk);
        let verification_plan = self.create_plan_for_risk_with_targets(
            &workspace_id,
            workspace,
            verification_risk,
            stage_targets,
            &registry,
        )?;
        let conventions = crate::conventions::status(workspace)?;
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
                DriftKind::RuntimeDrift => (
                    ReconciliationTaskKind::Verification,
                    ChangeIntent::AddVerification {
                        subject: finding.subject.clone(),
                        verification_kind: "runtime-capacity-regression".into(),
                    },
                ),
            };
            let write_scopes = if kind == ReconciliationTaskKind::Implementation {
                Workspace::normalize_relative_scope(&finding.subject)
                    .ok()
                    .filter(|scope| !scope.is_empty() && workspace.root().join(scope).exists())
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            };
            tasks.push(ReconciliationTask {
                id: task_id,
                kind,
                subject: finding.subject.clone(),
                description: finding.message.clone(),
                write_scopes,
                depends_on: Vec::new(),
            });
            intents.push(intent);
        }
        for finding in conventions
            .findings
            .iter()
            .filter(|finding| finding.severity == crate::conventions::ConventionSeverity::Error)
        {
            let constraints = if finding.code == "oversized-source-module" {
                vec!["CONSTRAINT-SOURCE-DECOMPOSITION".to_owned()]
            } else {
                Vec::new()
            };
            let write_scope = Workspace::normalize_relative_scope(&finding.path)?;
            tasks.push(ReconciliationTask {
                id: self.next_id("RT"),
                kind: ReconciliationTaskKind::Implementation,
                subject: finding.path.clone(),
                description: format!(
                    "Resolve hard repository convention `{}`: {}. Preserve behavior and public contracts; for oversized modules, split cohesive responsibilities before adding more behavior.",
                    finding.code, finding.message
                ),
                write_scopes: (!write_scope.is_empty()).then_some(write_scope).into_iter().collect(),
                depends_on: Vec::new(),
            });
            intents.push(ChangeIntent::ChangeBehavior {
                target: finding.path.clone(),
                desired: serde_json::json!({
                    "state": "conform_to_core_policy",
                    "policy": finding.code,
                }),
                constraints,
            });
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
            write_scopes: Vec::new(),
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
                write_scopes: Vec::new(),
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
            impacted_acceptance: impact.impacted_acceptance,
            implementation_tasks: tasks,
            change_intents: intents,
            verification_plan,
        };
        plan.validate()?;
        reconciliation_store::persist(workspace, &plan)?;
        reconciliation_execution_store::update_or_insert(workspace, &plan, |_| Ok(()))?;
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
        self.reconciliation_execution_status_from_plan(workspace_id, workspace, &plan)
    }

    pub(crate) fn reconciliation_execution_status_from_plan(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan: &ReconciliationPlan,
    ) -> Result<ReconciliationExecutionStatus> {
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let stored_execution = reconciliation_execution_store::load(workspace, &plan.id)?;
        let verification = self.reconciliation_verification_status(
            workspace_id,
            workspace,
            plan,
            stored_execution.as_ref(),
        )?;
        reconciliation_execution_status_from_inputs(
            workspace,
            plan,
            stored_execution,
            &verification,
        )
    }

    pub(super) fn reconciliation_execution_statuses_from_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plans: &[ReconciliationPlan],
        revision: &Revision,
        evidence: &[Evidence],
    ) -> Result<Vec<ReconciliationExecutionStatus>> {
        if plans.is_empty() {
            return Ok(Vec::new());
        }
        for plan in plans {
            if plan.workspace != workspace_id {
                return Err(anyhow!(
                    "reconciliation plan does not belong to the selected workspace"
                ));
            }
        }
        let plan_ids = plans.iter().map(|plan| plan.id.clone()).collect::<Vec<_>>();
        let verification_plan_ids = plans
            .iter()
            .map(|plan| plan.verification_plan.id.clone())
            .collect::<Vec<_>>();
        let (stored, verification) = rayon::join(
            || reconciliation_execution_store::load_many(workspace, &plan_ids),
            || {
                self.verification_statuses_from_snapshot(
                    workspace_id,
                    workspace,
                    &verification_plan_ids,
                    revision,
                    evidence,
                )
            },
        );
        let mut stored = stored?;
        let verification = verification?;
        plans
            .iter()
            .zip(verification.iter())
            .map(|(plan, verification)| {
                let stored_execution = stored.remove(&plan.id);
                reconciliation_execution_status_from_inputs(
                    workspace,
                    plan,
                    stored_execution,
                    verification,
                )
            })
            .collect()
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
        let snapshot = self.approved_reconciliation_snapshot(workspace_id, workspace, plan_id)?;
        let plan = &snapshot.plan;
        let run = reconciliation_execution_store::update_or_insert(workspace, plan, |execution| {
            Ok(execution.submit(task_id, executor, submission)?)
        })?;
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            format!("reconciliation-task:{}", run.task.id),
            EvidenceKind::Reconciliation,
            format!("executor:{executor}"),
            self.current_revision(workspace)?,
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
        let snapshot = self.approved_reconciliation_snapshot(workspace_id, workspace, plan_id)?;
        let plan = &snapshot.plan;
        reconciliation_execution_store::update_or_insert(workspace, plan, |execution| {
            Ok(execution.retry(task_id)?)
        })
    }
}

pub(super) fn reconciliation_intent_audit(
    workspace: &Workspace,
    intents: &[ChangeIntent],
) -> Result<(usize, Vec<String>)> {
    let auditable = intents
        .iter()
        .filter_map(|intent| match intent {
            ChangeIntent::ChangeBehavior {
                target, desired, ..
            } if desired["state"].as_str() == Some("conform_to_core_policy") => {
                Some((target.as_str(), desired["policy"].as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if auditable.is_empty() {
        return Ok((0, Vec::new()));
    }

    let report = crate::conventions::status(workspace)?;
    let mut blockers = Vec::new();
    for (target, policy) in &auditable {
        let Some(policy) = policy else {
            blockers.push(format!(
                "intent audit failed closed: {target} conform_to_core_policy is missing policy"
            ));
            continue;
        };
        if report.truncated {
            blockers.push(format!(
                "intent audit unavailable: Convention audit truncated before validating {target} against {policy}"
            ));
            continue;
        }
        if report.findings.iter().any(|finding| {
            finding.severity == crate::conventions::ConventionSeverity::Error
                && finding.path == *target
                && finding.code == *policy
        }) {
            blockers.push(format!(
                "intent not satisfied: {target} still violates {policy}"
            ));
        }
    }
    Ok((auditable.len(), blockers))
}

pub(super) fn reconciliation_execution_status_from_inputs(
    workspace: &Workspace,
    plan: &ReconciliationPlan,
    _stored_execution: Option<ReconciliationExecution>,
    verification: &VerificationStatus,
) -> Result<ReconciliationExecutionStatus> {
    let execution =
        reconciliation_execution_store::update_or_insert(workspace, plan, |execution| {
            execution.set_system_task(
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
                execution.set_system_task(
                    ReconciliationTaskKind::HumanApproval,
                    verification.human_approval,
                    if verification.human_approval {
                        "Explicit HumanApproval Evidence is present.".into()
                    } else {
                        "Explicit HumanApproval Evidence is still required.".into()
                    },
                );
            }
            Ok(execution.clone())
        })?;
    let mut status = execution.status();
    let (intent_checked, intent_blockers) =
        reconciliation_intent_audit(workspace, &plan.change_intents)?;
    status.intent_checked = intent_checked;
    if !intent_blockers.is_empty() {
        status.blocked = status.blocked.saturating_add(intent_blockers.len());
        status.converged = false;
    }
    status.intent_blockers = intent_blockers;
    Ok(status)
}

#[cfg(test)]
#[path = "../../../tests/unit/intelligence/intent_audit.rs"]
mod intent_audit_tests;
