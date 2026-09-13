function renderWorkspaceOptions() {
  const options = (state.project?.workspace_options || []).map((workspace) =>
    `<option value="${esc(workspace.id)}" ${workspace.id === state.current ? "selected" : ""}>${esc(workspace.id)}</option>`
  ).join("");
  setHtml("workspaceOptions", els.workspace, options);
}
function renderProject(force = false) {
  if (!state.project) return;
  if (force) state.rendered.clear();
  renderWorkspaceOptions();
  renderLive(); renderStats(); renderAttention(); renderArchitecture(); renderTraceabilityMap(); renderChangeConvergenceMap(); renderProjectNavigator();
  renderRequirements(); renderDetail(); renderVerificationImpact(); renderChanges(); renderProjectStructure();
  renderCodeStats(); renderRevisions(); renderLanguageQuality(); renderActivity(); renderProofSummary(); renderAdaptiveVerification(); renderVerifiedLearning();
}
const workspaceTabForSection = {
  architectureSection: "architecture",
  overviewSection: "overview",
  activitySection: "activity",
  proofSection: "proof",
  engineeringSection: "overview",
  requirementsSection: "requirements",
  changesSection: "changes",
  filesSection: "files",
  diagnosticsSection: "overview",
  qualitySection: "overview",
};
function renderWorkspaceHero(tab) {
  const copy = {
    overview: [localized("ENGINEERING OBSERVATORY", "工程观测台"), localized("System overview", "系统总览"), localized("Current project health, engineering signals, diagnostics and language quality in one operational summary.", "把当前项目健康度、工程信号、诊断与语言质量汇总在一个运行视图中。")],
    architecture: [localized("SYSTEM ARCHITECTURE", "系统架构"), localized("Engineering architecture", "工程架构"), localized("A hierarchical view of the software system, from high-level orchestration to foundational infrastructure.", "从高层编排到底层基础设施，按层次理解整个软件系统。")],
    activity: [localized("LIVE WORK", "实时工作"), localized("Task activity", "任务活动"), localized("Running work, queue pressure and execution time without mixing waiting time into runtime.", "区分排队与执行时间，查看实时工作与资源压力。")],
    proof: [localized("REVISION-BOUND PROOF", "版本绑定证据"), localized("Verification evidence", "验证证据"), localized("Current-revision evidence, verification readiness and adaptive checks kept separate from historical passes.", "当前版本证据、验证就绪度与自适应检查，与历史通过记录分开呈现。")],
    requirements: [localized("DESIRED STATE", "目标状态"), localized("Requirements", "需求"), localized("Trace intent through implementation, acceptance evidence and convergence.", "把需求意图追踪到实现、验收证据与收敛状态。")],
    changes: [localized("WORKING TREE", "工作树"), localized("Current changes", "当前变更"), localized("Map current edits back to components, requirements and verification impact.", "把当前修改映射回组件、需求与验证影响。")],
    files: [localized("REPOSITORY", "仓库"), localized("Project files", "项目文件"), localized("Browse the bounded repository structure and the files that dominate maintenance cost.", "浏览有界仓库结构以及维护成本最高的文件。")],
  }[tab] || [];
  if (els.workspaceKicker) els.workspaceKicker.textContent = copy[0] || "";
  if (els.workspaceTitle) els.workspaceTitle.textContent = copy[1] || "";
  if (els.workspaceSubtitle) els.workspaceSubtitle.textContent = copy[2] || "";
}
function activateWorkspaceTab(tab, { scroll = false } = {}) {
  const valid = ["overview", "architecture", "activity", "proof", "changes", "requirements", "files"];
  const next = valid.includes(tab) ? tab : "architecture";
  state.workspaceTab = next;
  renderWorkspaceHero(next);
  let activeTabButton = null;
  document.querySelectorAll("[data-workspace-tab]").forEach(button => {
    const active = button.dataset.workspaceTab === next;
    button.setAttribute("aria-selected", String(active));
    button.tabIndex = active ? 0 : -1;
    if (active) activeTabButton = button;
  });
  activeTabButton?.scrollIntoView({ block: "nearest", inline: "nearest" });
  document.querySelectorAll("[data-workspace-panel]").forEach(panel => {
    const active = panel.dataset.workspacePanel === next;
    panel.classList.toggle("hidden", !active);
  });
  document.querySelector(".drawer-stack")?.classList.toggle("hidden", next === "architecture");
  if (next !== "architecture" && els.componentInspector?.classList.contains("open")) {
    els.componentInspector.classList.remove("open");
  } else if (next === "architecture" && !state.systemMapFull) {
    const hasSelection = state.architectureView === "blueprint" ? state.selectedSubsystem : state.selectedComponent;
    if (hasSelection) els.componentInspector?.classList.add("open");
  }
  if (scroll) document.querySelector(".observatory-main")?.scrollIntoView({ behavior: "smooth", block: "start" });
}
function revealSection(id) {
  const section = document.getElementById(id);
  if (!section) return;
  activateWorkspaceTab(workspaceTabForSection[id] || state.workspaceTab);
  section.scrollIntoView({ behavior: "smooth", block: "start" });
  section.focus({ preventScroll: true });
}
function clearWorkspaceView() {
  state.workspaceEpoch++;
  state.pendingValue = null; state.pendingApplied = 0;
  state.accessRead = null;
  state.syncError = false;
  state.project = null; state.selected = ""; state.selectedComponent = ""; state.selectedSubsystem = ""; state.selectedEvidenceKey = ""; state.evidenceInspectorOpen = true;
  state.systemMapScale = 1; state.systemMapFit = true; state.systemMapFull = false;
  state.revisionKey = null; state.lastUpdated = 0; state.lastChecked = 0;
  state.activitySnapshot = null; state.activityUpdated = 0; state.activityError = false;
  if (els.projectNavigator) els.projectNavigator.value = "";
  if (els.navigatorResults) els.navigatorResults.classList.add("hidden");
  state.tunnelSnapshot = null; state.tunnelBusy = false;
  state.access = null; state.workspaceAccess = null; state.authorizations = [];
  state.accessLoaded = false; state.accessEpoch++; state.semanticRefreshPending = false;
  els.search.value = ""; els.componentSearch.value = "";
  state.activityController?.abort(); state.pollController?.abort();
  for (const key of ["stats", "attention", "architectureBlueprint", "engineeringFlow", "changeStory", "runtimeTopology", "engineeringTimeline", "traceabilityMap", "changeConvergenceMap", "architectureGraph", "componentCards", "componentInspector", "requirements", "detail", "verificationImpact", "changes", "fileTree", "largeFiles", "codeStats", "revisions", "languageQuality", "activity", "resourceStatus", "proofSummary", "adaptiveVerification", "verifiedLearning"]) {
    setHtml(key, els[key], `<div class="section empty">${esc(t("Loading project state…"))}</div>`);
  }
  els.lastUpdated.textContent = "—"; els.precisionBadge.textContent = "—";
  if (els.precisionProviders) els.precisionProviders.textContent = "—";
  els.reqCount.textContent = "—"; els.componentCount.textContent = "—";
  els.structureSummary.textContent = "—"; els.qualitySummary.textContent = "—";
  renderAccess(true);
  setHtml("statusSummary", els.statusSummary, `<h2>${esc(t("Loading project state…"))}</h2>`);
}
const revisionKey = (revision) => `${revision.fingerprint || "full"}|${revision.graph_revision || ""}|${revision.proof_revision || ""}|${revision.engineering_revision || ""}`;
async function refreshProject({ workspace, reason = "auto", force = false, revision } = {}) {
  if (workspace !== undefined && workspace !== state.current) {
    state.current = workspace; clearWorkspaceView(); force = true;
  }
  if (state.inFlight && reason === "auto") return false;
  state.controller?.abort();
  // Reserve the epoch before the first await, including the revision request.
  const controller = new AbortController(), epoch = ++state.requestEpoch, selectedWorkspace = state.current;
  state.controller = controller; state.inFlight = true;
  const stamp = observationStamp();
  const current = () => epoch === state.requestEpoch && observationCurrent(stamp);
  const options = { workspace: selectedWorkspace, signal: controller.signal };
  setSync("loading", t("Refreshing project state…"));
  try {
    let observed = revision;
    const revisionRequest = observed ? null : uiJson("/intelligence/revision", "GET", undefined, options).catch(() => null);
    const data = await uiJson("/intelligence/project", "GET", undefined, options);
    if (!current() || controller.signal.aborted) return false;
    if (typeof data.workspace !== "string" || (selectedWorkspace && data.workspace !== selectedWorkspace)) throw new Error("Workspace response mismatch");
    observePending(data.pending_authorizations, stamp);
    data.pending_authorizations = state.pendingValue;
    state.project = data; state.current = data.workspace;
    if (state.activitySnapshot && state.activitySnapshot.workspace !== data.workspace) state.activitySnapshot = null;
    state.lastUpdated = Date.now(); state.lastChecked = state.lastUpdated; state.syncError = false;
    // Render the usable project snapshot as soon as it is ready. The initial
    // revision signal runs concurrently and only seeds the later poll baseline;
    // a late response is generation/workspace checked before it can mutate state.
    state.revisionKey = observed ? revisionKey(observed) : null;
    if (state.selected && !data.requirements?.some(r => r.id === state.selected)) state.selected = "";
    renderProject(force);
    setSync("ok", localized("Snapshot up to date", "快照已更新"));
    if (revisionRequest) {
      void revisionRequest.then(nextRevision => {
        if (!nextRevision || !current() || controller.signal.aborted) return;
        state.revisionKey = revisionKey(nextRevision);
      });
    }
    return true;
  } catch (error) {
    if (current() && !controller.signal.aborted) {
      state.syncError = true;
      setSync("error", `${t("Refresh failed")} · ${error.message}`);
      renderAttention(); renderLive();
      if (!state.project) setHtml("statusSummary", els.statusSummary, `<h2>${esc(t("Refresh failed"))}</h2><p>${esc(error.message)}</p>`);
    }
    return false;
  } finally {
    if (state.controller === controller) { state.inFlight = false; state.controller = null; els.refresh.disabled = false; }
  }
}
async function pollRevision() {
  if (state.pollController || state.inFlight) return;
  const controller = new AbortController(), workspace = state.current, epoch = state.requestEpoch;
  const stamp = observationStamp();
  state.pollController = controller;
  try {
    const revision = await uiJson("/intelligence/revision", "GET", undefined, { workspace, signal: controller.signal });
    if (controller.signal.aborted || workspace !== state.current || epoch !== state.requestEpoch) return;
    if (revision.workspace && revision.workspace !== workspace) throw new Error("Workspace response mismatch");
    if (state.project && typeof revision.pending_authorizations === "number") {
      const accepted = observePending(revision.pending_authorizations, stamp);
      // A rejected, pre-mutation observation must not invalidate a newer list.
      if (accepted && state.authorizations.length !== revision.pending_authorizations) state.accessLoaded = false;
      renderStats(); renderAttention();
    }
    if (!state.project || state.syncError || revision.full_refresh_required || revisionKey(revision) !== state.revisionKey) {
      await refreshProject({ reason: "auto", revision });
    } else {
      state.lastChecked = Date.now(); renderLive();
      setSync("ok", localized("Snapshot up to date", "快照已更新"));
    }
  } catch (error) {
    if (!controller.signal.aborted && workspace === state.current && epoch === state.requestEpoch) {
      if (!state.project) {
        // Initial loading can still obtain the project snapshot when the cheap
        // revision signal is unavailable.
        await refreshProject({ reason: "auto" });
      } else {
        // Keep the last known snapshot visible and retry the cheap signal on
        // the next poll. A successful later signal sees syncError and forces
        // exactly one full refresh, without amplifying a signal outage into a
        // heavy project rebuild every interval.
        state.syncError = true;
        state.lastChecked = Date.now();
        setSync("error", `${t("Refresh failed")} · ${error.message}`);
        renderAttention(); renderLive();
      }
    }
  } finally { if (state.pollController === controller) state.pollController = null; }
}
async function refreshActivity() {
  if (state.activityController) return;
  const controller = new AbortController(), workspace = state.current;
  const stamp = observationStamp();
  state.activityController = controller;
  try {
    const data = await uiJson("/intelligence/activity", "GET", undefined, { workspace, signal: controller.signal, timeout: 10000 });
    if (controller.signal.aborted || !observationCurrent(stamp)) return;
    if (typeof data.workspace !== "string" || (workspace && data.workspace !== workspace)) throw new Error("Workspace response mismatch");
    observePending(data.pending_authorizations, stamp);
    state.activitySnapshot = data; state.activityUpdated = Date.now(); state.activityError = false;
    if (state.project) state.project.activity = data.activity;
    renderActivity(); if (state.project) { renderStats(); renderAttention(); renderEngineeringFlow(); renderChangeConvergenceMap(); renderRuntimeTopology(); renderEngineeringTimeline(); }
  } catch (error) {
    if (!controller.signal.aborted && observationCurrent(stamp)) { state.activityError = true; renderActivity(); if (state.project) { renderStats(); renderAttention(); renderEngineeringFlow(); renderChangeConvergenceMap(); renderRuntimeTopology(); renderEngineeringTimeline(); } }
  } finally { if (state.activityController === controller) state.activityController = null; }
}
async function refreshSemantics() {
  if (els.refreshSemantic.disabled) return;
  const stamp = observationStamp(), workspace = stamp.workspace;
  els.refreshSemantic.disabled = true;
  setSync("loading", t("Syncing"));
  try {
    const result = await uiJson("/intelligence/semantic-refresh", "POST", {}, { workspace, timeout: 120000 });
    if (!observationCurrent(stamp)) return;
    state.semanticRefreshPending = false; state.revisionKey = null;
    await refreshProject({ reason: "manual", force: true });
    if (!observationCurrent(stamp)) return;
    if (Array.isArray(result.failures) && result.failures.length) {
      setSync("warn", localized(`${result.failures.length} semantic refresh failures; inspect provider details`, `${result.failures.length} 项语义刷新失败，请查看分析器详情`));
      revealSection("qualitySection");
    }
  } catch (error) {
    if (!observationCurrent(stamp)) return;
    if (error.message.includes("authorization required")) {
      state.semanticRefreshPending = true; setAccessPanel(true); await loadAccess();
      if (observationCurrent(stamp)) setSync("warn", t("Semantic refresh needs approval"));
    } else setSync("error", `${t("Refresh failed")} · ${error.message}`);
  } finally { els.refreshSemantic.disabled = false; }
}
function autoEnabled() { return state.autoRefresh && !document.hidden; }
function activityInterval() {
  const activity = state.activitySnapshot?.activity;
  return !state.activityError && activity?.available === true &&
    (activity.active > 0 || activity.queued > 0 || pendingCount() > 0) ? 2000 : 8000;
}
function scheduleProject() {
  clearTimeout(state.timer); state.timer = null;
  if (autoEnabled() && !state.projectTickActive) state.timer = setTimeout(refreshTick, 8000);
}
function scheduleActivity() {
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (autoEnabled() && !state.activityTickActive) state.activityTimer = setTimeout(activityTick, activityInterval());
}
async function refreshTick() {
  clearTimeout(state.timer); state.timer = null;
  if (!autoEnabled() || state.projectTickActive) return;
  state.projectTickActive = true;
  try { await pollRevision(); }
  finally { state.projectTickActive = false; scheduleProject(); }
}
async function activityTick() {
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (!autoEnabled() || state.activityTickActive) return;
  state.activityTickActive = true;
  try {
    await refreshActivity();
    void refreshTunnels();
    if (autoEnabled() && accessPanelOpen() && !state.accessBusy) await loadAccess();
  } finally { state.activityTickActive = false; scheduleActivity(); }
}
function schedule() {
  scheduleProject(); scheduleActivity(); renderLive();
}
els.workspace.addEventListener("change", async () => {
  const refresh = refreshProject({ workspace: els.workspace.value, reason: "manual", force: true });
  void activityTick();
  await refresh; scheduleProject();
  if (accessPanelOpen()) await loadAccess();
});
els.language.addEventListener("click", () => {
  state.language = state.language === "zh-CN" ? "en" : "zh-CN";
  savePreference("wcode.ui.language", state.language); applyLanguage();
});
els.theme.addEventListener("click", () => {
  state.theme = state.theme === "system" ? "dark" : state.theme === "dark" ? "light" : "system";
  savePreference("wcode.ui.theme", state.theme); applyTheme();
});
els.manage.addEventListener("click", async () => { const open = !accessPanelOpen(); setAccessPanel(open); if (open) await loadAccess(); });
els.closeAccess.addEventListener("click", () => setAccessPanel(false));
els.addWorkspace.addEventListener("click", addWorkspaceFromUi);
els.workspacePath.addEventListener("keydown", event => { if (event.key === "Enter") addWorkspaceFromUi(); });
els.addCommand.addEventListener("click", addCommandFromUi);
els.commandCandidate.addEventListener("keydown", event => { if (event.key === "Enter") addCommandFromUi(); });
els.authorizeOperation.addEventListener("click", authorizeOperationFromUi);
els.operationArgs.addEventListener("keydown", event => { if (event.key === "Enter") authorizeOperationFromUi(); });
els.refresh.addEventListener("click", async () => {
  await Promise.all([refreshProject({ reason: "manual", force: true }), refreshActivity(), refreshTunnels()]);
  schedule(); if (accessPanelOpen()) await loadAccess();
});
els.refreshSemantic.addEventListener("click", refreshSemantics);
els.auto.addEventListener("click", () => {
  state.autoRefresh = !state.autoRefresh;
  applyAutoRefreshControl();
  schedule();
});
els.search.addEventListener("input", () => { invalidate("requirements", "detail"); renderRequirements(); renderDetail(); });
els.componentSearch.addEventListener("input", () => { renderComponentCards(); renderComponentInspector(); });
els.projectNavigator?.addEventListener("input", renderProjectNavigator);
els.projectNavigator?.addEventListener("keydown", event => {
  const results = [...(els.navigatorResults?.querySelectorAll("[data-nav-index]") || [])];
  if (event.key === "Escape") {
    els.projectNavigator.value = ""; renderProjectNavigator(); els.projectNavigator.blur();
  } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && results.length) {
    event.preventDefault();
    const current = results.findIndex(item => item.getAttribute("aria-selected") === "true"),
      next = event.key === "Home" ? 0 : event.key === "End" ? results.length - 1 : event.key === "ArrowDown" ? (current + 1 + results.length) % results.length : (current - 1 + results.length) % results.length;
    results.forEach((item, index) => item.setAttribute("aria-selected", String(index === next)));
    if (results[next]?.id) els.projectNavigator.setAttribute("aria-activedescendant", results[next].id);
    results[next]?.scrollIntoView({ block: "nearest" });
  } else if (event.key === "Enter") {
    const selected = results.find(item => item.getAttribute("aria-selected") === "true") || results[0];
    selected?.click();
  }
});
document.addEventListener("keydown", event => {
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
  if (((event.key === "/" && !event.metaKey && !event.ctrlKey && !event.altKey) || (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey))) && !typing) {
    event.preventDefault(); els.projectNavigator?.focus();
  }
});
document.addEventListener("click", event => {
  if (!event.target.closest(".project-navigator")) {
    els.navigatorResults?.classList.add("hidden");
    els.projectNavigator?.setAttribute("aria-expanded", "false");
    els.projectNavigator?.removeAttribute("aria-activedescendant");
  }
});
document.querySelectorAll(".filter").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".filter").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.filter = button.dataset.filter; invalidate("requirements", "detail"); renderRequirements(); renderDetail();
}));
document.querySelectorAll(".arch-mode").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".arch-mode").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.architectureMode = button.dataset.archMode || "overlay"; invalidate("architectureGraph"); renderArchitectureGraph();
}));
document.querySelectorAll("[data-arch-back]").forEach(button => button.addEventListener("click", () => {
  state.architectureView = button.dataset.archBack || "blueprint"; renderArchitecture();
}));
els.systemMapFit?.addEventListener("click", fitSystemMap);
els.systemMapZoomOut?.addEventListener("click", () => setSystemMapScale(state.systemMapScale - .1));
els.systemMapZoomIn?.addEventListener("click", () => setSystemMapScale(state.systemMapScale + .1));
els.systemMapFull?.addEventListener("click", () => setSystemMapFull(!state.systemMapFull));
document.querySelectorAll("[data-workspace-tab]").forEach(button => {
  button.addEventListener("click", () => activateWorkspaceTab(button.dataset.workspaceTab, { scroll: true }));
  button.addEventListener("keydown", event => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const tabs = [...document.querySelectorAll("[data-workspace-tab]")], index = tabs.indexOf(button);
    const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : event.key === "ArrowRight" ? (index + 1) % tabs.length : (index - 1 + tabs.length) % tabs.length;
    event.preventDefault();
    tabs[nextIndex]?.focus();
    activateWorkspaceTab(tabs[nextIndex]?.dataset.workspaceTab, { scroll: true });
  });
});
document.addEventListener("keydown", event => {
  if (event.key !== "Escape") return;
  if (accessPanelOpen()) { event.preventDefault(); setAccessPanel(false); return; }
  if (els.componentInspector?.classList.contains("open")) {
    event.preventDefault();
    if (state.architectureView === "blueprint") setSystemMapFull(true);
    else {
      state.selectedComponent = "";
      els.componentInspector.classList.remove("open");
    }
  }
});
window.addEventListener("resize", () => {
  if (accessPanelOpen()) setAccessPanel(true, false);
  if (state.systemMapFit && state.architectureView === "blueprint") requestAnimationFrame(fitSystemMap);
});
systemThemeQuery.addEventListener?.("change", () => { if (state.theme === "system") applyTheme(); });
document.addEventListener("visibilitychange", () => {
  clearTimeout(state.timer); state.timer = null;
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (document.hidden) {
    state.pollController?.abort(); state.activityController?.abort(); state.controller?.abort();
  } else if (state.autoRefresh) {
    void refreshTick(); void activityTick();
  }
  renderLive();
});
function startObservatory() {
  if (state.started) return;
  state.started = true;
  activateWorkspaceTab(state.workspaceTab);
  // Neither the initial render nor later project refreshes own the activity loop.
  void refreshTunnels();
  void activityTick();
  return refreshProject({ workspace: state.current, reason: "initial", force: true }).then(scheduleProject);
}

applyTheme();
applyLanguage();
startObservatory();
