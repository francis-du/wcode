// Read-only, revision-bound source reader for the Engineering Observatory.
// It reuses the protected /intelligence/code-source and /intelligence/change-detail
// contracts. It never accepts an arbitrary revision expression or filesystem root:
// the bounded code graph resolves the path to a snapshot node, and the server
// re-checks Workspace, snapshot and source SHA before returning bytes.
function codeReaderActive(current) {
  return Boolean(current) && state.codeReader === current && state.current === current.workspace
    && state.workspaceEpoch === current.epoch && token === current.owner
    && current.controller && !current.controller.signal.aborted;
}
function codeReaderSourceAction(reference, paths = repositorySourcePaths()) {
  if (!reference || reference.provider === "harness") return "";
  const path = designReferencePath(reference);
  if (!path || !paths.has(path)) return "";
  const enabled = Boolean(token) && validCodeGraphRepositoryRevision(repositorySourceRevision());
  const note = enabled
    ? localized("Open the mapped file in the read-only code reader at the observed revision.", "在只读代码阅读器中打开观测版本的映射文件。")
    : localized("Refresh the project with UI access before reading source.", "请在有界面访问权限时刷新项目，再阅读源码。");
  return `<button type="button" class="quiet-action code-reader-action" data-reader-path="${esc(path)}"${enabled ? "" : " disabled"} title="${esc(note)}" aria-label="${esc(localized("Read source: ", "阅读源码：") + path)}">${esc(t("Read source"))}</button>`;
}
function bindCodeReaderActions(host) {
  host?.querySelectorAll("[data-reader-path]").forEach(button => {
    button.addEventListener("click", () => {
      if (button.disabled) return;
      void openCodeReader(button.dataset.readerPath);
    });
  });
}
async function openCodeReader(path, { layer = "working", focusLine = null, loadGraph = null, reveal = true } = {}) {
  if (typeof path !== "string" || !validRepositorySourcePath(path)) return false;
  const revision = repositorySourceRevision();
  const previous = state.codeReader;
  const sequence = (previous?.sequence || 0) + 1;
  previous?.controller?.abort();
  const controller = new AbortController();
  const current = {
    open: reveal, path, layer, repositoryRevision: revision, source: null, change: null, requirements: [],
    symbols: [], symbolRequirements: new Map(), graphTruncated: false,
    loading: true, sourceError: "", changeError: "", missing: false, focusLine: Number.isInteger(focusLine) ? focusLine : null,
    nodeId: "", snapshotId: "", workspace: state.current, epoch: state.workspaceEpoch, owner: token,
    sequence, controller,
  };
  state.codeReader = current;
  if (!token || !validCodeGraphRepositoryRevision(revision)) {
    current.loading = false;
    current.sourceError = localized("Source revision unavailable. Refresh the project before reading source.", "源码版本不可用。请先刷新项目，再阅读源码。");
    renderCodeReader();
    return false;
  }
  renderCodeReader();
  const load = loadGraph || (typeof loadCodeGraph === "function" ? loadCodeGraph : null);
  try {
    if (!load) throw new Error(localized("Code graph is unavailable for source reading.", "代码图谱不可用，无法读取源码。"));
    const loaded = await load({
      nodeId: `file:${path}`, repositoryRevision: revision,
      observation: () => codeReaderActive(current) && repositorySourceRevisionMatches(revision),
    });
    if (!codeReaderActive(current) || !repositorySourceRevisionMatches(revision)) return false;
    if (!loaded) {
      current.loading = false;
      if (state.codeGraphError) current.sourceError = state.codeGraphError;
      else {
        current.missing = true;
        current.sourceError = localized("This mapped file is not available in the current bounded graph. Refresh the project or inspect its mapping; absence here does not prove the file is missing.", "当前有界图谱未提供此映射文件。请刷新项目或检查映射；此处未观测到不代表文件不存在。");
      }
      renderCodeReader();
      return false;
    }
    const graph = state.codeGraph;
    const nodeId = `file:${path}`;
    const node = graph?.nodes?.find(item => item.node?.id === nodeId && codeGraphPath(item.node) === path)?.node
      || graph?.nodes?.find(item => codeGraphPath(item.node) === path)?.node;
    if (!graph?.snapshot_id || !node?.id) {
      current.loading = false;
      current.missing = true;
      current.sourceError = localized("This mapped file is not available in the current bounded graph. Refresh the project or inspect its mapping; absence here does not prove the file is missing.", "当前有界图谱未提供此映射文件。请刷新项目或检查映射；此处未观测到不代表文件不存在。");
      renderCodeReader();
      return false;
    }
    current.nodeId = node.id;
    current.snapshotId = graph.snapshot_id;
    current.symbols = codeReaderSymbols(current);
    current.symbolRequirements = codeReaderSymbolRequirements(current.path);
    current.graphTruncated = graph.truncated === true;
    const startLine = current.focusLine != null ? Math.max(1, current.focusLine - 12) : null;
    await Promise.all([
      loadCodeReaderSource(current, node, graph.snapshot_id, { startLine }),
      loadCodeReaderChange(current, { expectedSnapshot: null }),
    ]);
    if (!codeReaderActive(current)) return false;
    current.requirements = codeReaderRequirements(path);
    current.loading = false;
    renderCodeReader();
    if (current.focusLine != null) focusCodeReaderLine(current.focusLine);
    return true;
  } catch (error) {
    if (!codeReaderActive(current)) return false;
    current.loading = false;
    current.sourceError = requestFailureMessage(error);
    renderCodeReader();
    return false;
  }
}
async function loadCodeReaderSource(current, node, snapshotId, { startLine = null } = {}) {
  const params = new URLSearchParams({ node_id: node.id, snapshot_id: snapshotId, context_lines: "12" });
  if (startLine != null) params.set("start_line", String(startLine));
  try {
    const data = await uiJson(`/intelligence/code-source?${params}`, "GET", undefined, {
      workspace: current.workspace, signal: current.controller.signal, timeout: 25000,
    });
    if (!codeReaderActive(current)) return false;
    const expected = { workspace: current.workspace, snapshot: snapshotId, nodeId: node.id, path: current.path };
    if (!validCodeGraphSourceResponse(data, expected)) throw new Error(localized("Invalid source snapshot", "源码快照无效"));
    current.source = data.source;
    current.sourceError = "";
    current.missing = false;
    renderCodeReader();
    return true;
  } catch (error) {
    if (!codeReaderActive(current)) return false;
    current.source = null;
    current.sourceError = error.status === 409
      ? localized("Source changed since the selected graph snapshot. Reload the reader instead of mixing versions.", "源码已变化，与所选图谱快照不一致。请重新读取，不能混合版本。")
      : error.status === 422
        ? localized("Source inspection is unavailable for this graph node. The file may be binary, protected, or outside the bounded snapshot.", "此图谱节点无法检查源码。文件可能是二进制、受保护或不在有界快照内。")
        : requestFailureMessage(error);
    renderCodeReader();
    return false;
  }
}
async function loadCodeReaderChange(current, { expectedSnapshot = null } = {}) {
  const params = new URLSearchParams({ path: current.path, layer: current.layer });
  if (expectedSnapshot) params.set("expected_snapshot", expectedSnapshot);
  try {
    const result = await uiJson(`/intelligence/change-detail?${params}`, "GET", undefined, {
      workspace: current.workspace, signal: current.controller.signal, timeout: 25000,
    });
    if (!codeReaderActive(current)) return false;
    const view = result.change;
    if (result.workspace !== current.workspace || view?.path !== current.path || view?.layer !== current.layer
      || !/^[a-f0-9]{64}$/i.test(view?.snapshot_id || "")
      || !["unified_diff", "untracked_source", "binary"].includes(view?.kind)
      || typeof view?.content !== "string" || view.content.length > 65536
      || !validChangeRanges(view)
      || typeof view?.truncated !== "boolean" || typeof view?.redacted !== "boolean") {
      throw new Error(localized("Invalid change snapshot", "变更快照无效"));
    }
    current.change = view;
    current.changeRepositoryRevision = validChangeRepositoryRevision(result.repository_revision) ? result.repository_revision : null;
    current.changeError = "";
    renderCodeReader();
    return true;
  } catch (error) {
    if (!codeReaderActive(current)) return false;
    current.change = null;
    current.changeError = error.status === 409
      ? localized("The file or index changed. Reload instead of mixing versions.", "文件或暂存区已变化，请重新读取，不能混合版本。")
      : requestFailureMessage(error);
    renderCodeReader();
    return false;
  }
}
function reloadCodeReader() {
  const current = state.codeReader;
  if (!current) return false;
  void openCodeReader(current.path, { layer: current.layer, focusLine: current.focusLine });
  return true;
}
function closeCodeReader() {
  clearCodeReader();
}
function clearCodeReader() {
  const current = state.codeReader;
  if (!current) return;
  current.controller?.abort();
  state.codeReader = null;
  const panel = els.codeReaderPanel;
  if (panel) {
    panel.classList.add("hidden");
    panel.setAttribute("aria-hidden", "true");
  }
  if (els.codeReaderBody) els.codeReaderBody.innerHTML = "";
  if (els.codeReaderPath) els.codeReaderPath.textContent = "";
}
function invalidateCodeReader() {
  const current = state.codeReader;
  if (!current) return;
  current.change = null;
  current.requirements = [];
  current.symbols = [];
  current.symbolRequirements = new Map();
  current.graphTruncated = false;
  current.changeError = localized("Project state changed or could not be refreshed. Reload this file.", "项目状态已更新或刷新失败，请重新读取此文件。");
  renderCodeReader();
}
function codeReaderChangeOverlays(change, source) {
  const overlays = new Map();
  const removed = [];
  if (!change || change.kind === "binary") return { overlays, removed, mismatch: false };
  const mismatch = change.after_source_matches_worktree === false
    || Boolean(change.worktree_sha256 && source?.current_sha256 && change.worktree_sha256 !== source.current_sha256);
  if (mismatch) return { overlays, removed, mismatch: true };
  for (const row of changeDisplayRows(change)) {
    if (change.kind === "untracked_source") {
      if (Number.isInteger(row.after)) overlays.set(row.after, "added");
      continue;
    }
    if (row.tone === "added" && Number.isInteger(row.after)) overlays.set(row.after, "added");
    else if (row.tone === "context" && Number.isInteger(row.after)) overlays.set(row.after, "context");
    else if (row.tone === "removed" && Number.isInteger(row.before)) removed.push({ line: row.before, text: row.text });
  }
  return { overlays, removed: removed.slice(0, 120), mismatch: false };
}
function codeReaderSourceHtml(current, overlays) {
  const source = current.source;
  if (!source) return "";
  const expectedRows = Math.max(0, source.end_line - source.start_line + 1);
  const lines = String(source.content || "").split("\n").slice(0, expectedRows);
  const rows = lines.map((line, index) => {
    const lineNumber = source.start_line + index;
    const tone = overlays.get(lineNumber) || "";
    const focus = lineNumber >= source.focus_start_line && lineNumber <= source.focus_end_line;
    return `<div class="code-reader-line${tone ? ` ${tone}` : ""}${focus ? " focus" : ""}" data-code-reader-line="${lineNumber}" tabindex="-1"><span class="code-reader-gutter">${lineNumber}</span><span class="code-reader-marker" aria-hidden="true"></span><code>${esc(line)}</code></div>`;
  }).join("");
  const empty = source.total_lines === 0
    ? `<div class="empty">${esc(t("This source file is empty."))}</div>`
    : "";
  const paging = source.total_lines === 0 ? "" : `<nav class="code-reader-paging" aria-label="${esc(t("Source pages"))}"><button type="button" data-code-reader-page="${Math.max(1, source.start_line - 240)}"${source.start_line <= 1 ? " disabled" : ""}>${esc(t("Previous"))}</button><span>${num(source.start_line)}–${num(source.end_line)} / ${num(source.total_lines)} ${esc(t("lines"))}</span><button type="button" data-code-reader-page="${source.end_line + 1}"${source.end_line >= source.total_lines ? " disabled" : ""}>${esc(t("Next"))}</button></nav>`;
  return `${empty}<div class="code-reader-source" role="region" tabindex="0" aria-label="${esc(t("Read-only source code"))}">${rows}</div>${paging}`;
}
function codeReaderChangeHtml(change, removed) {
  if (change.kind === "binary") {
    return `<p>${esc(t("Binary difference; text preview is unavailable."))}</p>`;
  }
  const after = change.after_changed_ranges || [];
  const before = change.before_changed_ranges || [];
  const rangeLabel = range => range.start_line === range.end_line
    ? `L${range.start_line}`
    : `L${range.start_line}–${range.end_line}`;
  const added = after.slice(0, 12).map(range =>
    `<button type="button" class="code-reader-range added" data-reader-goto="${range.start_line}">+${esc(rangeLabel(range))}</button>`
  ).join("");
  const removedRanges = before.slice(0, 12).map(range =>
    `<span class="code-reader-range removed">-${esc(rangeLabel(range))}</span>`
  ).join("");
  const more = after.length > 12 || before.length > 12
    ? `<span class="panel-meta">${esc(t("bounded map"))}</span>`
    : "";
  const note = change.kind === "untracked_source"
    ? localized("Untracked source; every line is new. The source gutter keeps the current line number.", "未跟踪源码，没有已提交基线；源码行号保持当前行号。")
    : localized("Added and context lines are marked in the source; removed baseline lines are listed below.", "新增行与上下文行在源码中标记；删除的基线行在下方列出。");
  const empty = !after.length && !before.length
    ? `<div class="empty">${esc(t("No differences in this selected layer."))}</div>`
    : "";
  const removedRows = removed.length
    ? `<details class="code-reader-removed"><summary>${esc(t("Removed baseline lines"))} (${removed.length})</summary>${removed.map(row =>
        `<div class="code-reader-line removed"><span class="code-reader-gutter">${row.line}</span><span class="code-reader-marker" aria-hidden="true"></span><code>${esc(row.text.replace(/^-/, ""))}</code></div>`
      ).join("")}</details>`
    : "";
  return `<p class="panel-meta">${esc(note)}</p><div class="code-reader-ranges">${added}${removedRanges}${more}</div>${empty}${removedRows}`;
}
function codeReaderRequirements(path) {
  const project = state.project;
  if (!project || project.workspace !== state.current) return [];
  const matches = new Map();
  const add = (requirement, via) => {
    if (!requirement?.id) return;
    const existing = matches.get(requirement.id);
    if (existing) {
      if (!existing.via.includes(via)) existing.via.push(via);
    } else {
      matches.set(requirement.id, { id: requirement.id, title: requirement.title || requirement.id, via: [via] });
    }
  };
  for (const requirement of project.requirements || []) {
    for (const component of requirement.components || []) {
      for (const reference of component.implementation || []) {
        if (designReferencePath(reference) === path) add(requirement, localized("implementation", "实现"));
      }
    }
    for (const criterion of requirement.acceptance || []) {
      for (const reference of criterion.verification || []) {
        if (designReferencePath(reference) === path) add(requirement, localized("acceptance", "验收"));
      }
    }
  }
  for (const change of project.changes || []) {
    if (change.path !== path) continue;
    for (const id of change.affected_requirements || []) {
      const requirement = (project.requirements || []).find(item => item.id === id);
      add(requirement || { id }, localized("change", "变更"));
    }
  }
  return [...matches.values()];
}
function codeReaderRequirementsHtml(current) {
  const items = current.requirements || [];
  if (!items.length) {
    return `<div class="empty">${esc(t("No requirement is linked to this file in the current snapshot."))}</div>`;
  }
  return `<div class="code-reader-requirements">${items.map(requirement =>
    `<button type="button" class="code-reader-requirement" data-reader-requirement="${esc(requirement.id)}"><span>${esc(requirement.id)}</span><small>${esc(requirement.title)}</small><em>${esc(requirement.via.join(" · "))}</em></button>`
  ).join("")}</div>`;
}
// The outline is only as complete as the bounded graph that was loaded for this
// file, so an empty result stays an explicit observation gap rather than a claim
// that the file defines nothing.
function validCodeReaderSymbolNode(node) {
  const attributes = node?.attributes;
  const range = attributes?.range;
  const start = range?.start_line;
  const end = range?.end_line;
  const name = attributes?.qualified_name || attributes?.name || node?.label;
  return Boolean(node) && node.kind !== "file" && node.kind !== "package"
    && codeGraphPath(node) && typeof name === "string" && name
    && Number.isInteger(start) && Number.isInteger(end) && start >= 1 && end >= start;
}
function codeReaderSymbols(current) {
  const nodes = Array.isArray(state.codeGraph?.nodes) ? state.codeGraph.nodes : [];
  const symbols = [];
  for (const entry of nodes) {
    const node = entry?.node;
    if (!validCodeReaderSymbolNode(node) || codeGraphPath(node) !== current.path) continue;
    const attributes = node.attributes;
    symbols.push({
      nodeId: typeof node.id === "string" ? node.id : "",
      name: attributes.qualified_name || attributes.name || node.label,
      shortName: typeof attributes.name === "string" ? attributes.name : "",
      kind: attributes.source_kind || node.kind || "symbol",
      startLine: attributes.range.start_line,
      endLine: attributes.range.end_line,
    });
  }
  symbols.sort((left, right) => left.startLine - right.startLine || left.name.localeCompare(right.name));
  return symbols.slice(0, 200);
}
function codeReaderSymbolRequirements(path) {
  return requirementSymbolMap(path);
}
function codeReaderSymbolChanged(symbol, change) {
  if (!change || change.after_source_matches_worktree === false) return false;
  const ranges = Array.isArray(change.after_changed_ranges) ? change.after_changed_ranges : [];
  return ranges.some(range => Number.isInteger(range?.start_line) && Number.isInteger(range?.end_line)
    && range.start_line <= symbol.endLine && range.end_line >= symbol.startLine);
}
function codeReaderSymbolsHtml(current, mismatch) {
  const symbols = current.symbols || [];
  if (!symbols.length) {
    return `<div class="empty">${esc(localized(
      "No symbol in this file is present in the current bounded graph; absence here does not prove the file defines none.",
      "当前有界图谱没有提供此文件中的符号；此处未观测到不代表文件没有定义符号。",
    ))}</div>`;
  }
  const requirements = current.symbolRequirements || new Map();
  const rows = symbols.slice(0, 80).map(symbol => {
    const ids = requirements.get(symbol.name) || requirements.get(symbol.shortName) || [];
    const chips = ids.map(id =>
      `<button type="button" class="code-reader-requirement" data-reader-requirement="${esc(id)}"><span>${esc(id)}</span></button>`
    ).join("");
    return `<div class="code-reader-symbol${!mismatch && codeReaderSymbolChanged(symbol, current.change) ? " changed" : ""}"><button type="button" class="code-reader-symbol-jump" data-reader-symbol-line="${symbol.startLine}"><span class="code-reader-symbol-kind">${esc(symbol.kind)}</span><span class="code-reader-symbol-name">${esc(symbol.name)}</span><span class="code-reader-symbol-range">L${symbol.startLine}–L${symbol.endLine}</span></button><div class="code-reader-symbol-reqs">${chips}</div></div>`;
  }).join("");
  const bounded = symbols.length > 80
    ? `<div class="panel-meta">${esc(localized(
      `${symbols.length} symbols observed; the outline is bounded to the first 80.`,
      `观测到 ${symbols.length} 个符号；大纲最多显示前 80 个。`,
    ))}</div>`
    : "";
  // The focus graph is bounded by its own node budget, so a complete-looking
  // outline can still be a subset of the file's definitions.
  const partial = current.graphTruncated
    ? `<div class="panel-meta">${esc(localized(
      "The loaded graph snapshot is truncated, so this outline may omit definitions the file actually has.",
      "所加载的图谱快照已截断，此大纲可能遗漏文件实际存在的定义。",
    ))}</div>`
    : "";
  const note = `<div class="panel-meta">${esc(localized(
    "Symbols come from the bounded graph at the captured revision; a changed marker only means the captured change overlaps the definition's lines, not a semantic impact proof.",
    "符号来自捕获版本的有界图谱；变更标记只表示捕获的变更与定义行重叠，不是语义影响证明。",
  ))}</div>`;
  return `<div class="code-reader-symbols">${rows}</div>${bounded}${partial}${note}`;
}
function codeReaderIdentityHtml(current) {
  const source = current.source, change = current.change;
  return `<details class="code-reader-identity"><summary>${esc(t("Captured snapshot identity"))}</summary><code>source ${esc(source?.source_revision || "—")}<br>worktree ${esc(source?.current_sha256 || "—")}<br>change ${esc(change?.snapshot_id || "—")}<br>repository code ${esc(current.repositoryRevision?.code || "—")}<br>repository Design ${esc(current.repositoryRevision?.design || "—")}</code></details>`;
}
function focusCodeReaderLine(line) {
  const target = els.codeReaderBody?.querySelector(`[data-code-reader-line="${Number(line)}"]`);
  if (!target) return false;
  target.scrollIntoView?.({ block: "center" });
  target.focus?.({ preventScroll: true });
  return true;
}
async function loadCodeReaderPage(current, startLine) {
  if (!current.nodeId || !current.snapshotId) return false;
  current.loading = true;
  renderCodeReader();
  try {
    await loadCodeReaderSource(current, { id: current.nodeId }, current.snapshotId, { startLine });
    if (!codeReaderActive(current)) return false;
    if (current.focusLine != null) focusCodeReaderLine(current.focusLine);
    return true;
  } catch (error) {
    if (!codeReaderActive(current)) return false;
    current.sourceError = requestFailureMessage(error);
    renderCodeReader();
    return false;
  } finally {
    if (codeReaderActive(current)) {
      current.loading = false;
      renderCodeReader();
    }
  }
}
function codeReaderGotoLine(line) {
  const current = state.codeReader;
  const target = Number(line);
  if (!current || !Number.isInteger(target) || target < 1) return false;
  const source = current.source;
  if (source && target >= source.start_line && target <= source.end_line) {
    return focusCodeReaderLine(target);
  }
  if (!current.nodeId || !current.snapshotId) return false;
  current.focusLine = target;
  void loadCodeReaderPage(current, Math.max(1, target - 12));
  return true;
}
function openCodeReaderRequirement(id) {
  if (typeof id !== "string" || !id) return false;
  const requirement = (state.project?.requirements || []).find(item => item.id === id);
  if (!requirement) return false;
  state.filter = "all";
  if (els.search) els.search.value = "";
  document.querySelectorAll(".filter").forEach(button => {
    button.classList.toggle("active", button.dataset.filter === "all");
    button.setAttribute("aria-pressed", String(button.dataset.filter === "all"));
  });
  state.selected = id;
  closeCodeReader();
  revealSection("requirementsSection");
  invalidate("requirements", "detail");
  renderRequirements();
  renderDetail();
  els.detail?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  return true;
}
function renderCodeReader() {
  const current = state.codeReader;
  const panel = els.codeReaderPanel;
  if (!panel) return;
  if (!current || !current.open) {
    panel.classList.add("hidden");
    panel.setAttribute("aria-hidden", "true");
    return;
  }
  panel.classList.remove("hidden");
  panel.setAttribute("aria-hidden", "false");
  if (els.codeReaderPath) els.codeReaderPath.textContent = current.path;
  const body = els.codeReaderBody;
  if (!body) return;
  const source = current.source, change = current.change;
  const { overlays, removed, mismatch } = codeReaderChangeOverlays(change, source);
  const stale = current.repositoryRevision && !repositorySourceRevisionMatches(current.repositoryRevision);
  const flags = [
    source?.redacted ? pill(t("redacted"), "warn") : "",
    source?.truncated ? pill(t("bounded preview"), "warn") : "",
    change?.redacted ? pill(t("redacted"), "warn") : "",
    change?.truncated || change?.changed_ranges_truncated ? pill(t("truncated"), "warn") : "",
    stale ? pill(t("stale"), "warn") : "",
    current.missing ? pill(t("missing"), "warn") : "",
  ].join("");
  const sourceState = current.loading && !source
    ? `<div class="code-reader-state"><p role="status">${esc(t("Reading bounded source and change…"))}</p></div>`
    : source
      ? codeReaderSourceHtml(current, overlays)
      : `<div class="code-reader-state"><p class="bad" role="alert">${esc(current.sourceError || t("Source is unavailable in this observation."))}</p><button type="button" class="quiet-action code-reader-retry">${esc(t("Retry source read"))}</button></div>`;
  const changeState = current.changeError
    ? `<p class="bad" role="alert">${esc(current.changeError)}</p>`
    : change
      ? codeReaderChangeHtml(change, removed)
      : current.loading
        ? `<p role="status">${esc(t("Reading bounded change snapshot…"))}</p>`
        : `<div class="empty">${esc(localized("No change on the selected layer is mapped to this file.", "当前图层没有映射到本文件的改动。"))}</div>`;
  const mismatchNote = mismatch
    ? `<p class="warn">${esc(t("The change snapshot and the current source bytes do not match; line highlighting is withheld."))}</p>`
    : "";
  body.innerHTML = `${stale ? `<p class="warn" role="alert">${esc(t("This reader observation belongs to an earlier project revision. Reload it before comparing with the current snapshot."))}</p>` : ""}${current.sourceError && source ? `<p class="bad" role="alert">${esc(current.sourceError)}</p>` : ""}${mismatchNote}<div class="code-reader-view"><section class="code-reader-source-shell" aria-label="${esc(t("Read-only source"))}"><div class="code-reader-source-head"><div><strong>${esc(t("Read-only source"))}</strong><small>${esc(source ? `${source.path} · ${String(source.source_revision || "").slice(0, 19)}` : current.path)}</small></div><div class="pills">${flags}</div></div>${sourceState}</section><aside class="code-reader-side"><section class="code-reader-section"><h3>${esc(t("Changed lines"))}</h3>${changeState}</section><section class="code-reader-section"><h3>${esc(t("Requirements linked to this file"))}</h3>${codeReaderRequirementsHtml(current)}</section><section class="code-reader-section"><h3>${esc(t("Symbols in this file"))}</h3>${codeReaderSymbolsHtml(current, mismatch)}</section>${codeReaderIdentityHtml(current)}</aside></div>`;
  body.querySelectorAll("[data-reader-symbol-line]").forEach(button => {
    button.addEventListener("click", () => { codeReaderGotoLine(button.dataset.readerSymbolLine); });
  });
  body.querySelectorAll("[data-reader-goto]").forEach(button => {
    button.addEventListener("click", () => { codeReaderGotoLine(button.dataset.readerGoto); });
  });
  body.querySelectorAll("[data-reader-requirement]").forEach(button => {
    button.addEventListener("click", () => { openCodeReaderRequirement(button.dataset.readerRequirement); });
  });
  body.querySelectorAll("[data-code-reader-page]").forEach(button => {
    button.addEventListener("click", () => {
      if (!button.disabled) void loadCodeReaderPage(current, Number(button.dataset.codeReaderPage));
    });
  });
  body.querySelectorAll("[data-reader-retry]").forEach(button => {
    button.addEventListener("click", () => { reloadCodeReader(); });
  });
  if (els.codeReaderClose) els.codeReaderClose.onclick = closeCodeReader;
  if (els.codeReaderReload) els.codeReaderReload.onclick = reloadCodeReader;
  const layers = document.getElementById("codeReaderLayers");
  if (layers) {
    layers.innerHTML = ["working", "staged", "unstaged"].map(layer =>
      `<button type="button" class="code-reader-layer" data-code-reader-layer="${layer}" aria-pressed="${current.layer === layer}">${esc(changeLayerLabel(layer))}</button>`
    ).join("");
    layers.querySelectorAll("[data-code-reader-layer]").forEach(button => {
      button.addEventListener("click", () => {
        const layer = button.dataset.codeReaderLayer;
        if (!["working", "staged", "unstaged"].includes(layer) || layer === current.layer) return;
        void openCodeReader(current.path, { layer, focusLine: current.focusLine });
      });
    });
  }
}
