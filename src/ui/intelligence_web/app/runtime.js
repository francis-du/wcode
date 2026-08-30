function renderWorkspaceOptions() {
  const options = (state.project?.workspace_options || []).map((workspace) =>
    `<option value="${esc(workspace.id)}" ${
      workspace.id === state.current ? "selected" : ""
    }>${esc(workspace.id)} — ${esc(workspace.root || "")}</option>`
  ).join("");
  setHtml("workspaceOptions", els.workspace, options);
}
function renderProject(force = false) {
  if (!state.project) return;
  if (force) state.rendered.clear();
  renderWorkspaceOptions();
  renderLive();
  renderStats();
  renderAttention();
  renderArchitecture();
  renderRequirements();
  renderDetail();
  renderChanges();
  renderProjectStructure();
  renderCodeStats();
  renderRevisions();
  renderLanguageQuality();
}

const revisionKey = (revision) =>
  `${revision.fingerprint || "full"}|${revision.graph_revision || ""}`;
async function primeRevision() {
  try {
    const revision = await uiJson("/intelligence/revision");
    state.revisionKey = revisionKey(revision);
    return true;
  } catch {
    return false;
  }
}
async function refreshProject(
  { workspace, reason = "auto", force = false } = {},
) {
  if (workspace !== undefined && workspace !== state.current) {
    state.current = workspace;
    state.selected = "";
    state.selectedComponent = "";
    state.revisionKey = null;
    force = true;
  }
  if (state.inFlight) {
    if (reason === "auto") return;
    if (state.controller) state.controller.abort();
  }
  if (reason !== "auto" && state.revisionKey === null) await primeRevision();
  const controller = new AbortController();
  state.controller = controller;
  const epoch = ++state.requestEpoch;
  state.inFlight = true;
  setSync(
    "loading",
    reason === "auto" ? t("Syncing") : t("Refreshing project state…"),
  );
  try {
    const response = await fetch("/intelligence/project", {
      headers: requestHeaders(),
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const data = await response.json();
    if (epoch !== state.requestEpoch) return;
    state.project = data;
    state.current = data.workspace;
    state.lastUpdated = Date.now();
    if (
      state.selected && !data.requirements?.some((r) => r.id === state.selected)
    ) state.selected = "";
    renderProject(force);
    setSync("ok", t("Live"));
  } catch (error) {
    if (error.name !== "AbortError") {
      setSync("error", `${t("Refresh failed")} · ${error.message}`);
    }
  } finally {
    if (state.controller === controller) {
      state.inFlight = false;
      state.controller = null;
    }
  }
}
async function pollRevision() {
  try {
    const revision = await uiJson("/intelligence/revision");
    if (
      state.project &&
      Number(state.project.pending_authorizations || 0) !==
        Number(revision.pending_authorizations || 0)
    ) {
      state.project.pending_authorizations = Number(
        revision.pending_authorizations || 0,
      );
      renderAttention();
    }
    const key = revisionKey(revision);
    if (state.revisionKey === null) {
      state.revisionKey = key;
      await refreshProject({ reason: "auto" });
      return;
    }
    if (revision.full_refresh_required || key !== state.revisionKey) {
      state.revisionKey = key;
      await refreshProject({ reason: "auto" });
      if (state.project) {
        const latestGraph = state.project.history?.[0]?.id ||
          revision.graph_revision || "";
        state.revisionKey = `${revision.fingerprint || "full"}|${latestGraph}`;
      }
    } else setSync("ok", t("Live"));
  } catch (error) {
    await refreshProject({ reason: "auto" });
  }
}
async function refreshSemantics() {
  els.refreshSemantic.disabled = true;
  setSync("loading", t("Syncing"));
  try {
    await uiJson("/intelligence/semantic-refresh", "POST", {});
    state.semanticRefreshPending = false;
    setSync("ok", t("Semantic refresh complete"));
    state.revisionKey = null;
    await refreshProject({ reason: "manual", force: true });
  } catch (error) {
    if (error.message.includes("authorization required")) {
      state.semanticRefreshPending = true;
      els.accessPanel.classList.remove("hidden");
      await loadAccess();
      setSync("warn", t("Semantic refresh needs approval"));
    } else {
      state.semanticRefreshPending = false;
      setSync("error", `${t("Refresh failed")} · ${error.message}`);
    }
  } finally {
    els.refreshSemantic.disabled = false;
  }
}
async function refreshTick() {
  state.timer = null;
  if (!els.auto.checked || document.hidden) return;
  refreshTunnels();
  await pollRevision();
  if (els.auto.checked && !document.hidden) {
    state.timer = setTimeout(refreshTick, 8000);
  }
}
function schedule() {
  clearTimeout(state.timer);
  state.timer = null;
  if (els.auto.checked && !document.hidden) {
    state.timer = setTimeout(refreshTick, 8000);
  }
}

els.workspace.addEventListener("change", async () => {
  await refreshProject({
    workspace: els.workspace.value,
    reason: "manual",
    force: true,
  });
  if (!els.accessPanel.classList.contains("hidden")) await loadAccess();
});
els.language.addEventListener("change", () => {
  state.language = els.language.value === "zh-CN" ? "zh-CN" : "en";
  localStorage.setItem("wcode.ui.language", state.language);
  applyLanguage();
});
els.theme.addEventListener("change", () => {
  state.theme = ["dark", "light"].includes(els.theme.value)
    ? els.theme.value
    : "system";
  localStorage.setItem("wcode.ui.theme", state.theme);
  applyTheme();
});
els.manage.addEventListener("click", async () => {
  els.accessPanel.classList.toggle("hidden");
  if (!els.accessPanel.classList.contains("hidden")) await loadAccess();
});
els.closeAccess.addEventListener(
  "click",
  () => els.accessPanel.classList.add("hidden"),
);
els.addWorkspace.addEventListener("click", addWorkspaceFromUi);
els.workspacePath.addEventListener("keydown", (event) => {
  if (event.key === "Enter") addWorkspaceFromUi();
});
els.addCommand.addEventListener("click", addCommandFromUi);
els.commandCandidate.addEventListener("keydown", (event) => {
  if (event.key === "Enter") addCommandFromUi();
});
els.authorizeOperation.addEventListener("click", authorizeOperationFromUi);
els.operationArgs.addEventListener("keydown", (event) => {
  if (event.key === "Enter") authorizeOperationFromUi();
});
els.refresh.addEventListener("click", async () => {
  await refreshProject({ reason: "manual", force: true });
  if (!els.accessPanel.classList.contains("hidden")) await loadAccess();
});
els.refreshSemantic.addEventListener("click", refreshSemantics);
els.auto.addEventListener("change", schedule);
els.search.addEventListener("input", () => {
  invalidate("requirements", "detail");
  renderRequirements();
  renderDetail();
});
document.querySelectorAll(".filter").forEach((button) =>
  button.addEventListener("click", () => {
    document.querySelectorAll(".filter").forEach((item) =>
      item.classList.remove("active")
    );
    button.classList.add("active");
    state.filter = button.dataset.filter;
    invalidate("requirements", "detail");
    renderRequirements();
    renderDetail();
  })
);
document.querySelectorAll(".arch-mode").forEach((button) =>
  button.addEventListener("click", () => {
    document.querySelectorAll(".arch-mode").forEach((item) =>
      item.classList.remove("active")
    );
    button.classList.add("active");
    state.architectureMode = button.dataset.archMode || "overlay";
    invalidate("architectureGraph");
    renderArchitectureGraph();
  })
);
document.addEventListener("visibilitychange", async () => {
  if (document.hidden) {
    clearTimeout(state.timer);
    state.timer = null;
    return;
  }
  if (els.auto.checked) {
    await refreshProject({ reason: "auto" });
    if (!els.accessPanel.classList.contains("hidden")) await loadAccess();
    schedule();
  }
});

applyTheme();
applyLanguage();
refreshTunnels();
refreshProject({ workspace: state.current, reason: "initial", force: true })
  .then(schedule);
