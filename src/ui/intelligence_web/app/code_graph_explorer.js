// Repository-level Code Graph overview/search workspace.
// Focused graph layout/interaction stays in code_graph.js.
function codeGraphOverviewLayout(overview) {
  const groups = new Map();
  for (const node of overview?.nodes || []) {
    const language = node.language || "unknown";
    if (!groups.has(language)) groups.set(language, []);
    groups.get(language).push(node);
  }
  const nodeWidth = 220, nodeHeight = 60, rowGap = 18, columnGap = 28, languageGap = 72;
  const maxRows = 10, padX = 40, padY = 72;
  const fresh = new Map();
  let cursorX = padX;
  for (const language of [...groups.keys()].sort()) {
    const nodes = groups.get(language).sort((a, b) => String(a.id).localeCompare(String(b.id)));
    const columns = Math.max(1, Math.ceil(nodes.length / maxRows));
    const laneWidth = columns * nodeWidth + Math.max(0, columns - 1) * columnGap;
    nodes.forEach((node, index) => {
      const column = Math.floor(index / maxRows), row = index % maxRows;
      fresh.set(node.id, {
        x: cursorX + column * (nodeWidth + columnGap),
        y: padY + row * (nodeHeight + rowGap),
        language,
        node,
      });
    });
    cursorX += laneWidth + languageGap;
  }

  const key = codeGraphViewKey(), cached = state.codeGraphLayouts.get(key) || new Map();
  const positions = new Map(), used = new Set();
  for (const node of overview?.nodes || []) {
    const previous = cached.get(node.id);
    if (!previous || previous.language !== (node.language || "unknown")) continue;
    positions.set(node.id, { ...previous, node });
    used.add(`${previous.x}:${previous.y}`);
  }
  for (const node of overview?.nodes || []) {
    if (positions.has(node.id)) continue;
    const base = fresh.get(node.id);
    if (!base) continue;
    let x = base.x, y = base.y, attempts = 0;
    while (used.has(`${x}:${y}`) && attempts++ < 96) {
      y += nodeHeight + rowGap;
      if (y > padY + 12 * (nodeHeight + rowGap)) {
        y = padY;
        x += nodeWidth + columnGap;
      }
    }
    positions.set(node.id, { x, y, language: base.language, node });
    used.add(`${x}:${y}`);
  }

  state.codeGraphLayouts.set(key, new Map([...positions].map(([id, p]) => [
    id, { x:p.x, y:p.y, language:p.language }
  ])));
  if (state.codeGraphLayouts.size > 24) state.codeGraphLayouts.delete(state.codeGraphLayouts.keys().next().value);

  const laneBounds = new Map();
  for (const position of positions.values()) {
    const language = position.language || "unknown";
    const current = laneBounds.get(language) || { min:position.x, max:position.x + nodeWidth, count:0 };
    current.min = Math.min(current.min, position.x);
    current.max = Math.max(current.max, position.x + nodeWidth);
    current.count += 1;
    laneBounds.set(language, current);
  }
  const lanes = [...laneBounds.entries()]
    .map(([language, bounds]) => ({ language, x:bounds.min, width:bounds.max - bounds.min, count:bounds.count }))
    .sort((a, b) => a.x - b.x || a.language.localeCompare(b.language));
  const values = [...positions.values()];
  const width = values.reduce((max, p) => Math.max(max, p.x + nodeWidth + padX), 920);
  const height = values.reduce((max, p) => Math.max(max, p.y + nodeHeight + 42), 460);
  return { positions, lanes, nodeWidth, nodeHeight, width, height };
}
function codeGraphOverviewDiagram(overview) {
  const layout = codeGraphOverviewLayout(overview);
  const selected = state.selectedCodeNode || "";
  const edges = (overview.edges || []).map(edge => {
    const from = layout.positions.get(edge.from), to = layout.positions.get(edge.to);
    if (!from || !to) return "";
    const x1 = from.x + layout.nodeWidth, y1 = from.y + layout.nodeHeight / 2;
    const x2 = to.x, y2 = to.y + layout.nodeHeight / 2;
    const bend = Math.max(42, Math.abs(x2 - x1) * .42);
    const active = edge.from === selected || edge.to === selected;
    const title = `${from.node.path} → ${to.node.path} · ${edge.count} relations`;
    return `<path class="code-graph-edge-path${active ? " selected" : ""}" data-edge-from="${esc(edge.from)}" data-edge-to="${esc(edge.to)}" d="M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}" marker-end="url(#codeGraphArrow)"><title>${esc(title)}</title></path>`;
  }).join("");
  const lanes = layout.lanes.map(lane =>
    `<g class="code-graph-language-lane"><text x="${lane.x}" y="34">${esc(lane.language)} · ${lane.count}</text><line x1="${lane.x}" y1="46" x2="${lane.x + lane.width}" y2="46"></line></g>`
  ).join("");
  const nodes = (overview.nodes || []).map(node => {
    const p = layout.positions.get(node.id);
    if (!p) return "";
    const changed = Boolean((state.project?.changes || []).some(change => change.path === node.path));
    const classes = ["code-graph-svg-node", "overview", selected === node.id ? "selected" : "", changed ? "changed" : ""].filter(Boolean).join(" ");
    const title = `${node.path} · ${node.language} · ${node.symbols} symbols · ${node.degree} links`;
    return `<g class="${classes}" transform="translate(${p.x} ${p.y})" data-code-overview-node="${esc(node.id)}" role="button" tabindex="0" aria-label="${esc(title)}">
      <title>${esc(title)}</title>
      <rect width="${layout.nodeWidth}" height="${layout.nodeHeight}" rx="13"></rect>
      ${changed ? `<line class="code-graph-node-change" x1="1" y1="12" x2="1" y2="${layout.nodeHeight - 12}"></line>` : ""}
      <text class="code-graph-svg-kind" x="13" y="18">${esc(node.language || "unknown")}</text>
      <text class="code-graph-svg-distance" x="${layout.nodeWidth - 13}" y="18" text-anchor="end">${node.degree} links</text>
      <text class="code-graph-svg-label" x="13" y="40">${esc(codeGraphShortText(node.path, 31))}</text>
      <text class="code-graph-svg-path" x="13" y="54">${node.symbols} symbols</text>
    </g>`;
  }).join("");
  return `<div class="code-graph-viewport code-graph-overview-viewport" role="region" aria-label="${esc(localized("Repository code graph overview", "仓库代码图谱概览"))}" tabindex="0">
    <svg class="code-graph-diagram" width="${layout.width}" height="${layout.height}" viewBox="0 0 ${layout.width} ${layout.height}">
      <defs><marker id="codeGraphArrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L8,4 L0,8 z" class="code-graph-arrow"></path></marker></defs>
      ${lanes}<g class="code-graph-edges">${edges}</g><g class="code-graph-nodes">${nodes}</g>
    </svg>
  </div>`;
}
function renderCodeGraphOverviewInspector(node) {
  if (!els.codeGraphInspector) return;
  if (!node) {
    els.codeGraphInspector.innerHTML = `<div class="empty">${esc(localized("Select a file to inspect repository-level relationships.", "选择文件以查看仓库级关系。"))}</div>`;
    return;
  }
  els.codeGraphInspector.innerHTML = `<div class="inspector-kind">${esc(node.language || "unknown")}</div>
    <h3>${esc(node.path)}</h3>
    <div class="pills"><span class="code-graph-chip accent">${node.degree} ${esc(localized("cross-file links", "条跨文件关系"))}</span><span class="code-graph-chip">${node.symbols} ${esc(localized("symbols", "个符号"))}</span></div>
    <p class="inspector-path">${esc(localized("Repository overview is file-level. Open Focus to inspect functions, types, callers and callees.", "仓库概览按文件聚合；进入 Focus 可查看函数、类型、调用者与被调用者。"))}</p>
    <button type="button" class="primary code-graph-focus" data-code-focus="${esc(node.id)}">${esc(localized("Focus this file", "聚焦此文件"))}</button>`;
  els.codeGraphInspector.querySelector("[data-code-focus]")?.addEventListener("click", event => {
    void loadCodeGraph({ nodeId: event.currentTarget.dataset.codeFocus });
  });
}
function renderCodeGraphOverview() {
  const overview = state.codeGraphOverview;
  if (state.codeGraphLoading && !overview) {
    els.codeGraphSummary.innerHTML = "";
    els.codeGraphMap.innerHTML = `<div class="code-graph-empty code-graph-loading"><strong>${esc(localized("Building repository overview…", "正在构建仓库概览…"))}</strong></div>`;
    return;
  }
  if (!overview?.nodes?.length) {
    const error = state.codeGraphError ? `<div class="bad">${esc(state.codeGraphError)}</div>` : "";
    els.codeGraphSummary.innerHTML = "";
    els.codeGraphMap.innerHTML = `<div class="code-graph-empty"><div><strong>${esc(localized("Repository graph is not available yet", "仓库图谱尚不可用"))}</strong><span>${esc(localized("Build the latest software graph to see bounded repository-wide structure.", "构建最新 Software Graph 后可查看有界的仓库级结构。"))}</span>${error}</div></div>`;
    return;
  }
  captureCodeGraphViewport();
  const languageCount = Object.keys(overview.languages || {}).length;
  els.codeGraphSummary.innerHTML = `<div class="code-graph-summary-main">
    <strong>${esc(localized("Repository overview", "仓库概览"))} · ${overview.files_indexed}/${overview.files_considered} ${esc(localized("files indexed", "个文件已索引"))}</strong>
    <span class="code-graph-chip">${overview.total_nodes} ${esc(localized("snapshot nodes", "快照节点"))}</span>
    <span class="code-graph-chip">${overview.total_edges} ${esc(localized("snapshot edges", "快照关系"))}</span>
    <span class="code-graph-chip accent">${languageCount} ${esc(localized("languages", "种语言"))}</span>
    ${overview.truncated ? `<span class="code-graph-chip warn">${esc(localized("bounded / truncated", "有界 / 已截断"))}</span>` : ""}
  </div><div class="pills">${Object.entries(overview.languages || {}).sort((a,b)=>b[1]-a[1]).slice(0,8).map(([language,count])=>`<span class="code-graph-chip">${esc(language)} ${count}</span>`).join("")}</div>`;
  els.codeGraphMap.innerHTML = codeGraphOverviewDiagram(overview);
  els.codeGraphMap.querySelectorAll("[data-code-overview-node]").forEach(element => {
    const select = () => {
      state.selectedCodeNode = element.dataset.codeOverviewNode;
      els.codeGraphMap.querySelectorAll("[data-code-overview-node]").forEach(node => node.classList.toggle("selected", node === element));
      const file = overview.nodes.find(node => node.id === state.selectedCodeNode);
      renderCodeGraphOverviewInspector(file);
    };
    element.addEventListener("click", select);
    element.addEventListener("keydown", event => {
      if (event.key === "Enter" || event.key === " ") { event.preventDefault(); select(); }
    });
    element.addEventListener("dblclick", () => void loadCodeGraph({ nodeId: element.dataset.codeOverviewNode }));
  });
  bindCodeGraphViewport();
  const selectedNode = overview.nodes.find(node => node.id === state.selectedCodeNode);
  renderCodeGraphOverviewInspector(selectedNode);
}

function validCodeGraphSearchResponse(data) {
  const search = data?.search;
  if (!(search && typeof search === "object" && !Array.isArray(search) &&
    typeof search.snapshot_id === "string" && search.snapshot_id &&
    typeof search.query === "string" &&
    Array.isArray(search.results) && search.results.length <= 40 &&
    typeof search.truncated === "boolean")) return false;
  const ids = new Set();
  return search.results.every(result => {
    const node = result?.node;
    if (!(node && typeof node === "object" && typeof node.id === "string" && node.id &&
      typeof node.label === "string" && typeof node.kind === "string" &&
      node.provenance && typeof node.provenance.provider === "string" &&
      typeof node.provenance.precision === "string" &&
      typeof result.match_kind === "string" &&
      Number.isInteger(Number(result.score)) && Number(result.score) >= 0 &&
      Number.isInteger(Number(result.relations)) && Number(result.relations) >= 0)) return false;
    if (ids.has(node.id)) return false;
    ids.add(node.id);
    return true;
  });
}
function validCodeGraphOverviewResponse(data) {
  const overview = data?.overview;
  if (!(overview && typeof overview === "object" && !Array.isArray(overview) &&
    typeof overview.snapshot_id === "string" && overview.snapshot_id &&
    Number.isInteger(Number(overview.files_considered)) &&
    Number.isInteger(Number(overview.files_indexed)) &&
    Number.isInteger(Number(overview.total_nodes)) &&
    Number.isInteger(Number(overview.total_edges)) &&
    Number.isInteger(Number(overview.total_files)) &&
    overview.languages && typeof overview.languages === "object" && !Array.isArray(overview.languages) &&
    overview.relation_counts && typeof overview.relation_counts === "object" && !Array.isArray(overview.relation_counts) &&
    Array.isArray(overview.nodes) && overview.nodes.length <= 320 &&
    Array.isArray(overview.edges) &&
    typeof overview.truncated === "boolean")) return false;
  const ids = new Set();
  for (const node of overview.nodes) {
    if (!(node && typeof node.id === "string" && node.id.startsWith("file:") &&
      typeof node.path === "string" && typeof node.language === "string" &&
      Number.isInteger(Number(node.symbols)) && Number(node.symbols) >= 0 &&
      Number.isInteger(Number(node.degree)) && Number(node.degree) >= 0) ||
      ids.has(node.id)) return false;
    ids.add(node.id);
  }
  return overview.edges.every(edge =>
    edge && ids.has(edge.from) && ids.has(edge.to) &&
    Number.isInteger(Number(edge.count)) && Number(edge.count) > 0);
}
function renderCodeGraphSearchResults() {
  if (!els.codeGraphSearchResults) return;
  const results = state.codeGraphSearchResults || [];
  if (!results.length) {
    els.codeGraphSearchResults.classList.add("hidden");
    els.codeGraphSearchResults.innerHTML = "";
    return;
  }
  els.codeGraphSearchResults.innerHTML = results.map((result, index) => {
    const node = result.node || {}, path = codeGraphPath(node);
    const language = node.attributes?.language || "";
    return `<button type="button" role="option" data-code-search-index="${index}" data-code-node-id="${esc(node.id)}">
      <span class="code-graph-search-result-main"><b>${esc(node.label || node.id)}</b><small>${esc(path || node.id)}</small></span>
      <span class="code-graph-search-result-meta"><em>${esc(statusLabel(codeGraphDisplayKind(node)))}</em>${language ? `<em>${esc(language)}</em>` : ""}<em>${result.relations} links</em></span>
    </button>`;
  }).join("");
  els.codeGraphSearchResults.classList.remove("hidden");
  els.codeGraphSearchResults.querySelectorAll("[data-code-search-index]").forEach(button => {
    button.addEventListener("click", () => chooseCodeGraphSearchResult(Number(button.dataset.codeSearchIndex)));
  });
}
function chooseCodeGraphSearchResult(index) {
  const result = state.codeGraphSearchResults?.[index];
  if (!result?.node?.id) return;
  state.codeGraphSearchResults = [];
  renderCodeGraphSearchResults();
  void loadCodeGraph({ nodeId: result.node.id, query: (els.codeGraphSearch?.value || "").trim() });
}
function setCodeGraphView(view, { load = true } = {}) {
  const next = view === "focus" ? "focus" : "overview";
  if (state.codeGraphView !== next) captureCodeGraphViewport();
  state.codeGraphView = next;
  document.querySelectorAll("[data-code-graph-view]").forEach(button =>
    button.classList.toggle("active", button.dataset.codeGraphView === next));
  els.codeGraphSection?.classList.toggle("code-graph-overview-mode", next === "overview");
  const focusOnly = next === "focus";
  document.querySelectorAll("[data-code-graph-mode]").forEach(button => { button.disabled = !focusOnly; });
  if (els.codeGraphDepth) els.codeGraphDepth.disabled = !focusOnly;
  if (load) {
    if (next === "overview") void loadCodeGraphOverview();
    else renderCodeGraph();
  } else {
    renderCodeGraph();
  }
}
async function loadCodeGraphOverview() {
  state.codeGraphController?.abort();
  const controller = new AbortController();
  state.codeGraphController = controller;
  state.codeGraphLoading = true;
  state.codeGraphError = "";
  state.codeGraphView = "overview";
  renderCodeGraph();
  const params = new URLSearchParams({ view: "overview", limit: "260" });
  if (state.codeGraphSnapshot) params.set("snapshot_id", state.codeGraphSnapshot);
  try {
    const data = await uiJson(`/intelligence/code-graph?${params}`, "GET", undefined, {
      workspace: state.current,
      signal: controller.signal,
      timeout: 30000,
    });
    if (controller.signal.aborted || data.workspace !== state.current) return false;
    if (!validCodeGraphOverviewResponse(data)) throw new Error(localized("Invalid repository graph overview", "仓库图谱概览响应无效"));
    state.codeGraphOverview = data.overview;
    state.codeGraphWorkspace = data.workspace;
    renderCodeGraph();
    return true;
  } catch (error) {
    if (!controller.signal.aborted) {
      state.codeGraphOverview = null;
      state.codeGraphError = requestFailureMessage(error);
      renderCodeGraph();
    }
    return false;
  } finally {
    if (state.codeGraphController === controller) {
      state.codeGraphController = null;
      state.codeGraphLoading = false;
      renderCodeGraph();
    }
  }
}
async function searchCodeGraph(requestedQuery) {
  const query = String(requestedQuery ?? els.codeGraphSearch?.value ?? "").trim();
  state.codeGraphSearchController?.abort();
  if (query.length < 2) {
    state.codeGraphSearchResults = [];
    renderCodeGraphSearchResults();
    return false;
  }
  if (codeGraphLooksLikeDesignPath(query)) {
    state.codeGraphSearchResults = [];
    state.codeGraphError = localized("Code Graph searches source-code entities only.", "代码图谱只搜索源码实体。");
    renderCodeGraphSearchResults();
    renderCodeGraph();
    return false;
  }
  const controller = new AbortController();
  state.codeGraphSearchController = controller;
  const params = new URLSearchParams({ view: "search", q: query, limit: "20" });
  if (state.codeGraphSnapshot) params.set("snapshot_id", state.codeGraphSnapshot);
  try {
    const data = await uiJson(`/intelligence/code-graph?${params}`, "GET", undefined, {
      workspace: state.current,
      signal: controller.signal,
      timeout: 20000,
    });
    if (controller.signal.aborted || data.workspace !== state.current) return false;
    if (!validCodeGraphSearchResponse(data) || data.search.query !== query) {
      throw new Error(localized("Invalid code graph search response", "代码图谱搜索响应无效"));
    }
    state.codeGraphSearchResults = data.search.results;
    state.codeGraphError = "";
    renderCodeGraphSearchResults();
    return true;
  } catch (error) {
    if (!controller.signal.aborted) {
      state.codeGraphSearchResults = [];
      state.codeGraphError = requestFailureMessage(error);
      renderCodeGraphSearchResults();
    }
    return false;
  } finally {
    if (state.codeGraphSearchController === controller) state.codeGraphSearchController = null;
  }
}
