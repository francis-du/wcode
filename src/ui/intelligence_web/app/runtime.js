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
  renderLive(); renderStats(); renderAttention(); renderArchitecture();
  renderRequirements(); renderDetail(); renderChanges(); renderProjectStructure();
  renderCodeStats(); renderRevisions(); renderLanguageQuality(); renderActivity(); renderProofSummary();
}
function revealSection(id) {
  const section = document.getElementById(id);
  if (!section) return;
  if (section.tagName === "DETAILS") section.open = true;
  section.scrollIntoView({ behavior: "smooth", block: "start" });
  section.focus({ preventScroll: true });
}
function clearWorkspaceView() {
  state.workspaceEpoch++;
  state.pendingValue = null; state.pendingApplied = 0;
  state.accessRead = null;
  state.syncError = false;
  state.project = null; state.selected = ""; state.selectedComponent = "";
  state.revisionKey = null; state.lastUpdated = 0; state.lastChecked = 0;
  state.activitySnapshot = null; state.activityUpdated = 0; state.activityError = false;
  state.access = null; state.workspaceAccess = null; state.authorizations = [];
  state.accessLoaded = false; state.accessEpoch++; state.semanticRefreshPending = false;
  els.search.value = ""; els.componentSearch.value = "";
  state.activityController?.abort(); state.pollController?.abort();
  for (const key of ["stats", "attention", "architectureMetrics", "architectureGraph", "componentCards", "componentInspector", "requirements", "detail", "changes", "fileTree", "largeFiles", "codeStats", "revisions", "languageQuality", "activity", "resourceStatus", "proofSummary"]) {
    setHtml(key, els[key], `<div class="section empty">${esc(t("Loading project state…"))}</div>`);
  }
  els.projectIdentity.textContent = state.current;
  els.lastUpdated.textContent = "—"; els.precisionBadge.textContent = "—";
  els.reqCount.textContent = "—"; els.componentCount.textContent = "—";
  els.structureSummary.textContent = "—"; els.qualitySummary.textContent = "—";
  renderAccess(true);
  setHtml("statusSummary", els.statusSummary, `<h2>${esc(t("Loading project state…"))}</h2>`);
}
const revisionKey = (revision) => `${revision.fingerprint || "full"}|${revision.graph_revision || ""}|${revision.proof_revision || ""}`;
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
    if (!observed) {
      try { observed = await uiJson("/intelligence/revision", "GET", undefined, options); }
      catch (error) { if (controller.signal.aborted || !current()) throw error; }
    }
    if (!current() || controller.signal.aborted) return false;
    const data = await uiJson("/intelligence/project", "GET", undefined, options);
    if (!current() || controller.signal.aborted) return false;
    if (typeof data.workspace !== "string" || (selectedWorkspace && data.workspace !== selectedWorkspace)) throw new Error("Workspace response mismatch");
    observePending(data.pending_authorizations, stamp);
    data.pending_authorizations = state.pendingValue;
    state.project = data; state.current = data.workspace;
    if (state.activitySnapshot && state.activitySnapshot.workspace !== data.workspace) state.activitySnapshot = null;
    state.lastUpdated = Date.now(); state.lastChecked = state.lastUpdated; state.syncError = false;
    // Only acknowledge a revision after its corresponding project fetch succeeds.
    state.revisionKey = observed ? revisionKey(observed) : null;
    if (state.selected && !data.requirements?.some(r => r.id === state.selected)) state.selected = "";
    renderProject(force);
    setSync("ok", localized("Snapshot up to date", "快照已更新"));
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
      // Git can be unavailable while the project and read-only activity still work.
      await refreshProject({ reason: "auto" });
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
    renderActivity(); if (state.project) { renderStats(); renderAttention(); }
  } catch (error) {
    if (!controller.signal.aborted && observationCurrent(stamp)) { state.activityError = true; renderActivity(); if (state.project) { renderStats(); renderAttention(); } }
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
function autoEnabled() { return els.auto.checked && !document.hidden; }
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
els.language.addEventListener("change", () => {
  state.language = els.language.value === "zh-CN" ? "zh-CN" : "en";
  savePreference("wcode.ui.language", state.language); applyLanguage();
});
els.theme.addEventListener("change", () => {
  state.theme = ["dark", "light"].includes(els.theme.value) ? els.theme.value : "system";
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
  await Promise.all([refreshProject({ reason: "manual", force: true }), refreshActivity()]);
  schedule(); if (accessPanelOpen()) await loadAccess();
});
els.refreshSemantic.addEventListener("click", refreshSemantics);
els.auto.addEventListener("change", schedule);
els.search.addEventListener("input", () => { invalidate("requirements", "detail"); renderRequirements(); renderDetail(); });
els.componentSearch.addEventListener("input", renderComponentCards);
document.querySelectorAll(".filter").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".filter").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.filter = button.dataset.filter; invalidate("requirements", "detail"); renderRequirements(); renderDetail();
}));
document.querySelectorAll(".arch-mode").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".arch-mode").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.architectureMode = button.dataset.archMode || "overlay"; invalidate("architectureGraph"); renderArchitectureGraph();
}));
document.querySelectorAll("[data-arch-view]").forEach(button => button.addEventListener("click", () => {
  state.architectureView = button.dataset.archView; renderArchitecture();
  document.querySelectorAll("[data-arch-view]").forEach(item => item.setAttribute("aria-pressed", String(item === button)));
}));
document.querySelectorAll("[data-jump]").forEach(button => button.addEventListener("click", () => revealSection(button.dataset.jump)));
document.addEventListener("keydown", event => { if (event.key === "Escape" && accessPanelOpen()) { event.preventDefault(); setAccessPanel(false); } });
window.addEventListener("resize", () => { if (accessPanelOpen()) setAccessPanel(true, false); });
systemThemeQuery.addEventListener?.("change", () => { if (state.theme === "system") applyTheme(); });
document.addEventListener("visibilitychange", () => {
  clearTimeout(state.timer); state.timer = null;
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (document.hidden) {
    state.pollController?.abort(); state.activityController?.abort(); state.controller?.abort();
  } else if (els.auto.checked) {
    void refreshTick(); void activityTick();
  }
  renderLive();
});
function startObservatory() {
  if (state.started) return;
  state.started = true;
  // Neither the initial render nor later project refreshes own the activity loop.
  void activityTick();
  return refreshProject({ workspace: state.current, reason: "initial", force: true }).then(scheduleProject);
}

applyTheme();
applyLanguage();
startObservatory();
