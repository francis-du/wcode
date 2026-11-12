use super::{truncate_end, IntelligenceStats, UiLanguage, ACCENT, TEXT_DIM, TEXT_MUTED, WARNING};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(super) fn intelligence_refresh_line(
    stats: &IntelligenceStats,
    width: u16,
    language: UiLanguage,
) -> Line<'static> {
    if stats.refreshing {
        return Line::from(Span::styled(
            language.tr("Loading current Workspace data…"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(error) = stats.refresh_error.as_deref() {
        return Line::from(vec![
            Span::styled(
                format!("{}: ", language.tr("Partial refresh")),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate_end(error, width.saturating_sub(20) as usize),
                Style::default().fg(TEXT_MUTED),
            ),
        ]);
    }
    Line::from(Span::styled(
        language.tr("R refresh · risk checks remain authorization-bound"),
        Style::default().fg(TEXT_DIM),
    ))
}

// Absent counts are unknown, not measured zeros, and unknown state never takes
// success styling.
pub(super) fn convergence_label(
    stable: u64,
    changing: u64,
    needs: u64,
    incomplete: u64,
    observed: bool,
    language: UiLanguage,
) -> String {
    if !observed {
        return language.tr("convergence unknown").to_owned();
    }
    format!(
        "{} {} · {} {} · {} {}",
        stable,
        language.tr("stable"),
        changing,
        language.tr("changing"),
        needs.saturating_add(incomplete),
        language.tr("need work")
    )
}

pub(super) fn drift_findings_label(count: Option<u64>, language: UiLanguage) -> String {
    match count {
        Some(count) => format!("{count} {}", language.tr("drift finding(s)")),
        None => language.tr("unknown drift finding count").to_owned(),
    }
}

pub(super) fn evidence_counts_label(
    total: Option<u64>,
    failed: Option<u64>,
    disagreed: Option<u64>,
    language: UiLanguage,
) -> String {
    match (total, failed, disagreed) {
        (Some(total), Some(failed), Some(disagreed)) => format!(
            "{total} {} · {failed} {} · {disagreed} {}",
            language.tr("total"),
            language.tr("failed"),
            language.tr("disagreed")
        ),
        _ => language.tr("unknown evidence counts").to_owned(),
    }
}

pub(super) fn pending_tasks_label(count: Option<u64>, language: UiLanguage) -> String {
    match count {
        Some(count) => format!("{count} {}", language.tr("pending task(s)")),
        None => language.tr("pending count unknown").to_owned(),
    }
}

pub(super) fn blockers_label(count: Option<u64>, language: UiLanguage) -> String {
    match count {
        Some(count) => format!("{count} {}", language.tr("blocker(s)")),
        None => language.tr("unknown blocker count").to_owned(),
    }
}

// Counters that share one source event are only measurable once that event was
// observed; otherwise the group reads as unknown rather than as a zero.
pub(super) fn observed_count(value: u64, observed: bool, language: UiLanguage) -> String {
    if observed {
        value.to_string()
    } else {
        language.tr("unknown").to_owned()
    }
}

// Graph counts travel as one group: the node and edge totals come from the same
// snapshot, so a missing side means the graph was never observed, not that it is
// empty.
pub(super) fn graph_label(nodes: Option<u64>, edges: Option<u64>, language: UiLanguage) -> String {
    match (nodes, edges) {
        (Some(nodes), Some(edges)) => format!(
            "{nodes} {} · {edges} {}",
            language.tr("node(s)"),
            language.tr("edge(s)")
        ),
        _ => language.tr("unknown").to_owned(),
    }
}

pub(super) fn graph_diff_label(
    added_nodes: Option<u64>,
    removed_nodes: Option<u64>,
    changed_nodes: Option<u64>,
    added_edges: Option<u64>,
    removed_edges: Option<u64>,
    changed_edges: Option<u64>,
    language: UiLanguage,
) -> String {
    match (
        added_nodes,
        removed_nodes,
        changed_nodes,
        added_edges,
        removed_edges,
        changed_edges,
    ) {
        (Some(an), Some(rn), Some(cn), Some(ae), Some(re), Some(ce)) => format!(
            "{} +{an}/-{rn}/~{cn} · {} +{ae}/-{re}/~{ce}",
            language.tr("node(s)"),
            language.tr("edge(s)")
        ),
        _ => language.tr("unknown").to_owned(),
    }
}

pub(super) fn intelligence_line(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(TEXT_DIM)),
        Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}
