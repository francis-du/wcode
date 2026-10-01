//! Bounded, revision-bound issue projection shared by terminal and browser.
use crate::evidence::{Confidence, EvidenceKind, EvidenceResult, Revision};
use crate::intelligence_types::ProjectObservatory;
use crate::report_types::ChangeReviewReport;
use crate::verification::VerificationStatus;
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_ITEMS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectAttentionItem {
    pub id: String,
    pub kind: String,
    pub severity: AttentionSeverity,
    pub subject: String,
    pub message: String,
    pub provider: String,
    pub precision: String,
    pub section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectAttentionView {
    pub revision: Revision,
    pub total: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub items: Vec<ProjectAttentionItem>,
    pub truncated: bool,
    pub partial: bool,
    pub partial_reasons: Vec<String>,
}

impl ProjectAttentionView {
    pub(crate) fn empty(revision: Revision) -> Self {
        Self {
            revision,
            total: 0,
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            info: 0,
            items: Vec::new(),
            truncated: false,
            partial: false,
            partial_reasons: Vec::new(),
        }
    }
}

fn clean(value: &str) -> String {
    crate::workspace::redact_sensitive_text(value)
        .0
        .chars()
        .take(500)
        .collect()
}

impl ProjectAttentionItem {
    fn new(
        kind: &str,
        severity: AttentionSeverity,
        subject: &str,
        message: &str,
        section: &str,
        provider: &str,
        precision: &str,
    ) -> Self {
        Self {
            id: format!("{kind}:{}", clean(subject)),
            kind: kind.into(),
            severity,
            subject: clean(subject),
            message: clean(message),
            section: section.into(),
            provider: clean(provider),
            precision: precision.into(),
            path: None,
            requirement: None,
            count: 1,
        }
    }
}

pub(crate) fn build(
    project: &ProjectObservatory,
    review: Option<&ChangeReviewReport>,
    verification: &[VerificationStatus],
) -> ProjectAttentionView {
    use AttentionSeverity::*;
    let mut view = ProjectAttentionView::empty(project.repository_revision.clone());
    let mut items = Vec::new();
    let mut partial = |condition, reason: &str| {
        if condition {
            view.partial_reasons.push(reason.into());
        }
    };
    partial(review.is_none(), "change_review_unavailable");
    partial(
        review.is_some_and(|r| r.truncated || r.probes.iter().any(|p| !p.success)),
        "change_review_partial",
    );
    partial(
        project.code.graph_truncated || project.structure.truncated,
        "source_graph_partial",
    );
    partial(project.coverage.truncated, "traceability_partial");
    partial(
        project.proof.evidence_scan_truncated || project.proof.effective.truncated,
        "evidence_partial",
    );
    partial(verification.len() >= 100, "verification_history_partial");
    partial(
        project.language_quality.truncated,
        "language_registry_partial",
    );
    partial(
        project.risk.as_ref().is_some_and(|r| r.drift.truncated),
        "drift_partial",
    );
    for evidence in &project.proof.effective.items {
        let (kind, severity) = match evidence.result {
            EvidenceResult::Fail => ("evidence_failed", High),
            EvidenceResult::Disagree => ("evidence_disagreed", High),
            EvidenceResult::Inconclusive => ("evidence_inconclusive", Medium),
            EvidenceResult::Pass => continue,
        };
        let mut item = ProjectAttentionItem::new(
            kind,
            severity,
            &evidence.subject,
            evidence
                .summary
                .as_deref()
                .unwrap_or("Inspect the current revision evidence and its check output."),
            "proof",
            &evidence.producer,
            match evidence.kind {
                EvidenceKind::ModelReview => "model_review",
                EvidenceKind::HumanApproval => "human",
                EvidenceKind::Runtime => "runtime",
                _ if evidence.confidence == Confidence::Deterministic => "deterministic",
                _ => "recorded",
            },
        );
        item.provider = clean(&evidence.producer);
        // Effective records retain distinct policy/kind/confidence scopes. Never merge them.
        item.id = stable_id(kind, &evidence.id);
        if let Some(policy) = &evidence.policy {
            item.message = clean(&format!("{} [policy: {}]", item.message, clean(policy)));
        }
        items.push(item);
    }
    // The persisted plan history is newest-first; UUID order is not chronology.
    let latest = verification.iter().find(|status| {
        status.plan.revision.as_ref() == Some(&project.repository_revision)
            && status.plan.subject == format!("change:{}", project.repository_revision.code)
    });
    if let Some(status) = latest.filter(|status| !status.ready) {
        for (index, blocker) in status.blockers.iter().enumerate() {
            let mut item = ProjectAttentionItem::new(
                "verification_blocked",
                High,
                &status.plan.id,
                blocker,
                "proof",
                "verification-plan",
                "deterministic",
            );
            item.id = format!("verification_blocked:{}:{index}", status.plan.id);
            items.push(item);
        }
    } else if latest.is_none() && !project.changes.is_empty() {
        items.push(ProjectAttentionItem::new(
            "verification_missing",
            Medium,
            "Current changes",
            "No inspected verification plan is bound to this repository revision.",
            "proof",
            "verification-plan",
            "deterministic",
        ));
    }
    for diagnostic in &project.coverage.diagnostics {
        let mut item = ProjectAttentionItem::new(
            "design_diagnostic",
            if diagnostic.severity == crate::design::DiagnosticSeverity::Error {
                High
            } else {
                Medium
            },
            "Design State",
            &diagnostic.message,
            "requirements",
            "design-validator",
            "declared",
        );
        item.id = format!(
            "design_diagnostic:{}:{}:{}",
            clean(&diagnostic.code),
            clean(&diagnostic.path),
            clean(&diagnostic.message)
        );
        items.push(item);
    }
    if !project.coverage.initialized {
        items.push(ProjectAttentionItem::new("design_missing", Info, "Design State",
            "Requirement and acceptance mappings are unavailable until Design State is initialized.",
            "requirements", "design-validator", "declared"));
    }
    for requirement in &project.coverage.requirements {
        let unresolved = requirement
            .implementation
            .iter()
            .chain(&requirement.verification)
            .filter(|r| !r.resolved)
            .count();
        if unresolved == 0 {
            continue;
        }
        let mut item = ProjectAttentionItem::new("trace_unresolved", Medium, &requirement.title,
            "Implementation or verification references could not be resolved; inspect their mappings.",
            "requirements", "traceability", "syntax");
        item.id = format!("trace_unresolved:{}", requirement.id);
        item.requirement = Some(requirement.id.clone());
        item.count = unresolved;
        items.push(item);
    }
    if let Some(risk) = &project.risk {
        for finding in &risk.risks {
            let mut item = ProjectAttentionItem::new(
                "risk",
                severity(finding.level),
                &finding.subject,
                &finding.summary,
                "changes",
                "risk-analysis",
                "heuristic",
            );
            item.id = stable_id("risk", &finding.id);
            items.push(item);
        }
        for finding in &risk.drift.findings {
            let severity = match finding.risk_level {
                crate::risk::RiskLevel::Critical => Critical,
                crate::risk::RiskLevel::High => High,
                crate::risk::RiskLevel::Medium => Medium,
                crate::risk::RiskLevel::Low => Low,
            };
            let mut item = ProjectAttentionItem::new(
                "drift",
                severity,
                &finding.subject,
                &finding.message,
                "architecture",
                "drift-analysis",
                "mixed",
            );
            item.id = format!("drift:{}", finding.id);
            item.path = finding.paths.first().cloned();
            item.requirement = finding.affected_requirements.first().cloned();
            items.push(item);
        }
    }
    for file in project
        .structure
        .largest_files
        .iter()
        .filter(|file| file.over_limit && !file.generated)
    {
        let mut item = ProjectAttentionItem::new(
            "oversized_source",
            High,
            &file.path,
            &format!(
                "{} lines exceeds the {} line maintainability bound.",
                file.lines, project.structure.line_limit
            ),
            "files",
            "source-index",
            "syntax",
        );
        item.path = Some(file.path.clone());
        items.push(item);
    }
    let shown_oversized = items
        .iter()
        .filter(|item| item.kind == "oversized_source")
        .count();
    let remaining_oversized = project
        .structure
        .oversized_files
        .saturating_sub(shown_oversized);
    if remaining_oversized > 0 {
        let mut item = ProjectAttentionItem::new("oversized_remaining", High, "Source file bounds",
            "Additional source files exceed the line limit outside the bounded largest-file listing.",
            "files", "source-index", "syntax");
        item.count = remaining_oversized;
        items.push(item);
        view.partial_reasons.push("oversized_paths_partial".into());
    }
    for language in &project.language_quality.languages {
        for gap in &language.gaps {
            let mut item = ProjectAttentionItem::new(
                "provider_gap",
                Info,
                &format!("{:?}", language.language),
                gap,
                "quality",
                project.language_quality.provider,
                "declared",
            );
            item.id = format!("provider_gap:{}:{}", item.subject, clean(gap));
            items.push(item);
        }
    }
    items.sort_by(|left, right| (left.severity, &left.id).cmp(&(right.severity, &right.id)));
    items.dedup_by(|left, right| left.id == right.id);
    view.total = items.len();
    for item in &items {
        match item.severity {
            Critical => view.critical += 1,
            High => view.high += 1,
            Medium => view.medium += 1,
            Low => view.low += 1,
            Info => view.info += 1,
        }
    }
    view.truncated = items.len() > MAX_ITEMS;
    items.truncate(MAX_ITEMS);
    view.items = items;
    view.partial = !view.partial_reasons.is_empty();
    view
}

fn stable_id(kind: &str, identity: &str) -> String {
    format!("{kind}:{:x}", Sha256::digest(identity.as_bytes()))
}

fn severity(level: crate::risk::RiskLevel) -> AttentionSeverity {
    match level {
        crate::risk::RiskLevel::Critical => AttentionSeverity::Critical,
        crate::risk::RiskLevel::High => AttentionSeverity::High,
        crate::risk::RiskLevel::Medium => AttentionSeverity::Medium,
        crate::risk::RiskLevel::Low => AttentionSeverity::Low,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/intelligence/attention.rs"]
mod tests;
