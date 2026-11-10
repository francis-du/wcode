// Design-vs-actual matrix. One row per design object carries the design
// statement, the observed state and the delta between them, so a drift can be
// traced from intent to implementation without switching views. It combines
// existing identities; it does not redefine them.
const consistencyKinds = ["all", "requirement", "component", "constraint", "acceptance"];
const consistencyStates = ["all", "needs_attention", "stable", "unobserved"];

const consistencyFindings = () => (state.project?.risk?.drift?.findings || [])
  .filter(item => item.kind === "runtime_drift");
// A requirement row is answered by every drift that names it: an incomplete
// implementation/verification trace, a runtime bound it breaks, and an
// actual-state change the design never recorded all describe the same
// requirement, so filtering here would hide the reason it needs attention.
const consistencyRequirementFindings = () => state.project?.risk?.drift?.findings || [];

const consistencyDriftKinds = ["all", "implementation_drift", "design_drift", "runtime_drift"];
const driftKindLabels = value => ({
  all: t("All drift kinds"),
  implementation_drift: t("implementation drift"),
  design_drift: t("design drift"),
  runtime_drift: t("runtime drift"),
}[value] || statusLabel(value));

// The queue links each finding back to the object that carries its evidence.
// A subject that names no known object still lands on the drift diagnostics.
const driftFindingTarget = finding => {
  const subject = String(finding.subject || "");
  const project = state.project || {};
  if ((project.requirements || []).some(item => item.id === subject)) return `object:requirement:${subject}`;
  if ((architectureData().components || []).some(item => item.id === subject)) return `object:component:${subject}`;
  if (subject.startsWith("implementation:")) return "changesSection";
  return "diagnosticsSection";
};

// Numeric drift keeps its unit, precision and revision binding together: a
// percentage alone cannot say what was measured, on what, or for which version.
const driftDeviationText = deviation => {
  if (!deviation) return "";
  const parts = [deviation.metric, `${num(deviation.observed)} / ${num(deviation.expected_max)} ${deviation.unit}`,
    `${num(deviation.deviation_percent)}%`, statusLabel(deviation.precision),
    deviation.revision_bound ? t("revision-bound") : t("not revision-bound")];
  return parts.filter(Boolean).join(" · ");
};

const consistencyRows = () => {
  const project = state.project || {}, design = project.design || {};
  const findings = consistencyFindings(), requirementFindings = consistencyRequirementFindings();
  const rows = [];
  for (const requirement of (project.requirements || [])) {
    const dependencies = requirement.dependency_alignment || [];
    const blocking = dependencies.filter(dependency => dependency.blocking).length;
    const needs = requirement.convergence === "needs_convergence" || blocking > 0;
    // A drift affecting this requirement is only actionable next to the
    // convergence plan that claims it; an unclaimed drift says so instead.
    const affecting = requirementFindings.filter(finding => (finding.affected_requirements || []).includes(requirement.id));
    const plans = affecting.length
      ? (project.reconciliation || []).filter(plan => (plan.drift_ids || []).some(id => affecting.some(finding => finding.id === id)))
      : [];
    const convergence = !affecting.length ? ""
      : !plans.length ? [t("no convergence plan references this drift")]
        : plans.map(plan => {
          const tasks = plan.implementation_tasks || [];
          const matched = (project.reconciliation_execution || []).find(item => item?.execution?.plan_id === plan.id) || null;
          const detail = matched
            ? `${matched.pending} ${t("pending")} · ${matched.completed} ${t("completed")} · ${matched.failed} ${t("failed")} · ${matched.converged ? t("converged") : t("not converged")}`
            : unit(tasks.length, "task", "tasks", "个任务");
          // Drift ids are stable across revisions, so a plan bound to another
          // revision can still claim a current finding. Say so instead of
          // presenting its counts as the current input's result.
          const current = state.project?.repository_revision;
          const stale = plan.revision != null && current != null
            && (plan.revision.code !== current.code
              || (plan.revision.design ?? null) !== (current.design ?? null));
          return `${plan.id}${stale ? ` [${t("superseded revision")}]` : ""} (${detail})`;
        });
    rows.push({
      id: requirement.id,
      kind: "requirement",
      design: [statusLabel(requirement.priority),
        unit((requirement.components || []).length, "component", "components", "个组件"),
        unit((requirement.acceptance || []).length, "acceptance criterion", "acceptance criteria", "条验收标准")].join(" · "),
      actual: [statusLabel(requirement.convergence),
        blocking ? `${num(blocking)} ${t("blockers")}` : t("no blockers"),
        requirement.changed ? t("changed") : t("unchanged"),
        unit(requirement.implementation_lines || 0, "line", "lines", "行"),
        affecting.flatMap(finding => finding.paths || []).slice(0, 3).map(esc).join(", "),
        convergence].filter(Boolean).join(" · "),
      state: needs ? "needs_attention" : requirement.convergence === "stable" ? "stable" : "unobserved",
      navigate: `object:requirement:${requirement.id}`,
    });
  }
  for (const component of (architectureData().components || [])) {
    const tone = architectureNodeTone(component, architectureData().dependencies || []);
    const changed = Boolean(component.changed);
    // Dependency alignment keeps the four evidence states apart: an unverified
    // declared dependency is not missing implementation, and an undeclared
    // observation is only actionable when its provider is stronger than syntax.
    const edges = (architectureData().dependencies || [])
      .filter(edge => edge.from === component.id || edge.to === component.id);
    const count = status => edges.filter(edge => edge.status === status).length;
    const architecture = architectureData();
    const candidateTotal = Number(architecture.dependency_candidates_total ?? (architecture.dependency_candidates || []).length);
    const ownersIncomplete = architectureOwnerSamplesIncomplete();
    // Truncation and a zero denominator have to travel with the counts: an empty
    // sample is not evidence that the component has no dependencies.
    const evidenceLimited = edges.some(edge => edge.evidence_truncated)
      || architecture.dependency_candidates_truncated
      || candidateTotal > (architecture.dependency_candidates || []).length;
    const alignment = [
      count("aligned") ? `${num(count("aligned"))} ${t("aligned")}` : "",
      count("unverified_actual") ? `${num(count("unverified_actual"))} ${t("declared, not observed")}` : "",
      count("undeclared_actual") ? `${num(count("undeclared_actual"))} ${t("observed, not declared")}` : "",
      count("observed_actual") ? `${num(count("observed_actual"))} ${t("observed candidate")}` : "",
      count("unknown") ? `${num(count("unknown"))} ${t("unknown")}` : "",
      !edges.length ? localized("No observed dependency edges for this component.", "该组件没有被观测到的依赖边。") : "",
      evidenceLimited ? localized("Dependency evidence is a bounded sample; absence is not proof.", "依赖证据为有界样本；未出现不代表不存在。") : "",
      ownersIncomplete ? localized("Owner samples are incomplete; more affected components may be omitted.", "归属样本不完整；可能还有未显示的受影响组件。") : "",
    ].filter(Boolean).join(" · ");
    const hasBlocking = edges.some(edge => edge.blocking);
    const hasCandidate = edges.some(edge => ["unverified_actual", "observed_actual", "unknown"].includes(edge.status));
    rows.push({
      id: component.id,
      kind: "component",
      design: [unit((component.responsibilities || []).length, "responsibility", "responsibilities", "项职责"),
        unit((component.depends_on || []).length, "declared dependency", "declared dependencies", "条声明依赖")].join(" · "),
      actual: [statusLabel(tone),
        unit(component.implementation_files || 0, "mapped file", "mapped files", "个映射文件"),
        changed ? t("changed") : t("unchanged"),
        alignment].filter(Boolean).join(" · "),
      state: hasBlocking || tone === "drift" ? "needs_attention" : hasCandidate ? "unobserved" : "stable",
      navigate: `object:component:${component.id}`,
    });
  }
  // Acceptance and behaviour contracts come from the required-verification rows,
  // which already separate mapped, executed, outcome and freshness. A check id is
  // not a criterion id, so the design text comes from the criteria that declare
  // this check through an explicit verification reference.
  for (const check of (project.proof?.acceptance?.checks || [])) {
    const mapped = Boolean(check.mapped), executed = check.execution === "executed";
    const failed = check.outcome === "fail", fresh = check.freshness === "current";
    const delta = !mapped ? t("not mapped")
      : !executed ? t("mapped but not executed")
        : failed ? t("executed and failed")
          : !fresh ? t("passed but stale")
            : t("passed and current");
    const criteria = Object.values(design.acceptance || {}).filter(criterion =>
      (criterion.verification || []).some(reference => reference.kind === "check" && reference.id === check.id));
    // A criterion that names the check but declares no verification reference is
    // a gap, so it is stated rather than silently omitted.
    const criterionText = criterion => {
      const refs = criterion.verification || [];
      const kinds = [...new Set(refs.map(reference => reference.kind))].join("/");
      const statement = typeof criterion.statement === "string" && criterion.statement.trim()
        ? ` — ${criterion.statement.trim().slice(0, 120)}`
        : "";
      return `${criterion.title || criterion.id}${statement} [${refs.length} ${kinds}]`;
    };
    rows.push({
      id: check.id,
      kind: "acceptance",
      design: criteria.length
        ? criteria.map(criterionText).join(" / ")
        : t("no acceptance criterion references this check"),
      // The outcome is only meaningful next to the input version and policy it
      // was produced under, so an observed check carries them; an unobserved
      // one leaves them out rather than implying a current-version result.
      actual: [mapped ? t("mapped") : t("not mapped"),
        `${t("Required")}: ${check.required_level || "—"}`,
        check.level ? `${t("Reported")}: ${check.level}` : "",
        `${t("Execution")}: ${check.execution || "—"}`,
        `${t("Outcome")}: ${check.outcome || "—"}`,
        `${t("Freshness")}: ${check.freshness || "—"}`,
        check.evidence_id ? `${t("Evidence")}: ${check.evidence_id}` : "",
        check.revision?.code ? `${t("Revision")}: ${String(check.revision.code).slice(0, 19)}` : "",
        check.policy ? `${t("Policy")}: ${check.policy}` : ""].filter(Boolean).join(" · "),
      state: !mapped || !executed || failed ? "needs_attention" : !fresh ? "needs_attention" : "stable",
      delta,
      navigate: "proofSection",
    });
  }
  // A declared criterion that no check references is a missing acceptance item.
  // It has no verification row to attach to, so the matrix shows it directly.
  for (const criterion of Object.values(design.acceptance || {})) {
    const refs = (criterion.verification || []).filter(reference => reference.kind === "check");
    if (refs.length) continue;
    const statement = typeof criterion.statement === "string" && criterion.statement.trim()
      ? ` — ${criterion.statement.trim().slice(0, 120)}`
      : "";
    rows.push({
      id: criterion.id,
      kind: "acceptance",
      design: `${criterion.title || criterion.id}${statement} [${t("no verification ref")}]`,
      actual: t("no check references this criterion"),
      state: "needs_attention",
      navigate: "proofSection",
    });
  }
  for (const constraint of Object.values(design.constraints || {})) {
    const finding = findings.find(item => item.subject === constraint.id);
    const deviation = finding?.deviation;
    const exceeded = Boolean(deviation) && Number(deviation.deviation_percent) > 0;
    rows.push({
      id: constraint.id,
      kind: "constraint",
      design: constraint.title || constraint.id,
      actual: !finding ? t("no observation recorded")
        : !deviation ? t("observation without a comparable value")
          : `${num(deviation.observed)} / ${num(deviation.expected_max)} ${deviation.unit}`,
      state: !finding ? "unobserved" : exceeded ? "needs_attention" : "stable",
      navigate: "diagnosticsSection",
    });
  }
  return rows;
};

function renderConsistencyMatrix() {
  if (!els.consistencySection) return;
  const kind = consistencyKinds.includes(state.consistencyKind) ? state.consistencyKind : "all";
  const filter = consistencyStates.includes(state.consistencyState) ? state.consistencyState : "all";
  const rows = consistencyRows().filter(row =>
    (kind === "all" || row.kind === kind) && (filter === "all" || row.state === filter));
  const options = (values, current, labels) => values.map(value =>
    `<option value="${esc(value)}"${value === current ? " selected" : ""}>${esc(labels(value))}</option>`).join("");
  const kindLabels = value => ({ all: t("All kinds"), requirement: t("Requirements"), component: t("Components"), constraint: t("Constraints"), acceptance: t("Acceptance") })[value];
  const stateLabels = value => ({ all: t("All states"), needs_attention: t("Needs attention"), stable: t("Stable"), unobserved: t("Unobserved") })[value];
  const body = rows.length ? rows.map(row => `<tr data-consistency-row="${esc(row.navigate)}" data-consistency-label="${esc(localized(`Open ${row.id}`, `打开 ${row.id}`))}" tabindex="0" role="button"${row.state === "needs_attention" ? ' class="attention"' : ""}><th scope="row"><code>${esc(row.id)}</code></th><td>${esc(t({ requirement: "Requirement", component: "Component", constraint: "Constraint", acceptance: "Acceptance" }[row.kind]))}</td><td>${esc(row.design)}</td><td>${esc(row.actual)}</td><td>${esc(row.delta || stateLabels(row.state))}</td></tr>`).join("")
    : `<tr><td colspan="5" class="empty">${esc(t("No design object matches this filter."))}</td></tr>`;
  const driftKind = consistencyDriftKinds.includes(state.consistencyDriftKind) ? state.consistencyDriftKind : "all";
  const drift = state.project?.risk?.drift;
  const driftRows = (drift?.findings || []).filter(finding => driftKind === "all" || finding.kind === driftKind);
  const driftBody = driftRows.length ? driftRows.map(finding => `<tr data-consistency-drift-row="${esc(driftFindingTarget(finding))}" data-consistency-drift-label="${esc(localized(`Open drift ${finding.id}`, `打开偏离 ${finding.id}`))}" tabindex="0" role="button"><th scope="row"><code>${esc(finding.id)}</code></th><td>${esc(driftKindLabels(finding.kind))}</td><td>${esc(statusLabel(finding.risk_level))}</td><td>${esc(finding.subject)}</td><td>${esc(finding.message)}</td><td>${esc((finding.affected_requirements || []).join(", "))}</td><td>${esc((finding.paths || []).join(", "))}</td><td>${esc(driftDeviationText(finding.deviation))}</td></tr>`).join("")
    : `<tr><td colspan="8" class="empty">${esc(t("No drift recorded in this snapshot."))}</td></tr>`;
  const driftBlock = `<header class="consistency-head"><div><span class="section-kicker">${esc(t("DRIFT QUEUE"))}</span><h3>${esc(t("Drift queue"))}</h3><p class="panel-meta">${esc(t("Every recorded drift, separated by kind. A finding is a prompt to confirm, not a verdict on the code."))}</p></div><div class="consistency-filters" role="group" aria-label="${esc(t("Drift queue filters"))}"><label>${esc(t("Kind"))}<select data-consistency-drift>${options(consistencyDriftKinds, driftKind, driftKindLabels)}</select></label></div></header><div class="consistency-table-wrap" tabindex="0" role="region" aria-label="${esc(t("Drift queue"))}"><table class="consistency-table"><thead><tr><th scope="col">${esc(t("Finding"))}</th><th scope="col">${esc(t("Kind"))}</th><th scope="col">${esc(t("Risk"))}</th><th scope="col">${esc(t("Subject"))}</th><th scope="col">${esc(t("Message"))}</th><th scope="col">${esc(t("Affected requirements"))}</th><th scope="col">${esc(t("Paths"))}</th><th scope="col">${esc(t("Deviation"))}</th></tr></thead><tbody>${driftBody}</tbody></table></div>${drift?.truncated ? `<p class="panel-meta">${esc(t("Drift findings are truncated; the absence of a finding is not an all-clear."))}</p>` : ""}`;
  setHtml("consistencySection", els.consistencySection, `<header class="consistency-head"><div><span class="section-kicker">${esc(t("DESIGN CONSISTENCY"))}</span><h3>${esc(t("Design consistency"))}</h3><p class="panel-meta">${esc(t("Design intent, observed state and the delta between them. This matrix combines existing identities and does not replace the requirements or architecture views."))}</p></div><div class="consistency-filters" role="group" aria-label="${esc(t("Design consistency filters"))}"><label>${esc(t("Kind"))}<select data-consistency-kind>${options(consistencyKinds, kind, kindLabels)}</select></label><label>${esc(t("State"))}<select data-consistency-state>${options(consistencyStates, filter, stateLabels)}</select></label></div></header><div class="consistency-table-wrap" tabindex="0" role="region" aria-label="${esc(t("Design consistency matrix"))}"><table class="consistency-table"><thead><tr><th scope="col">${esc(t("Object"))}</th><th scope="col">${esc(t("Kind"))}</th><th scope="col">${esc(t("Design"))}</th><th scope="col">${esc(t("Observed"))}</th><th scope="col">${esc(t("Delta"))}</th></tr></thead><tbody>${body}</tbody></table></div>${driftBlock}`, () => {
    els.consistencySection.querySelectorAll("[data-consistency-kind]").forEach(select => select.addEventListener("change", () => { state.consistencyKind = select.value; renderConsistencyMatrix(); }));
    els.consistencySection.querySelectorAll("[data-consistency-state]").forEach(select => select.addEventListener("change", () => { state.consistencyState = select.value; renderConsistencyMatrix(); }));
    els.consistencySection.querySelectorAll("[data-consistency-drift]").forEach(select => select.addEventListener("change", () => { state.consistencyDriftKind = select.value; renderConsistencyMatrix(); }));
    // Both tables activate a row on click. A row is not a native control, so
    // it also needs a focus stop and Enter/Space to keep the keyboard path
    // visible; the label names the object it opens.
    const activateRow = (row, command, label) => {
      row.setAttribute("aria-label", label);
      row.addEventListener("keydown", event => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        executeCommand(command);
      });
    };
    els.consistencySection.querySelectorAll("[data-consistency-row]").forEach(row => {
      row.querySelectorAll("a,button").forEach(() => {});
      activateRow(row, row.dataset.consistencyRow, row.dataset.consistencyLabel || row.dataset.consistencyRow);
      row.addEventListener("click", () => executeCommand(row.dataset.consistencyRow));
    });
    els.consistencySection.querySelectorAll("[data-consistency-drift-row]").forEach(row => {
      activateRow(row, row.dataset.consistencyDriftRow, row.dataset.consistencyDriftLabel || row.dataset.consistencyDriftRow);
      row.addEventListener("click", () => executeCommand(row.dataset.consistencyDriftRow));
    });
  });
}
