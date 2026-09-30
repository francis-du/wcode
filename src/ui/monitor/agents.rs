use super::*;

const MAX_AGENT_ITEMS: usize = 20;

fn public_text(value: &Value, key: &str, max: usize) -> Value {
    value[key]
        .as_str()
        .map(|text| Value::String(console_clean(text).chars().take(max).collect()))
        .unwrap_or(Value::Null)
}

fn public_strings(value: &Value, key: &str) -> Value {
    value[key]
        .as_array()
        .map(|values| {
            Value::Array(
                values
                    .iter()
                    .take(16)
                    .filter_map(|value| {
                        value.as_str().map(|text| {
                            Value::String(console_clean(text).chars().take(300).collect())
                        })
                    })
                    .collect(),
            )
        })
        .unwrap_or(Value::Null)
}

fn public_revision(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    serde_json::json!({
        "code": public_text(value, "code", 160),
        "design": public_text(value, "design", 256),
    })
}

fn public_evidence(value: &Value) -> Value {
    Value::Array(
        value
            .as_array()
            .into_iter()
            .flatten()
            .take(16)
            .filter(|reference| reference.is_object())
            .map(|reference| {
                serde_json::json!({
                    "id": public_text(reference, "id", 160),
                    "kind": public_text(reference, "kind", 80),
                    "result": public_text(reference, "result", 80),
                    "producer": public_text(reference, "producer", 256),
                })
            })
            .collect(),
    )
}

/// Keep only observable metadata. Never retain the private capability used to submit a claim.
pub(super) fn project_worklist_projection(value: &Value) -> Value {
    let mut items = value["items"].as_array().cloned().unwrap_or_default();
    items.sort_by_key(|item| {
        if item["claim"].is_object() {
            0
        } else if item["status"] == "blocked" {
            1
        } else if item["status"] == "in_progress" {
            2
        } else if item["status"] == "pending" {
            3
        } else {
            4
        }
    });
    let retained = items.iter().take(MAX_AGENT_ITEMS).map(|item| {
        let claim = &item["claim"];
        let result = &item["result"];
        serde_json::json!({
            "id": public_text(item, "id", 64), "title": public_text(item, "title", 300),
            "status": public_text(item, "status", 40),
            "write_paths": public_strings(item, "write_paths"),
            "claim": if claim.is_object() { serde_json::json!({
                "actor": public_text(claim, "actor", 120),
                "base_revision": public_revision(&claim["base_revision"]),
                "claimed_at_ms": claim["claimed_at_ms"].as_u64(),
                "expired": claim["expired"].as_bool(),
                "expires_at_ms": claim["expires_at_ms"].as_u64(),
            }) } else { Value::Null },
            "result": if result.is_object() { serde_json::json!({
                "actor": public_text(result, "actor", 120), "outcome": public_text(result, "outcome", 40),
                "proof_status": public_text(result, "proof_status", 80),
                "evidence": public_evidence(&result["evidence"]),
                "evidence_count": result["evidence"].as_array().map_or(0, Vec::len),
                "evidence_truncated": result["evidence"].as_array().is_some_and(|items| items.len() > 16),
                "base_revision": public_revision(&result["base_revision"]),
                "repository_revision": public_revision(&result["repository_revision"]),
                "summary": public_text(result, "summary", 1000),
                "summary_truncated": result["summary"].as_str().is_some_and(|text| text.chars().count() > 1000),
                "reported_at_ms": result["reported_at_ms"].as_u64(),
            }) } else { Value::Null },
        })
    }).collect::<Vec<_>>();
    serde_json::json!({
        "exists": value["exists"].as_bool(), "revision": value["revision"].as_u64(),
        "total": items.len(), "items": retained,
        "truncated": items.len() > MAX_AGENT_ITEMS || value["truncated"].as_bool() == Some(true),
        "observed_at_ms": agent_now_ms(),
    })
}

fn agent_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn item_text<'a>(value: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    value[key].as_str().unwrap_or(fallback)
}

fn revision_text(label: &str, revision: &Value) -> String {
    format!(
        "{label} · code {} · design {}",
        item_text(revision, "code", "unknown"),
        item_text(revision, "design", "unknown")
    )
}

fn claim_expiry(claim: &Value) -> String {
    claim["expires_at_ms"]
        .as_u64()
        .map(|expiry| {
            let now = agent_now_ms();
            if expiry <= now {
                "expired".to_owned()
            } else {
                format!("expires in {}s", (expiry - now).div_ceil(1000))
            }
        })
        .unwrap_or_else(|| "expiry unknown".to_owned())
}

pub(super) fn render_agent_console(
    frame: &mut Frame<'_>,
    area: Rect,
    stats: &IntelligenceStats,
    ui: &DashboardState,
) {
    let Some(status) = stats.project_worklist.as_ref() else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Agent coordination state unknown · R refresh",
                    Style::default().fg(WARNING),
                )),
                Line::from("Worker reports are not verification evidence."),
            ]),
            area,
        );
        return;
    };
    if status["exists"].as_bool() == Some(false) {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("No Worklist observed for this workspace."),
                Line::from("Worker reports are not verification evidence."),
            ]),
            area,
        );
        return;
    }
    let items = status["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(if area.height >= 12 { 4 } else { 2 }),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "Worklist revision {} · {}/{} items{} · observed {}s ago",
                    status["revision"]
                        .as_u64()
                        .map(|revision| revision.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    items.len(),
                    status["total"].as_u64().unwrap_or(items.len() as u64),
                    if status["truncated"].as_bool() == Some(true) {
                        " · truncated"
                    } else {
                        ""
                    },
                    status["observed_at_ms"]
                        .as_u64()
                        .map(|observed| agent_now_ms().saturating_sub(observed) / 1000)
                        .unwrap_or(0)
                ),
                Style::default().fg(TEXT_DIM),
            )),
            Line::from(Span::styled(
                "Worker report · not Verification evidence",
                Style::default().fg(WARNING),
            )),
        ]),
        rows[0],
    );
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("No agent work items in this observed snapshot."),
            rows[1],
        );
        return;
    }
    let focus = ui.console_focus.min(items.len() - 1);
    let page = usize::from(rows[1].height.max(1));
    let offset = focus.saturating_sub(page - 1);
    let list = items
        .iter()
        .enumerate()
        .skip(offset)
        .take(page)
        .map(|(index, item)| {
            let actor = item["claim"]["actor"].as_str().unwrap_or("unclaimed");
            ListItem::new(Line::from(vec![
                Span::styled(
                    if index == focus { "▸ " } else { "  " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    truncate_end(
                        &format!(
                            "{} · {} · {actor}",
                            item_text(item, "id", "unknown"),
                            item_text(item, "status", "unknown")
                        ),
                        rows[1].width.saturating_sub(2) as usize,
                    ),
                    Style::default().fg(TEXT),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(list), rows[1]);
    let item = &items[focus];
    let claim = &item["claim"];
    let result = &item["result"];
    let scopes = item["write_paths"]
        .as_array()
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let mut details = vec![
        Line::from(Span::styled(
            format!(
                "{} · {}",
                item_text(item, "id", "unknown"),
                item_text(item, "title", "unknown")
            ),
            Style::default().fg(ACCENT),
        )),
        Line::from(format!(
            "State {} · actor {} · {}",
            item_text(item, "status", "unknown"),
            item_text(claim, "actor", "unclaimed"),
            if claim.is_object() {
                claim_expiry(claim)
            } else {
                "no active claim".to_owned()
            }
        )),
        Line::from(format!("Scope {scopes}")),
    ];
    if claim.is_object() {
        details.push(Line::from(revision_text(
            "Claim base",
            &claim["base_revision"],
        )));
    }
    if result.is_object() {
        details.push(Line::from(format!(
            "Reported result {} · actor {}",
            item_text(result, "outcome", "unknown"),
            item_text(result, "actor", "unknown")
        )));
        details.push(Line::from(format!(
            "Reported references {} · {}",
            result["evidence_count"].as_u64().unwrap_or(0),
            item_text(result, "proof_status", "not_reported")
        )));
        details.push(Line::from(revision_text(
            "Reported base",
            &result["base_revision"],
        )));
        details.push(Line::from(revision_text(
            "Reported revision",
            &result["repository_revision"],
        )));
        details.push(Line::from(
            item_text(result, "summary", "Report summary unavailable").to_owned(),
        ));
        for reference in result["evidence"].as_array().into_iter().flatten() {
            details.push(Line::from(format!(
                "Reference {} · {} · {} · {}",
                item_text(reference, "id", "unknown"),
                item_text(reference, "kind", "unknown"),
                item_text(reference, "result", "unknown"),
                item_text(reference, "producer", "unknown")
            )));
        }
        if result["evidence_truncated"].as_bool() == Some(true) {
            details.push(Line::from("Reference metadata truncated"));
        }
        if result["summary_truncated"].as_bool() == Some(true) {
            details.push(Line::from(Span::styled(
                "Worker report summary truncated",
                Style::default().fg(WARNING),
            )));
        }
    } else {
        details.push(Line::from(
            "Worker report unavailable; no verification conclusion is implied.",
        ));
    }
    console_paragraph(frame, rows[2], details, ui.console_scroll);
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/agents.rs"]
mod tests;
