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
  if (!jobs.length) {
    // A scope change clears the list before the next read lands, so an unread
    // list must not claim the workspace has no jobs.
    return `<p class="drawer-empty">${esc(state.runtimeJobsLoaded
      ? t("No retained jobs in this snapshot")
      : t("Reading retained jobs for this workspace…"))}</p>`;
  }
  return jobs.map(job => `<button type="button" class="drawer-row${job.task_id === state.drawerJob ? " active" : ""}" data-drawer-action="select-job" data-drawer-job="${esc(job.task_id)}"><span class="drawer-row-mark job-${esc(job.status || "unknown")}"></span><span class="drawer-row-text"><strong>${esc(job.task_id)}</strong><small>${esc(localized(`status ${job.status || "unknown"} · origin ${job.origin || "unknown"}`, `状态 ${job.status || "未知"} · 来源 ${job.origin || "未知"}`))}</small></span></button>`).join("");
}

function drawerLogsHtml() {
  const job = state.runtimeJob;
  const openActivity = `<button type="button" class="quiet-action" data-drawer-action="open-view" data-drawer-view="activitySection">${esc(t("Open task activity"))}</button>`;
  if (!state.drawerJob) {
    return `<p class="drawer-empty">${esc(t("Select a job to read its retained output."))}</p>${openActivity}`;
  }
  // The header names the selected job, so the body must never keep showing a
  // different job's output while the selected one is still unread.
  if (!job || job.task_id !== state.drawerJob) {
    const notice = state.runtimeJobsMessage ? `<p class="drawer-notice">${esc(state.runtimeJobsMessage)}</p>` : "";
    return `<p class="drawer-empty">${esc(localized("Reading retained output for this job…", "正在读取该任务的保留输出…"))}</p>${notice}${openActivity}`;
  }
  // A bounded tail of the retained window, not the whole stream: the drawer is a
  // compact projection, and the activity view still owns the full output. The
  // drawer's own cut is stated with its position, so "last 12 lines" can never
  // read as the whole retained window.
  const drawerTailLines = 12;
  const tails = [];
  const drawerNotices = [];
  for (const name of ["stdout", "stderr"]) {
    const stream = job[name];
    if (!stream || typeof stream.text !== "string" || !stream.text) continue;
    const lines = stream.text.replace(/\r\n?/g, "\n").split("\n");
    if (lines.length > drawerTailLines) {
      drawerNotices.push(localized(`"${name}" shows the last ${drawerTailLines} of ${num(lines.length)} retained lines.`, `"${name}" 显示保留输出的最后 ${drawerTailLines} / ${num(lines.length)} 行。`));
    }
    tails.push(`<div class="drawer-log-stream">${esc(name)}</div><pre class="drawer-log">${esc(lines.slice(-drawerTailLines).join("\n"))}</pre>`);
  }
  const preview = tails.join("");
  const notices = [
    job.stdout?.truncated || job.stderr?.truncated ? t("Output was truncated before display; the tail is shown.") : "",
    job.stdout?.redacted || job.stderr?.redacted ? t("Output was redacted before display.") : "",
    ...drawerNotices,
  ].filter(Boolean).map(text => `<p class="drawer-notice">${esc(text)}</p>`).join("");
  return `<div class="drawer-log-head"><code>${esc(state.drawerJob)}</code><span>${esc(localized(`status ${job.status || "unknown"} · origin ${job.origin || "unknown"}`, `状态 ${job.status || "未知"} · 来源 ${job.origin || "未知"}`))}</span></div>`
    + (preview || `<p class="drawer-empty">${esc(t("No retained output for this job."))}</p>`)
    + notices
    + `<button type="button" class="quiet-action" data-drawer-action="open-view" data-drawer-view="activitySection">${esc(t("Open retained output"))}</button>`;
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
  // The drawer uses aria-pressed rather than a tab role: the workspace tabs are
  // the page's only tab set, and a second selected tab would break that contract.
  document.querySelectorAll("[data-drawer-section]").forEach(button => {
    const active = button.dataset.drawerSection === section;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  renderInvestigationTelemetry();
  if (els.drawerToggle) {
    els.drawerToggle.setAttribute("aria-expanded", String(!state.drawerCollapsed));
    els.drawerToggle.textContent = t(state.drawerCollapsed ? "Expand panel" : "Collapse panel");
  }
  els.investigationDrawer.classList.toggle("collapsed", Boolean(state.drawerCollapsed));
  if (els.drawerLabel && document.activeElement !== els.drawerLabel) {
    els.drawerLabel.value = state.drawerLabel || "";
  }
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
        const id = button.dataset.drawerJob;
        state.drawerJob = id;
        state.runtimeJobSelected = id;
        state.drawerSection = "logs";
        if (state.runtimeJob?.task_id !== id) state.runtimeJob = null;
        // Load the selected job's own retained output; the observation lands
        // after this render, so the drawer is repainted when it settles.
        void observeRuntimeJob(id, true, null).then(renderInvestigationDrawer);
      } else if (action === "open-view") {
        revealSection(button.dataset.drawerView);
      }
      renderInvestigationDrawer();
    }));
  });
}
