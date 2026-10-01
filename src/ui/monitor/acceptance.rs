use super::*;

const ACCEPTANCE_CHECK_LIMIT: usize = 64;
const ACCEPTANCE_REASON_LIMIT: usize = 32;

pub(super) struct AcceptanceView<'a> {
    pub(super) record: Option<&'a Value>,
    state: &'static str,
    notice: &'static str,
}

fn digest(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.strip_prefix("sha256:").is_some_and(|hash| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    })
}

fn member(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|text| choices.contains(&text))
}

fn action_name(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "capture_context" => Some("Capture context"),
        "activate_policy" => Some("Activate policy"),
        "refresh_policy" => Some("Refresh policy"),
        "capture_git" => Some("Capture Git"),
        "plan_verification" => Some("Plan verification"),
        "run_verification" => Some("Run verification"),
        "inspect_failure" => Some("Inspect failure"),
        "request_review" => Some("Request review"),
        "request_human_approval" => Some("Request human approval"),
        "refresh_revision" => Some("Refresh revision"),
        "resolve_discovery" => Some("Resolve discovery"),
        _ => None,
    }
}

fn valid_verification(value: &Value) -> bool {
    let outcomes = ["pass", "fail", "inconclusive", "disagree"];
    value.is_null()
        || (value.is_object()
            && ["deterministic_result", "human_approval"]
                .iter()
                .all(|key| value[key].is_null() || member(&value[key], &outcomes))
            && value["stage_results"].as_object().is_some_and(|stages| {
                stages.iter().all(|(key, result)| {
                    ["property", "mutation", "fuzz", "runtime_canary"].contains(&key.as_str())
                        && member(result, &outcomes)
                })
            })
            && [
                "queued",
                "claimed",
                "submitted",
                "reviewer_failures",
                "reviewer_inconclusive",
                "disagreements",
            ]
            .iter()
            .all(|key| value[key].as_u64().is_some()))
}

fn valid_record(record: &Value) -> bool {
    record["schema_version"].as_u64() == Some(1)
        && record["producer"].as_str() == Some("wcode/native-acceptance/v1")
        && record["id"].as_str().is_some_and(|id| !id.is_empty())
        && digest(&record["record_digest"])
        && digest(&record["revision"]["code"])
        && (record["revision"]["design"].is_null() || digest(&record["revision"]["design"]))
        && record["captured_at_ms"].as_u64().is_some()
        && member(
            &record["state"],
            &["ready", "blocked", "needs_review", "incomplete", "stale"],
        )
        && member(
            &record["risk_level"],
            &["low", "medium", "high", "critical"],
        )
        && record["partial"].as_bool().is_some()
        && valid_verification(&record["verification"])
        && [
            "required",
            "discovered",
            "mapped",
            "executed",
            "passed",
            "failed",
            "skipped",
            "unavailable",
            "stale",
            "unknown",
        ]
        .iter()
        .all(|key| record["summary"][key].as_u64().is_some())
        && record["actions"].as_array().is_some_and(|items| {
            items.len() <= 11 && items.iter().all(|item| action_name(item).is_some())
        })
        && record["reasons"].as_array().is_some_and(|items| {
            items.len() <= 256
                && items.iter().all(|item| {
                    item["code"].is_string()
                        && (item["subject"].is_null() || item["subject"].is_string())
                        && action_name(&item["action"]).is_some()
                })
        })
        && record["checks"].as_array().is_some_and(|items| {
            items.len() <= 64
                && items.iter().all(|item| {
                    item["id"].is_string()
                        && (item["signature"].is_null() || item["signature"].is_string())
                        && ["required", "discovered", "mapped"]
                            .iter()
                            .all(|key| item[key].as_bool().is_some())
                        && member(
                            &item["execution"],
                            &["unknown", "executed", "skipped", "unavailable"],
                        )
                        && member(
                            &item["outcome"],
                            &["unknown", "pass", "fail", "inconclusive", "disagree"],
                        )
                        && member(
                            &item["freshness"],
                            &["current", "stale", "unbound", "missing"],
                        )
                        && member(&item["required_level"], &["quick", "full"])
                        && (item["level"].is_null() || member(&item["level"], &["quick", "full"]))
                        && item["evidence_ids"].as_array().is_some_and(|ids| {
                            ids.len() <= 4096 && ids.iter().all(Value::is_string)
                        })
                })
        })
        && record["evidence"]
            .as_array()
            .is_some_and(|items| items.len() <= 4096)
        && record["evidence_ids"]
            .as_array()
            .is_some_and(|items| items.len() <= 4096 && items.iter().all(Value::is_string))
}

pub(super) fn acceptance_view<'a>(
    stats: &'a IntelligenceStats,
    workspace: &str,
) -> AcceptanceView<'a> {
    let unknown = |notice| AcceptanceView {
        record: None,
        state: "unknown",
        notice,
    };
    let Some(record) = stats
        .project_acceptance
        .as_ref()
        .filter(|record| !record.is_null())
    else {
        return unknown("No canonical Acceptance Record · R refresh");
    };
    if !valid_record(record) {
        return unknown("Unsupported or incomplete Acceptance Record");
    }
    if record["workspace"].as_str() != Some(workspace) {
        return unknown("Acceptance belongs to another workspace");
    }
    let stale = stats.refreshing
        || stats.refresh_error.is_some()
        || stats.project_revision.as_ref() != Some(&record["revision"]);
    let (state, notice) = if stale {
        (
            "stale",
            "Historical observation · refresh must succeed before this is current",
        )
    } else if record["partial"].as_bool() == Some(true) && record["state"] == "ready" {
        ("incomplete", "Partial observation cannot be Ready")
    } else {
        (
            match record["state"].as_str() {
                Some("ready") => "ready",
                Some("blocked") => "blocked",
                Some("needs_review") => "needs_review",
                Some("stale") => "stale",
                _ => "incomplete",
            },
            "Canonical native decision · checks are not run by viewing",
        )
    };
    AcceptanceView {
        record: Some(record),
        state,
        notice,
    }
}

fn state_label(state: &str, language: UiLanguage) -> &'static str {
    language.tr(match state {
        "ready" => "Ready",
        "blocked" => "Blocked",
        "needs_review" => "Needs review",
        "incomplete" => "Incomplete",
        "stale" => "Stale",
        _ => "Unknown",
    })
}

fn tone(state: &str) -> Color {
    match state {
        "ready" => SUCCESS,
        "blocked" => DANGER,
        "needs_review" | "incomplete" | "stale" => WARNING,
        _ => TEXT_MUTED,
    }
}

fn text(value: &Value, key: &str) -> String {
    console_clean(value[key].as_str().unwrap_or("unknown"))
}

pub(super) fn acceptance_entry_count(stats: &IntelligenceStats, workspace: &str) -> usize {
    acceptance_view(stats, workspace)
        .record
        .map_or(0, |record| {
            record["reasons"]
                .as_array()
                .map_or(0, |items| items.len().min(ACCEPTANCE_REASON_LIMIT))
                + record["checks"]
                    .as_array()
                    .map_or(0, |items| items.len().min(ACCEPTANCE_CHECK_LIMIT))
        })
}

fn entries(record: &Value) -> Vec<(&Value, bool)> {
    record["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .take(ACCEPTANCE_REASON_LIMIT)
        .map(|item| (item, false))
        .chain(
            record["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .take(ACCEPTANCE_CHECK_LIMIT)
                .map(|item| (item, true)),
        )
        .collect()
}

pub(super) fn acceptance_inspect_targets(
    stats: &IntelligenceStats,
    workspace: &str,
    selected: usize,
) -> Vec<InspectTarget> {
    let Some(record) = acceptance_view(stats, workspace).record else {
        return Vec::new();
    };
    let rows = entries(record);
    let Some((item, check)) = rows.get(selected.min(rows.len().saturating_sub(1))) else {
        return Vec::new();
    };
    let check = if *check {
        Some(*item)
    } else {
        record["checks"]
            .as_array()
            .and_then(|checks| checks.iter().find(|check| check["id"] == item["subject"]))
    };
    let mut targets = Vec::new();
    for id in check.into_iter().flat_map(|check| {
        check["evidence_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
    }) {
        let Some(evidence) = record["evidence"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == *id))
        else {
            continue;
        };
        for target in evidence["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .take(16)
        {
            if let Some(target) = target.as_str().and_then(inspect_target) {
                if targets.contains(&target) {
                    continue;
                }
                targets.push(target);
                if targets.len() == 16 {
                    return targets;
                }
            }
        }
    }
    // Native stage/check targets may identify a language rather than a file.
    // Candidate paths remain a distinct inspection source, never replacement proof.
    if targets.is_empty() {
        for change in record["git"]["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .take(16)
        {
            for key in ["new_path", "old_path"] {
                if let Some(mut target) = change[key].as_str().and_then(inspect_target) {
                    target.candidate = true;
                    if targets.contains(&target) {
                        continue;
                    }
                    targets.push(target);
                    if targets.len() == 16 {
                        return targets;
                    }
                }
            }
        }
    }
    targets
}

fn action_guidance(action: &Value) -> &'static str {
    match action.as_str() {
        Some("activate_policy" | "refresh_policy") =>
            "Connected Agent: acceptance_policy action=preview/activate; native operator must approve the exact request.",
        Some("plan_verification") => "Connected Agent: acceptance_plan; inspect the bound Policy, Git and required checks.",
        Some("run_verification") => "Connected Agent: acceptance_verify; native checks remain authorization-bound.",
        Some("request_human_approval") =>
            "Request exact revision-bound Human Approval; Y/N only handles an independently pending native request.",
        Some("request_review") => "Connected Agent: verification_status; claim and submit the required independent review.",
        Some("resolve_discovery") => "Inspect discovery/configuration and Providers; unavailable checks are not passes.",
        Some("inspect_failure") => "Inspect the exact linked Evidence, then W Web for Evidence and file/symbol drilldowns.",
        _ => "R refresh captures current native context; it does not execute verification or grant approval.",
    }
}

fn detail_lines(record: &Value, selected: usize, language: UiLanguage) -> Vec<Line<'static>> {
    let rows = entries(record);
    let Some((item, check)) = rows.get(selected.min(rows.len().saturating_sub(1))) else {
        return vec![Line::raw(
            language.tr("No blocking items or required checks"),
        )];
    };
    let mut lines = vec![Line::from(Span::styled(
        language.tr(if *check {
            "Check and exact Evidence"
        } else {
            "Blocking action"
        }),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))];
    if *check {
        lines.push(Line::raw(format!(
            "{} · required {}",
            text(item, "id"),
            item["required"]
        )));
        lines.push(Line::raw(format!(
            "discovered {} · mapped {} · execution {}",
            item["discovered"],
            item["mapped"],
            text(item, "execution")
        )));
        lines.push(Line::raw(format!(
            "outcome {} · freshness {} · required {} / reported {}",
            text(item, "outcome"),
            text(item, "freshness"),
            text(item, "required_level"),
            text(item, "level")
        )));
        lines.push(Line::raw(format!("signature {}", text(item, "signature"))));
        let ids = item["evidence_ids"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if ids.is_empty() {
            lines.push(Line::raw(
                language.tr("No linked Evidence · unknown is not Pass"),
            ));
        }
        for id in ids.iter().take(8) {
            let id = console_clean(id.as_str().unwrap_or("unknown"));
            lines.push(Line::from(Span::styled(
                format!("Evidence {id}"),
                Style::default().fg(SECONDARY),
            )));
            let evidence = record["evidence"]
                .as_array()
                .and_then(|items| items.iter().find(|evidence| evidence["id"] == id));
            if let Some(evidence) = evidence {
                lines.push(Line::raw(format!(
                    "{} · {} · authority {} · freshness {}",
                    text(evidence, "producer"),
                    text(evidence, "kind"),
                    text(evidence, "authority"),
                    text(evidence, "freshness")
                )));
                lines.push(Line::raw(format!(
                    "relation {} · Code {}",
                    text(evidence, "verification_relation"),
                    text(&evidence["revision"], "code")
                )));
                for target in evidence["targets"].as_array().into_iter().flatten().take(4) {
                    lines.push(Line::raw(format!(
                        "target {}",
                        console_clean(target.as_str().unwrap_or("unknown"))
                    )));
                }
            } else {
                lines.push(Line::raw(language.tr(
                    "Linked Evidence metadata unavailable · no substitute selected",
                )));
            }
        }
        if ids.len() > 8 {
            lines.push(Line::raw(format!(
                "{} of {} Evidence references shown · W Web",
                8,
                ids.len()
            )));
        }
        lines.push(Line::raw(
            language.tr("W Web: Acceptance → Evidence → file / symbol"),
        ));
    } else {
        lines.push(Line::raw(format!(
            "{} · {}",
            text(item, "code"),
            text(item, "subject")
        )));
        lines.push(Line::raw(
            language.tr(action_name(&item["action"]).unwrap_or("Unknown")),
        ));
        lines.push(Line::raw(action_guidance(&item["action"])));
    }

    if let Some(verification) = record["verification"].as_object() {
        lines.push(Line::from(Span::styled(
            language.tr("Verification gates"),
            Style::default().fg(ACCENT),
        )));
        lines.push(Line::raw(format!(
            "deterministic {} · human approval {}",
            text(&record["verification"], "deterministic_result"),
            text(&record["verification"], "human_approval")
        )));
        if let Some(stages) = verification["stage_results"].as_object() {
            for (stage, outcome) in stages {
                lines.push(Line::raw(format!(
                    "stage {stage} · {}",
                    console_clean(outcome.as_str().unwrap_or("unknown"))
                )));
            }
        }
        lines.push(Line::raw(language.tr(
            "Observed gate results do not decide which gates Policy requires",
        )));
    }
    lines.push(Line::raw(format!(
        "Policy {} · generation {} / current {}",
        text(&record["policy"]["selection"], "policy_id"),
        record["policy"]["generation"],
        record["policy"]["current_generation"]
    )));

    lines.push(Line::raw(format!(
        "Record {}",
        text(record, "record_digest")
    )));
    lines.push(Line::raw(format!(
        "Code {} · Design {}",
        text(&record["revision"], "code"),
        text(&record["revision"], "design")
    )));
    lines
}

pub(super) fn render_acceptance_console(
    frame: &mut Frame<'_>,
    area: Rect,
    stats: &IntelligenceStats,
    ui: &DashboardState,
    workspace: &str,
) {
    if area.height == 0 {
        return;
    }
    if let Some(inspection) = ui.source_inspection.as_ref() {
        render_source_inspection(frame, area, inspection);
        return;
    }
    let view = acceptance_view(stats, workspace);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let record = view.record;
    let status = format!("{} · {}", state_label(view.state, ui.language), workspace);
    let summary = record
        .map(|record| {
            format!(
                "required {} · executed {} · pass {} · fail {} · unavailable {} · stale {}",
                record["summary"]["required"],
                record["summary"]["executed"],
                record["summary"]["passed"],
                record["summary"]["failed"],
                record["summary"]["unavailable"],
                record["summary"]["stale"],
            )
        })
        .unwrap_or_else(|| {
            ui.language
                .tr("Evidence counts do not determine Acceptance")
                .to_owned()
        });
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                status,
                Style::default()
                    .fg(tone(view.state))
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(console_clean(view.notice)),
            Line::raw(summary),
            Line::raw(record.map_or_else(
                || "risk unknown · Policy/plan unknown".to_owned(),
                |record| {
                    format!(
                        "risk {} · plan {} · Git HEAD {}",
                        text(record, "risk_level"),
                        text(&record["plan"], "id"),
                        text(&record["git"]["binding"], "head_sha")
                    )
                },
            )),
        ]),
        rows[0],
    );
    if let Some(record) = record {
        if ui.acceptance_detail_open {
            console_paragraph(
                frame,
                rows[1],
                detail_lines(record, ui.console_focus, ui.language),
                ui.console_scroll,
            );
        } else {
            let entries = entries(record);
            let selected = ui.console_focus.min(entries.len().saturating_sub(1));
            let visible = rows[1].height as usize;
            let offset = selected.saturating_sub(visible.saturating_sub(1));
            let items = entries
                .iter()
                .enumerate()
                .skip(offset)
                .take(visible)
                .map(|(index, (item, check))| {
                    let label = if *check {
                        format!(
                            "{} · {} / {} / {} · required {}",
                            text(item, "id"),
                            text(item, "execution"),
                            text(item, "outcome"),
                            text(item, "freshness"),
                            text(item, "required_level")
                        )
                    } else {
                        format!(
                            "{} · {} → {}",
                            text(item, "code"),
                            text(item, "subject"),
                            ui.language
                                .tr(action_name(&item["action"]).unwrap_or("Unknown"))
                        )
                    };
                    ListItem::new(format!(
                        "{} {}",
                        if index == selected { "›" } else { " " },
                        label
                    ))
                    .style(Style::default().fg(if index == selected {
                        ACCENT
                    } else {
                        TEXT_MUTED
                    }))
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                frame.render_widget(
                    Paragraph::new(ui.language.tr("No blocking items or required checks")),
                    rows[1],
                );
            } else {
                frame.render_widget(List::new(items), rows[1]);
            }
        }
        let shown_reasons = record["reasons"].as_array().map_or(0, Vec::len);
        frame.render_widget(
            Paragraph::new(format!(
                "{} · {} / {} reasons · {} checks",
                ui.language.tr(
                    "↑↓ select · Enter Evidence/action · F inspect file/symbol · PgUp/PgDn · W Web"
                ),
                shown_reasons.min(ACCEPTANCE_REASON_LIMIT),
                shown_reasons,
                record["checks"].as_array().map_or(0, Vec::len),
            ))
            .style(Style::default().fg(TEXT_DIM)),
            rows[2],
        );
    } else {
        console_paragraph(
            frame,
            rows[1],
            vec![
                Line::raw(
                    ui.language
                        .tr("No canonical decision · inspect current context before accepting"),
                ),
                Line::raw(
                    ui.language
                        .tr("R refresh · 2 Attention · 6 Observations · W Web"),
                ),
            ],
            ui.console_scroll,
        );
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/acceptance.rs"]
mod tests;
