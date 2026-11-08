// Design-vs-actual matrix. One row per design object carries the design
// statement, the observed state and the delta between them, so a drift can be
// traced from intent to implementation without switching views. It combines
// existing identities; it does not redefine them.
const consistencyKinds = ["all", "requirement", "component", "constraint", "acceptance"];
const consistencyStates = ["all", "needs_attention", "stable", "unobserved"];

const consistencyFindings = () => (state.project?.risk?.drift?.findings || [])
  .filter(item => item.kind === "runtime_drift");

const consistencyRows = () => {
  const project = state.project || {}, design = project.design || {};
  const findings = consistencyFindings();
  const rows = [];
  for (const requirement of (project.requirements || [])) {
    const dependencies = requirement.dependency_alignment || [];
    const blocking = dependencies.filter(dependency => dependency.blocking).length;
    const needs = requirement.convergence === "needs_convergence" || blocking > 0;
    // A drift affecting this requirement is only actionable next to the
    // convergence plan that claims it; an unclaimed drift says so instead.
    const affecting = findings.filter(finding => (finding.affected_requirements || []).includes(requirement.id));
    const plans = affecting.length
      ? (project.reconciliation || []).filter(plan => (plan.drift_ids || []).some(id => affecting.some(finding => finding.id === id)))
      : [];
    const convergence = !affecting.length ? ""
      : !plans.length ? [t("no convergence plan references this drift")]
        : plans.map(plan => {
          const tasks = plan.implementation_tasks || [];
          const execution = project.reconciliation_execution;
          const matched = execution?.execution?.plan_id === plan.id ? execution : null;
          const detail = matched
            ? `${matched.pending} ${t("pending")} · ${matched.completed} ${t("completed")} · ${matched.failed} ${t("failed")} · ${matched.converged ? t("converged") : t("not converged")}`
            : unit(tasks.length, "task", "tasks", "个任务");
          return `${plan.id} (${detail})`;
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
    rows.push({
      id: component.id,
      kind: "component",
      design: [unit((component.responsibilities || []).length, "responsibility", "responsibilities", "项职责"),
        unit((component.depends_on || []).length, "declared dependency", "declared dependencies", "条声明依赖")].join(" · "),
      actual: [statusLabel(tone),
        unit(component.implementation_files || 0, "mapped file", "mapped files", "个映射文件"),
        changed ? t("changed") : t("unchanged")].join(" · "),
      state: tone === "drift" || tone === "uncertain" ? "needs_attention" : changed ? "stable" : "stable",
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
    rows.push({
      id: check.id,
      kind: "acceptance",
      design: criteria.length
        ? criteria.map(criterion => criterion.title || criterion.id).join(" / ")
        : t("no acceptance criterion references this check"),
      actual: [mapped ? t("mapped") : t("not mapped"),
        `${t("Required")}: ${check.required_level || "—"}`,
        check.level ? `${t("Reported")}: ${check.level}` : "",
        `${t("Execution")}: ${check.execution || "—"}`,
        `${t("Outcome")}: ${check.outcome || "—"}`,
        `${t("Freshness")}: ${check.freshness || "—"}`].filter(Boolean).join(" · "),
      state: !mapped || !executed || failed ? "needs_attention" : !fresh ? "needs_attention" : "stable",
      delta,
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
  const body = rows.length ? rows.map(row => `<tr data-consistency-row="${esc(row.navigate)}"${row.state === "needs_attention" ? ' class="attention"' : ""}><th scope="row"><code>${esc(row.id)}</code></th><td>${esc(t({ requirement: "Requirement", component: "Component", constraint: "Constraint", acceptance: "Acceptance" }[row.kind]))}</td><td>${esc(row.design)}</td><td>${esc(row.actual)}</td><td>${esc(row.delta || stateLabels(row.state))}</td></tr>`).join("")
    : `<tr><td colspan="5" class="empty">${esc(t("No design object matches this filter."))}</td></tr>`;
  setHtml("consistencySection", els.consistencySection, `<header class="consistency-head"><div><span class="section-kicker">${esc(t("DESIGN CONSISTENCY"))}</span><h3>${esc(t("Design consistency"))}</h3><p class="panel-meta">${esc(t("Design intent, observed state and the delta between them. This matrix combines existing identities and does not replace the requirements or architecture views."))}</p></div><div class="consistency-filters" role="group" aria-label="${esc(t("Design consistency filters"))}"><label>${esc(t("Kind"))}<select data-consistency-kind>${options(consistencyKinds, kind, kindLabels)}</select></label><label>${esc(t("State"))}<select data-consistency-state>${options(consistencyStates, filter, stateLabels)}</select></label></div></header><div class="consistency-table-wrap" tabindex="0" role="region" aria-label="${esc(t("Design consistency matrix"))}"><table class="consistency-table"><thead><tr><th scope="col">${esc(t("Object"))}</th><th scope="col">${esc(t("Kind"))}</th><th scope="col">${esc(t("Design"))}</th><th scope="col">${esc(t("Observed"))}</th><th scope="col">${esc(t("Delta"))}</th></tr></thead><tbody>${body}</tbody></table></div>`, () => {
    els.consistencySection.querySelectorAll("[data-consistency-kind]").forEach(select => select.addEventListener("change", () => { state.consistencyKind = select.value; renderConsistencyMatrix(); }));
    els.consistencySection.querySelectorAll("[data-consistency-state]").forEach(select => select.addEventListener("change", () => { state.consistencyState = select.value; renderConsistencyMatrix(); }));
    els.consistencySection.querySelectorAll("[data-consistency-row]").forEach(row => row.addEventListener("click", () => {
      row.querySelectorAll("a,button").forEach(() => {});
      executeCommand(row.dataset.consistencyRow);
    }));
  });
}
