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
