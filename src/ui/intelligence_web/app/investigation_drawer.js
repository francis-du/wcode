// A persistent bottom panel over the same facts the views own. It keeps the
// problems, jobs, logs and evidence being tracked visible while the main view
// changes; each section is a compact projection, and the full detail stays in
// its own view rather than being copied here.
const drawerSections = ["problems", "jobs", "logs", "evidence"];

function drawerProblemsHtml() {
  const signals = attentionSignals();
  if (!signals.length) return `<p class="drawer-empty">${esc(t("No known blockers in this snapshot"))}</p>`;
  return signals.slice(0, 10).map(signal => `<button type="button" class="drawer-row" data-drawer-action="run" data-summary-action="${esc(signal.target)}" data-summary-path="${esc(signal.path || "")}" data-summary-requirement="${esc(signal.requirement || "")}"><span class="drawer-row-mark signal-${esc(signal.tone)}"></span><span class="drawer-row-text"><strong>${esc(signal.title)}</strong><small>${esc(signal.detail)}</small></span></button>`).join("")
    + (signals.length > 10 ? `<p class="drawer-more">${esc(localized(`${num(signals.length - 10)} more observations`, `另有 ${num(signals.length - 10)} 项观测`))}</p>` : "");
}

function drawerJobsHtml() {
  const jobs = (state.runtimeJobs || []).slice(0, 10);
  if (!jobs.length) return `<p class="drawer-empty">${esc(t("No retained jobs in this snapshot"))}</p>`;
  return jobs.map(job => `<button type="button" class="drawer-row${job.task_id === state.drawerJob ? " active" : ""}" data-drawer-action="select-job" data-drawer-job="${esc(job.task_id)}"><span class="drawer-row-mark job-${esc(job.status || "unknown")}"></span><span class="drawer-row-text"><strong>${esc(job.task_id)}</strong><small>${esc(localized(`status ${job.status || "unknown"} · origin ${job.origin || "unknown"}`, `状态 ${job.status || "未知"} · 来源 ${job.origin || "未知"}`))}</small></span></button>`).join("");
}

function drawerLogsHtml() {
  const job = state.runtimeJob;
  if (!state.drawerJob || !job) {
    return `<p class="drawer-empty">${esc(t("Select a job to read its retained output."))}</p><button type="button" class="quiet-action" data-drawer-action="open-view" data-drawer-view="activitySection">${esc(t("Open task activity"))}</button>`;
  }
  return `<div class="drawer-log-head"><code>${esc(state.drawerJob)}</code><span>${esc(localized(`status ${job.status || "unknown"} · origin ${job.origin || "unknown"}`, `状态 ${job.status || "未知"} · 来源 ${job.origin || "未知"}`))}</span></div><p class="drawer-empty">${esc(t("Retained output is read in the activity view; this panel keeps the selection while you change views."))}</p><button type="button" class="quiet-action" data-drawer-action="open-view" data-drawer-view="activitySection">${esc(t("Open retained output"))}</button>`;
}

function drawerEvidenceHtml() {
  const proof = effectiveProof();
  const acceptance = acceptanceView();
  const rows = [
    [`${num(proof.current_evidence || 0)}`, localized("current-version evidence", "当前版本证据")],
    [`${num(proof.current_failed || 0)}`, localized("effective failures", "有效失败")],
    [`${num(proof.current_inconclusive || 0)}`, localized("inconclusive", "未定结论")],
    [acceptance.record ? statusLabel(acceptance.status) : "—", localized("canonical acceptance", "统一验收结论")],
  ];
  return `<div class="drawer-kv">${rows.map(([value, label]) => `<span><i>${esc(label)}</i><b>${esc(value)}</b></span>`).join("")}</div>`;
}

function renderInvestigationDrawer() {
  if (!els.investigationDrawer || !els.drawerBody) return;
  const section = drawerSections.includes(state.drawerSection) ? state.drawerSection : "problems";
  state.drawerSection = section;
  document.querySelectorAll("[data-drawer-section]").forEach(button => {
    const active = button.dataset.drawerSection === section;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });
  if (els.drawerToggle) {
    els.drawerToggle.setAttribute("aria-expanded", String(!state.drawerCollapsed));
    els.drawerToggle.textContent = t(state.drawerCollapsed ? "Expand panel" : "Collapse panel");
  }
  els.investigationDrawer.classList.toggle("collapsed", Boolean(state.drawerCollapsed));
  const body = section === "problems" ? drawerProblemsHtml()
    : section === "jobs" ? drawerJobsHtml()
      : section === "logs" ? drawerLogsHtml()
        : drawerEvidenceHtml();
  setHtml("drawerBody", els.drawerBody, `<div class="drawer-section-body">${body}</div>`, () => {
    els.drawerBody.querySelectorAll("[data-drawer-action]").forEach(button => button.addEventListener("click", () => {
      const action = button.dataset.drawerAction;
      if (action === "run") {
        void runSummaryAction({
          action: button.dataset.summaryAction,
          path: button.dataset.summaryPath,
          requirement: button.dataset.summaryRequirement,
        });
      } else if (action === "select-job") {
        state.drawerJob = button.dataset.drawerJob;
        state.runtimeJobSelected = state.drawerJob;
        state.drawerSection = "logs";
      } else if (action === "open-view") {
        revealSection(button.dataset.drawerView);
      }
      renderInvestigationDrawer();
    }));
  });
}
