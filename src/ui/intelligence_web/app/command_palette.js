// Object search for the command palette. Requirements, components, files,
// attention signals, evidence records, worklist tasks and code-graph symbols are
// reached through this one entry, and every row names its kind before its name.
// Attention signals resolve through the same summary action path as the list, so
// the palette keeps the named destination here instead of a second copy.
let paletteSignalActions = {};
let paletteEvidenceKeys = {};
let paletteTaskIds = {};
let paletteScopeNote = "";

const commandObjectActions = (query) => {
  paletteSignalActions = {};
  paletteEvidenceKeys = {};
  paletteTaskIds = {};
  paletteScopeNote = "";
  const project = state.project;
  // Objects only join a search: an empty palette stays the short command list,
  // and a query bounds how many object rows are built and rendered.
  if (!project || !query) return [];
  const rows = [];
  for (const requirement of (project.requirements || [])) {
    const id = requirement?.id;
    if (typeof id !== "string" || !id) continue;
    rows.push([`object:requirement:${id}`, requirement.title || id, requirement.id, "target", localized("Requirement", "需求")]);
  }
  for (const component of (architectureData().components || [])) {
    const id = component?.id;
    if (typeof id !== "string" || !id) continue;
    rows.push([`object:component:${id}`, component.name || id, (component.responsibilities || [])[0] || localized("No responsibility declared", "未声明职责"), "layers", localized("Component", "组件")]);
  }
  for (const file of (project.structure?.entries || [])) {
    const path = file?.path;
    if (typeof path !== "string" || !path) continue;
    rows.push([`object:file:${path}`, file.name || path, path, "document", localized("File", "文件")]);
  }
  for (const signal of attentionSignals()) {
    const id = signal?.id;
    if (typeof id !== "string" || !id) continue;
    const key = `object:signal:${id}`;
    paletteSignalActions[key] = { action: signal.target, path: signal.path, requirement: signal.requirement };
    rows.push([key, signal.title, signal.detail, "shield", localized("Signal", "信号")]);
  }
  const evidenceItems = (project.proof?.effective?.items || []).slice(0, 32);
  for (const [index, item] of evidenceItems.entries()) {
    const key = evidenceKey(item, index);
    paletteEvidenceKeys[index] = key;
    rows.push([`object:evidence:${index}`, item.subject || key,
      `${item.producer || "—"} · ${item.result || "unknown"}`, "cube", localized("Evidence", "证据")]);
  }
  // Worklist tasks use the existing filtered list rather than a second selection
  // concept: the palette names the task, the list filters to it.
  const worklist = worklistObservation(project.execution?.worklist);
  if (worklist.known && worklist.exists) {
    for (const [index, item] of worklist.items.entries()) {
      paletteTaskIds[index] = item.id;
      rows.push([`object:task:${index}`, item.title || item.id,
        `${worklistStateLabel(worklistTaskState(item))} · ${item.id}`, "monitor", localized("Task", "任务")]);
    }
  }
  // Symbols only exist in the bounded code graph, which is loaded on demand. The
  // palette searches the loaded snapshot and says when there is none, rather than
  // silently omitting every symbol.
  const graph = state.codeGraph;
  if (graph) {
    for (const entry of (graph.nodes || [])) {
      const node = entry?.node;
      const id = node?.id;
      if (typeof id !== "string" || !id) continue;
      const path = node.attributes?.path || "";
      rows.push([`object:symbol:${id}`, node.label || id,
        `${codeGraphDisplayKind(node)}${path ? ` · ${path}` : ""}`, "code", localized("Symbol", "符号")]);
    }
  } else {
    paletteScopeNote = localized("Symbol search needs the loaded code graph. Open Code graph to search symbols.", "符号搜索需要已加载的代码图谱。打开 Code graph 后才能搜索符号。");
  }
  return rows;
};

// Returns true when the command named an object, so executeCommand can stop.
const paletteExecute = (command) => {
  if (command.startsWith("object:requirement:")) {
    state.selected = command.slice("object:requirement:".length);
    setRequirementFilter("all");
    activateWorkspaceTab("requirements", { scroll: true });
    return true;
  }
  if (command.startsWith("object:component:")) {
    state.selectedComponent = command.slice("object:component:".length);
    state.architectureView = "components";
    activateWorkspaceTab("architecture", { scroll: true });
    return true;
  }
  if (command.startsWith("object:file:")) {
    void openRepositoryFile(command.slice("object:file:".length));
    return true;
  }
  if (command.startsWith("object:change:")) {
    const path = command.slice("object:change:".length);
    if (!path) return true;
    revealSection("changesSection");
    void openChangeInspection(path, "working", true);
    requestAnimationFrame(() => focusSelectedChangePath(path));
    return true;
  }
  if (command.startsWith("object:signal:")) {
    const signal = paletteSignalActions[command];
    if (signal) void runSummaryAction(signal);
    return true;
  }
  if (command.startsWith("object:evidence:")) {
    const key = paletteEvidenceKeys[command.slice("object:evidence:".length)];
    if (key) openEvidenceKey(key);
    return true;
  }
  if (command.startsWith("object:task:")) {
    const id = paletteTaskIds[command.slice("object:task:".length)];
    if (!id) return true;
    const scope = worklistViewScope();
    scope.filter = "all"; scope.query = id; scope.page = 0;
    setWorklistView("all", id, 0, scope);
    revealSection("activitySection");
    return true;
  }
  if (command.startsWith("object:symbol:")) {
    const id = command.slice("object:symbol:".length);
    if (!state.codeGraph) return true;
    revealSection("codeGraphSection");
    state.codeGraphView = "focus";
    selectCodeGraphNode(id);
    return true;
  }
  return false;
};

// The palette's own mechanics live beside its object search so the whole
// feature is one module rather than a search here and its dialog in runtime.
const commandActions = () => [
  ["overview", localized("Open acceptance", "打开变更验收"), localized("Current decision, blockers and required checks", "当前结论、阻塞原因与必需检查"), "shield"],
  ["architecture", localized("Open architecture", "打开架构"), localized("System map and dependency evidence", "系统图谱与依赖证据"), "layers"],
  ["activity", localized("Open task activity", "打开任务活动"), localized("Task progress and runtime work", "持久化执行与实时工作"), "monitor"],
  ["proof", localized("Open verification evidence", "打开验证证据"), localized("Revision-bound checks and proof ledger", "版本绑定检查与证据账本"), "shield"],
  ["changes", localized("Open changes", "打开变更"), localized("Inspect source diffs and verification impact", "检查源码差异与验证影响"), "code"],
  ["files", localized("Browse source files", "浏览源码文件"), localized("Repository tree and protected source preview", "仓库树与受保护源码预览"), "document"],
  ["requirements", localized("Open requirements", "打开需求"), localized("Implementation and acceptance traceability", "实现与验收追踪"), "target"],
  ["refresh", localized("Refresh project", "刷新项目"), localized("Fetch the latest bounded snapshot", "获取最新有界项目快照"), "sync"],
  ["access", localized("Manage access", "管理访问权限"), localized("Workspace, executable and operation policy", "工作区、可执行文件与操作策略"), "settings"],
  ["theme", localized("Toggle theme", "切换主题"), localized("System, dark and light appearance", "系统、深色和浅色外观"), "monitor"],
];
function renderCommandPalette() {
  if (!els.commandPaletteList) return;
  const query = String(els.commandPaletteSearch?.value || "").trim().toLowerCase();
  const matches = commandActions().concat(commandObjectActions(query))
    .filter(item => !query || [item[1], item[2], item[4]].filter(Boolean).some(text => String(text).toLowerCase().includes(query)));
  const limit = 100;
  const rows = matches.slice(0, limit);
  const omitted = matches.length - rows.length;
  state.commandPaletteIndex = Math.min(state.commandPaletteIndex, Math.max(0, rows.length - 1));
  const note = (omitted
    ? `<div class="empty">${esc(localized(`${num(omitted)} more matches not shown. Narrow the search.`, `另有 ${num(omitted)} 项匹配未显示，请缩小搜索范围。`))}</div>`
    : "") + (paletteScopeNote ? `<div class="empty">${esc(paletteScopeNote)}</div>` : "");
  const body = rows.length
    ? rows.map((item, index) => `<button type="button" class="command-palette-item" role="option" aria-selected="${index === state.commandPaletteIndex}" data-command="${esc(item[0])}">${uiIcon(item[3])}<span><strong>${esc(item[1])}</strong><small>${item[4] ? `${esc(item[4])} · ` : ""}${esc(item[2])}</small></span><kbd>${index < 9 ? index + 1 : ""}</kbd></button>`).join("")
    : `<div class="empty">${esc(localized("No matching actions.", "没有匹配的操作。"))}</div>`;
  setHtml("commandPaletteList", els.commandPaletteList, body + note, () => {
    els.commandPaletteList.querySelectorAll("[data-command]").forEach((button, index) => button.addEventListener("click", () => { state.commandPaletteIndex = index; executeCommand(button.dataset.command); }));
  });
}
function setCommandPalette(open) {
  const wasOpen = state.commandPaletteOpen;
  state.commandPaletteOpen = Boolean(open);
  els.commandPalette?.classList.toggle("hidden", !state.commandPaletteOpen);
  els.commandPalette?.setAttribute("aria-hidden", String(!state.commandPaletteOpen));
  if (state.commandPaletteOpen) {
    if (!wasOpen) state.commandPaletteReturnFocus = document.activeElement;
    state.commandPaletteIndex = 0;
    renderCommandPalette();
    requestAnimationFrame(() => { if (state.commandPaletteOpen) els.commandPaletteSearch?.focus(); });
  } else {
    if (els.commandPaletteSearch) els.commandPaletteSearch.value = "";
    const target = state.commandPaletteReturnFocus;
    state.commandPaletteReturnFocus = null;
    if (wasOpen) (target?.isConnected ? target : els.projectNavigator)?.focus({ preventScroll: true });
  }
}
function executeCommand(command) {
  setCommandPalette(false);
  if (command === "refresh") { els.refresh?.click(); return; }
  if (command === "access") { els.manage?.click(); return; }
  if (command === "theme") { els.theme?.click(); return; }
  if (paletteExecute(command)) return;
  if (["overview", "architecture", "activity", "proof", "changes", "files", "requirements"].includes(command)) activateWorkspaceTab(command, { scroll: true });
  // A command may also name a section inside a tab; those are not tabs of their
  // own, so they resolve through the same reveal the in-page links use.
  else if (workspaceTabForSection[command]) revealSection(command);
}
function wireCommandPalette() {
  els.closeCommandPalette?.addEventListener("click", () => setCommandPalette(false));
  els.commandPaletteSearch?.addEventListener("input", () => { state.commandPaletteIndex = 0; renderCommandPalette(); });
  els.commandPaletteSearch?.addEventListener("keydown", event => {
    if (event.isComposing || event.keyCode === 229) return;
    const items = [...(els.commandPaletteList?.querySelectorAll("[data-command]") || [])];
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setCommandPalette(false); }
    else if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); state.commandPaletteIndex = (state.commandPaletteIndex + (event.key === "ArrowDown" ? 1 : -1) + Math.max(items.length, 1)) % Math.max(items.length, 1); renderCommandPalette(); }
    else if (event.key === "Enter" && items[state.commandPaletteIndex]) { event.preventDefault(); executeCommand(items[state.commandPaletteIndex].dataset.command); }
  });
  els.commandPalette?.addEventListener("keydown", event => {
    if (!state.commandPaletteOpen || event.key !== "Tab" || event.isComposing || event.keyCode === 229) return;
    const controls = [...els.commandPalette.querySelectorAll("button:not([disabled]), input:not([disabled])")];
    const first = controls[0], last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  });
  els.commandPalette?.addEventListener("click", event => { if (event.target === els.commandPalette) setCommandPalette(false); });
}
