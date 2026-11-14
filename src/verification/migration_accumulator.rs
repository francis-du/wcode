use super::*;

#[derive(Default)]
pub(super) struct TransitionAccumulator {
    pub(super) legacy_matches: usize,
    pub(super) replacement_matches: usize,
    pub(super) legacy_locations: Vec<MigrationAuditLocation>,
}

pub(crate) fn audit(
    workspace: &Workspace,
    revision: &Revision,
) -> Result<Option<MigrationAuditReport>> {
    let started = Instant::now();
    let spec = match load_spec(workspace) {
        Ok(Some(spec)) => spec,
        Ok(None) => return Ok(None),
        Err(error) => {
            return Ok(Some(invalid_report(
                revision,
                error.to_string(),
                started.elapsed().as_millis(),
            )));
        }
    };

    let mut findings = Vec::new();
    let mut findings_truncated = false;
    for path in &spec.required_paths {
        if workspace.path_info(path).is_err() {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "required_path_missing".into(),
                    rule_id: None,
                    message: format!("required migration path is missing or inaccessible: {path}"),
                    locations: Vec::new(),
                },
            );
        }
    }
    for path in &spec.forbidden_paths {
        let finding = match workspace.path_info(path) {
            Ok(_) => Some((
                "forbidden_path_present",
                format!("legacy migration path must be removed: {path}"),
            )),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            // Symlink, policy, permission and inspection errors do not prove
            // absence. Keep the migration gate closed without bypassing guards.
            Err(_) => Some((
                "forbidden_path_uninspectable",
                format!("cannot confirm forbidden migration path is absent: {path}"),
            )),
        };
        if let Some((code, message)) = finding {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: code.into(),
                    rule_id: None,
                    message,
                    locations: vec![MigrationAuditLocation {
                        path: path.clone(),
                        line: 1,
                    }],
                },
            );
        }
    }

    let mut roots = BTreeSet::new();
    for transition in &spec.transitions {
        let paths = if transition.paths.is_empty() {
            &spec.scopes
        } else {
            &transition.paths
        };
        roots.extend(paths.iter().cloned());
    }

    let mut files = BTreeSet::new();
    let mut truncated = false;
    for root in roots {
        match workspace.source_files(&root, MAX_AUDIT_FILES) {
            Ok((scoped, scoped_truncated)) => {
                truncated |= scoped_truncated;
                for path in scoped {
                    if path == CONFIG_PATH
                        || path.starts_with(".wcode/")
                        || excluded_path(&path, &spec.exclude_paths)
                    {
                        continue;
                    }
                    files.insert(path);
                    if files.len() > MAX_AUDIT_FILES {
                        truncated = true;
                        break;
                    }
                }
            }
            Err(error) => {
                push_finding(
                    &mut findings,
                    &mut findings_truncated,
                    MigrationAuditFinding {
                        code: "scope_unavailable".into(),
                        rule_id: None,
                        message: format!("migration audit scope `{root}` is unavailable: {error}"),
                        locations: Vec::new(),
                    },
                );
            }
        }
        if truncated {
            break;
        }
    }
    if truncated {
        push_finding(
            &mut findings,
            &mut findings_truncated,
            MigrationAuditFinding {
                code: "scan_truncated".into(),
                rule_id: None,
                message: format!(
                    "migration audit exceeded its {MAX_AUDIT_FILES}-file scan bound; completeness cannot be proven"
                ),
                locations: Vec::new(),
            },
        );
    }

    let mut accumulators = spec
        .transitions
        .iter()
        .map(|transition| (transition.id.clone(), TransitionAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut scanned_files = 0usize;
    let mut scanned_bytes = 0usize;
    if !truncated {
        for path in files.iter().take(MAX_AUDIT_FILES) {
            let source = match workspace.load_source(path) {
                Ok(source) => source,
                Err(error) => {
                    push_finding(
                        &mut findings,
                        &mut findings_truncated,
                        MigrationAuditFinding {
                            code: "target_unreadable".into(),
                            rule_id: None,
                            message: format!(
                                "migration target `{path}` cannot be audited: {error}"
                            ),
                            locations: Vec::new(),
                        },
                    );
                    continue;
                }
            };
            if scanned_bytes.saturating_add(source.content.len()) > MAX_AUDIT_BYTES {
                truncated = true;
                push_finding(
                    &mut findings,
                    &mut findings_truncated,
                    MigrationAuditFinding {
                        code: "scan_truncated".into(),
                        rule_id: None,
                        message: format!(
                            "migration audit exceeded its {MAX_AUDIT_BYTES}-byte scan bound; completeness cannot be proven"
                        ),
                        locations: Vec::new(),
                    },
                );
                break;
            }
            scanned_files = scanned_files.saturating_add(1);
            scanned_bytes = scanned_bytes.saturating_add(source.content.len());
            for transition in &spec.transitions {
                if !transition_applies(&source.path, transition, &spec.scopes) {
                    continue;
                }
                let accumulator = accumulators
                    .get_mut(&transition.id)
                    .expect("validated transition accumulator");
                if let Some(pattern) = transition.legacy.as_deref() {
                    for (index, _) in source.content.match_indices(pattern) {
                        accumulator.legacy_matches = accumulator.legacy_matches.saturating_add(1);
                        if accumulator.legacy_locations.len() < MAX_SAMPLE_LOCATIONS {
                            accumulator.legacy_locations.push(MigrationAuditLocation {
                                path: source.path.clone(),
                                line: line_number(&source.content, index),
                            });
                        }
                    }
                }
                if let Some(pattern) = transition.replacement.as_deref() {
                    accumulator.replacement_matches = accumulator
                        .replacement_matches
                        .saturating_add(source.content.match_indices(pattern).count());
                }
            }
        }
    }

    let mut transitions = Vec::with_capacity(spec.transitions.len());
    for transition in &spec.transitions {
        let accumulator = accumulators
            .remove(&transition.id)
            .expect("validated transition accumulator");
        let min_replacement_matches = transition
            .replacement
            .as_ref()
            .map(|_| transition.min_replacement_matches.max(1))
            .unwrap_or(0);
        let mixed_state = accumulator.legacy_matches > 0 && accumulator.replacement_matches > 0;
        if accumulator.legacy_matches > 0 {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "legacy_remaining".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "{} legacy occurrence(s) remain for migration rule `{}`",
                        accumulator.legacy_matches, transition.id
                    ),
                    locations: accumulator.legacy_locations,
                },
            );
        }
        if transition.replacement.is_some()
            && accumulator.replacement_matches < min_replacement_matches
        {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "replacement_missing".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "migration rule `{}` found {} replacement occurrence(s), below required minimum {}",
                        transition.id,
                        accumulator.replacement_matches,
                        min_replacement_matches
                    ),
                    locations: Vec::new(),
                },
            );
        }
        if mixed_state {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "mixed_state".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "migration rule `{}` contains both legacy and replacement patterns; the repository is only partially migrated",
                        transition.id
                    ),
                    locations: Vec::new(),
                },
            );
        }
        transitions.push(MigrationTransitionResult {
            id: transition.id.clone(),
            legacy_matches: accumulator.legacy_matches,
            replacement_matches: accumulator.replacement_matches,
            min_replacement_matches,
            mixed_state,
        });
    }

    let passed = findings.is_empty() && !truncated;
    let summary = if passed {
        format!(
            "Migration audit `{}` passed: {} transition(s), {} required path(s), {} forbidden path(s), {scanned_files} file(s) inspected.",
            spec.id,
            spec.transitions.len(),
            spec.required_paths.len(),
            spec.forbidden_paths.len()
        )
    } else {
        format!(
            "Migration audit `{}` failed with {} retained finding(s){}; behavioral verification must not be treated as proof of migration completeness.",
            spec.id,
            findings.len(),
            if findings_truncated { " (details truncated)" } else { "" }
        )
    };
    Ok(Some(MigrationAuditReport {
        id: spec.id,
        config_path: CONFIG_PATH,
        provider: "wcode-migration-audit",
        precision: "deterministic",
        revision: revision.clone(),
        passed,
        scanned_files,
        scanned_bytes,
        truncated,
        findings_truncated,
        required_paths: spec.required_paths.len(),
        forbidden_paths: spec.forbidden_paths.len(),
        transitions,
        findings,
        summary,
        elapsed_ms: started.elapsed().as_millis(),
    }))
}

pub(crate) fn context_summary(workspace: &Workspace) -> Option<Value> {
    if !config_present(workspace) {
        return None;
    }
    Some(match load_spec(workspace) {
        Ok(Some(spec)) => json!({
            "configured": true,
            "valid": true,
            "id": spec.id,
            "config_path": CONFIG_PATH,
            "provider": "wcode-migration-audit",
            "precision": "deterministic",
            "gate_order": "before_behavioral_verification",
            "scopes": spec.scopes,
            "transitions": spec.transitions.len(),
            "required_paths": spec.required_paths.len(),
            "forbidden_paths": spec.forbidden_paths.len(),
        }),
        Ok(None) => return None,
        Err(error) => json!({
            "configured": true,
            "valid": false,
            "config_path": CONFIG_PATH,
            "provider": "wcode-migration-audit",
            "precision": "deterministic",
            "error": error.to_string(),
        }),
    })
}

pub(crate) fn capabilities() -> Value {
    json!({
        "config": CONFIG_PATH,
        "schema_version": 1,
        "provider": "wcode-migration-audit",
        "precision": "deterministic",
        "matching": "literal-substring",
        "path_assertions": ["required", "forbidden"],
        "gate_order": "before_behavioral_verification",
        "max_transitions": MAX_TRANSITIONS,
        "max_paths": MAX_PATHS,
        "max_scan_files": MAX_AUDIT_FILES,
        "max_scan_bytes": MAX_AUDIT_BYTES,
        "no_shell": true,
    })
}
