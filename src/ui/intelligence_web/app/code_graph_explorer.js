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

// Change-inspection → Code Graph bridge. Keep snapshot/revision truth at the boundary.
async function openChangeSymbolInGraph(nodeId) {
  const current = state.changeInspection, view = current?.data, mapping = current?.symbolImpact;
  const repositoryRevision = current?.repositoryRevision;
  if (typeof nodeId !== "string" || !nodeId.startsWith("symbol:ts:") || !validChangeRepositoryRevision(repositoryRevision)
    || !view || !validChangeSymbolImpact(mapping, view)
    || mapping?.path !== view.path || mapping?.snapshot_id !== view.snapshot_id
    || mapping?.source_state !== "worktree" || mapping?.source_sha256 !== view.worktree_sha256
    || view.after_source_matches_worktree === false || !Array.isArray(mapping?.after_symbols)
    || !mapping.after_symbols.some(symbol => symbol.node_id === nodeId && symbol.counterpart_only === false)) return false;
  state.codeGraphSnapshot = "";
  revealSection("codeGraphSection");
  renderArchitecture();
  return loadCodeGraph({nodeId, repositoryRevision});
}
async function openChangeRelationNodeInGraph(nodeId) {
  const current = state.changeInspection, view = current?.data, mapping = current?.symbolImpact;
  const impact = current?.relationImpact, repositoryRevision = current?.repositoryRevision;
  const selectedNodeId = impact?.node_id;
  if (typeof nodeId !== "string" || !nodeId.startsWith("symbol:ts:") || !validChangeRepositoryRevision(repositoryRevision)
    || !view || !mapping || !validChangeSymbolImpact(mapping, view) || current?.impactNodeId !== selectedNodeId
    || mapping.path !== view.path || mapping.snapshot_id !== view.snapshot_id
    || mapping.source_state !== "worktree" || mapping.source_sha256 !== view.worktree_sha256
    || view.after_source_matches_worktree === false || !Array.isArray(mapping.after_symbols)
    || !mapping.after_symbols.some(symbol => symbol.node_id === selectedNodeId && symbol.counterpart_only === false)
    || !validChangeRelationImpact(impact, current, selectedNodeId)
    || impact.precision !== "syntax" || impact.degraded !== true
    || !impact.incoming_calls.some(row => row.node_id === nodeId)) return false;
  state.codeGraphSnapshot = "";
  revealSection("codeGraphSection");
  renderArchitecture();
  return loadCodeGraph({nodeId, repositoryRevision});
}
function validChangeRelationImpact(impact, current, nodeId) {
  const validLocations = rows => Array.isArray(rows) && rows.length <= 24 && rows.every(row =>
    typeof row?.path === "string" && row.path.length > 0
    && Number.isInteger(row.line) && row.line >= 1
    && Number.isInteger(row.character) && row.character >= 1 && (row.node_id == null || (typeof row.node_id === "string" && row.node_id.startsWith("symbol:ts:"))));
  const validSearchMatches = rows => Array.isArray(rows) && rows.length <= 24 && rows.every(row =>
    typeof row?.path === "string" && row.path.length > 0
    && Number.isInteger(row.line) && row.line >= 1
    && typeof row.source_sha256 === "string" && /^[0-9a-f]{64}$/i.test(row.source_sha256)
    && typeof row.text === "string" && [...row.text].length <= 240);
  if (!impact || impact.path !== current.path || impact.snapshot_id !== current.snapshotId
    || impact.node_id !== nodeId || impact.source_sha256 !== current.data?.worktree_sha256
    || !["syntax", "semantic"].includes(impact.precision) || typeof impact.provider !== "string"
    || typeof impact.routing !== "string" || typeof impact.degraded !== "boolean"
    || typeof impact.partial !== "boolean" || !validLocations(impact.incoming_calls)
    || !validLocations(impact.references) || !validLocations(impact.implementations)
    || !validSearchMatches(impact.search_matches)) return false;
  if (impact.precision === "syntax" && (!impact.degraded || impact.degraded_from !== "lsp"
    || impact.provider !== "tree-sitter+search" || impact.routing !== "syntax-degraded"
    || impact.references.length || impact.implementations.length)) return false;
  if (impact.precision === "semantic" && (impact.degraded || impact.degraded_from != null || impact.routing !== "lsp" || !impact.provider.startsWith("lsp:") || impact.search_matches.length
    || [...impact.incoming_calls, ...impact.references, ...impact.implementations].some(row => row.node_id != null))) return false;
  return true;
}
function changeRelationImpactPanel(current) {
  if (current.impactLoading) return `<section class="change-symbol-impact"><p role="status">${esc(localized("Tracing impact for this exact snapshot…", "正在基于当前精确快照追踪影响…"))}</p></section>`;
  if (current.impactError) return `<section class="change-symbol-impact"><p class="bad" role="alert">${esc(current.impactError)}</p></section>`;
  const impact = current.relationImpact;
  if (!impact) return "";
  const syntax = impact.precision === "syntax";
  const title = syntax ? localized("Syntax impact candidates", "语法影响候选") : localized("Semantic impact relations", "语义影响关系");
  const note = syntax
    ? localized("LSP is unavailable; incoming callers are bounded Tree-sitter candidates. Candidates with a concrete syntax node can continue into the revision-bound Code Graph and source preview; that identity is navigation context, not semantic proof. Exact text matches are shown separately and are not call relations, semantic proof, or verification proof.", "LSP 不可用；入向调用方仅是有界 Tree-sitter 候选。带有明确语法节点的候选可以继续进入同版本绑定的代码图和源码预览；该身份只用于导航，不是语义证明。精确文本匹配会单独展示，它们不是调用关系、语义证明或验证证明。")
    : localized("Relations come from the live semantic provider for this exact source revision; they still do not prove verification success.", "这些关系来自当前精确源码版本的实时语义提供器，但仍不代表验证成功。");
  const selected = current.symbolImpact?.after_symbols?.find(symbol => symbol.node_id === impact.node_id);
  const selectedName = selected?.qualified_name || selected?.name || impact.node_id;
  const root = `<p class="panel-meta"><b>${esc(localized("Selected changed symbol", "所选变更符号"))}</b> <code>${esc(selectedName)}</code></p>`;
  const group = (label, relation, rows) => `<div><b>${esc(label)}</b>${rows.length ? rows.map(row => row.node_id ? `<button type="button" class="change-symbol-link" data-change-relation-node="${esc(row.node_id)}" aria-label="${esc(localized(`Open ${row.path}:${row.line} syntax caller in revision-bound Code Graph`, `在同版本代码图中打开 ${row.path}:${row.line} 语法调用方`))}"><code>${esc(row.path)}:${row.line}</code><span>${esc(row.name || "")}</span><span class="panel-meta">${esc(relation)} · ${esc(localized("Open in Code Graph · syntax node", "在代码图中打开 · 语法节点"))}</span></button>` : `<span class="change-symbol-before"><code>${esc(row.path)}:${row.line}</code><span>${esc(row.name || "")}</span><span class="panel-meta">${esc(relation)}</span></span>`).join("") : `<span class="panel-meta">${esc(localized("None observed", "未观测到"))}</span>`}</div>`;
  const searchGroup = syntax ? `<div><b>${esc(localized("Exact search matches", "精确文本匹配"))}</b>${impact.search_matches.length ? impact.search_matches.map(row => `<span class="change-symbol-before"><code>${esc(row.path)}:${row.line}</code><span>${esc(row.text)}</span><span class="panel-meta">${esc(localized("text mention, not a call relation", "文本提及，不是调用关系"))}</span></span>`).join("") : `<span class="panel-meta">${esc(localized("None observed", "未观测到"))}</span>`}</div>` : "";
  const partial = impact.partial ? `<p class="warn">${esc(localized("Impact relations are bounded/partial.", "影响关系有界或不完整。"))}</p>` : "";
  const incoming = group(localized("Incoming callers", "入向调用方"), localized("calls selected symbol", "调用所选符号"), impact.incoming_calls);
  const references = group(localized("References", "引用"), localized("references selected symbol", "引用所选符号"), impact.references);
  const implementations = group(localized("Implementations", "实现"), localized("implements selected symbol", "实现所选符号"), impact.implementations);
  return `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(note)}</p>${root}${partial}<div class="change-symbol-pair">${incoming}${references}</div><div class="change-symbol-pair">${implementations}${searchGroup}</div></section>`;
}
async function openChangeSymbolImpact(nodeId) {
  const current = state.changeInspection, view = current?.data, mapping = current?.symbolImpact;
  if (!current || !view || !validChangeRepositoryRevision(current.repositoryRevision)
    || !validChangeSymbolImpact(mapping, view)
    || mapping?.path !== view.path || mapping?.snapshot_id !== view.snapshot_id
    || mapping?.source_state !== "worktree" || mapping?.source_sha256 !== view.worktree_sha256
    || view.after_source_matches_worktree === false
    || !mapping.after_symbols?.some(symbol => symbol.node_id === nodeId && symbol.counterpart_only === false)) return false;
  current.impactController?.abort();
  const sequence = state.changeImpactSequence = (state.changeImpactSequence || 0) + 1;
  const workspace = state.current, epoch = state.workspaceEpoch, controller = new AbortController();
  current.impactController = controller; current.impactNodeId = nodeId; current.impactLoading = true;
  current.impactError = ""; current.relationImpact = null; renderChangeInspector();
  const params = new URLSearchParams({path: current.path, layer: current.layer,
    expected_snapshot: current.snapshotId, expected_code_revision: current.repositoryRevision.code, node_id: nodeId});
  if (current.repositoryRevision.design != null) params.set("expected_design_revision", current.repositoryRevision.design);
  const active = () => state.changeInspection === current && state.current === workspace
    && state.workspaceEpoch === epoch && state.changeImpactSequence === sequence && !controller.signal.aborted;
  try {
    const result = await uiJson(`/intelligence/change-impact?${params}`, "GET", undefined, {workspace, signal: controller.signal});
    if (!active()) return false;
    const revision = result.repository_revision, latestView = current.data, latestMapping = current.symbolImpact;
    if (result.workspace !== workspace || !validChangeRepositoryRevision(revision)
      || revision.code !== current.repositoryRevision.code
      || (revision.design ?? null) !== (current.repositoryRevision.design ?? null)
      || !latestView || latestView.path !== current.path || latestView.snapshot_id !== current.snapshotId
      || !validChangeSymbolImpact(latestMapping, latestView)
      || latestMapping?.source_state !== "worktree" || latestMapping?.source_sha256 !== latestView.worktree_sha256
      || latestView.after_source_matches_worktree === false
      || !latestMapping.after_symbols?.some(symbol => symbol.node_id === nodeId && symbol.counterpart_only === false)
      || !validChangeRelationImpact(result.impact, current, nodeId)) throw new Error(localized("Invalid change impact", "变更影响结果无效"));
    current.relationImpact = result.impact;
  } catch (error) {
    if (!active()) return false;
    current.impactError = error.status === 409
      ? localized("The file changed. Reload before tracing impact.", "文件已变化，请重新读取后再追踪影响。")
      : requestFailureMessage(error);
  } finally {
    if (active()) { current.impactLoading = false; renderChangeInspector(); }
  }
  return Boolean(current.relationImpact);
}

const codeSourceDrafts = new Map();
function codeSourceDraftKey(source) {
  return JSON.stringify([state.current, source.path, source.start_line, source.end_line]);
}
function codeSourceEditable(source) {
  return source?.editable === true && source.redacted === false
    && ["none", "lf", "crlf"].includes(source.line_ending)
    && /^[0-9a-f]{64}$/i.test(source.current_sha256 || "");
}
function codeSourceDraft(source) {
  return codeSourceDrafts.get(codeSourceDraftKey(source)) || [...codeSourceDrafts.values()].find(draft =>
    draft.workspace === state.current && draft.source.path === source.path && draft.status !== "saved");
}
function beginCodeSourceEdit(source) {
  if (!codeSourceEditable(source)) return false;
  const key = codeSourceDraftKey(source);
  let draft = codeSourceDrafts.get(key);
  if (!draft) {
    if (codeSourceDrafts.size >= 4) {
      const disposable = [...codeSourceDrafts].find(([, value]) => value.status === "saved");
      if (!disposable) return false;
      codeSourceDrafts.delete(disposable[0]);
    }
    draft = { key, workspace:state.current, source:{...source}, text:source.content.replace(/\r\n/g, "\n"),
      status:"editing", error:"", stamp:observationStamp() };
    codeSourceDrafts.set(key, draft);
  } else if (!draft.controller && draft.status === "editing") {
    draft.stamp = observationStamp();
    if (draft.source.current_sha256 !== source.current_sha256) {
      draft.status = "conflict";
      draft.error = localized("Source changed. Your draft is retained; compare it with current source.", "源码已变化。草稿已保留，请与当前源码比较。");
    }
  }
  renderCodeGraphInspector();
  return true;
}
function codeSourceEditorPanel(source) {
  const draft = codeSourceDraft(source);
  if (!draft) return codeSourceEditable(source)
    ? `<button type="button" class="code-source-edit" data-code-edit>${esc(localized("Edit this window", "编辑当前行区间"))}</button>` : "";
  const blocked = draft.status !== "editing" || draft.source.current_sha256 !== source.current_sha256
    || draft.source.start_line !== source.start_line || draft.source.end_line !== source.end_line;
  const pending = draft.status === "saving";
  const title = pending ? localized("Saving guarded edit…", "正在校验并保存…")
    : draft.status === "saved" ? localized("Saved. Reload current file to inspect changes.", "已保存。重新读取当前文件以检查变更。")
    : localized("Source draft", "源码草稿");
  return `<section class="code-source-editor"><strong>${esc(title)}</strong>
    <small>${esc(draft.source.path)} · ${draft.source.start_line}–${draft.source.end_line} · ${esc(draft.source.current_sha256.slice(0,12))}</small>
    <textarea data-code-draft spellcheck="false" aria-label="${esc(localized("Source draft", "源码草稿"))}" ${pending ? "readonly" : ""}>${esc(draft.text)}</textarea>
    <details><summary>${esc(localized("Compare original and draft", "比较原文与草稿"))}</summary>
      <div class="code-source-comparison"><div><b>${esc(localized("Original", "原文"))}</b><pre>${esc(draft.source.content)}</pre></div><div><b>${esc(localized("Draft", "草稿"))}</b><pre data-code-draft-diff>${esc(draft.text)}</pre></div></div>
    </details>
    ${draft.error ? `<p class="warn" role="status">${esc(draft.error)}</p>` : ""}
    <div class="code-source-edit-actions"><button type="button" class="primary" data-code-save ${blocked ? "disabled" : ""}>${esc(localized("Save this window", "保存当前行区间"))}</button>
    <button type="button" data-code-reload ${pending ? "disabled" : ""}>${esc(localized("Inspect current file", "检查当前文件"))}</button>
    <button type="button" data-code-discard ${pending ? "disabled" : ""}>${esc(localized("Discard draft", "丢弃草稿"))}</button></div>
    <p>${esc(localized("Only this line window changes. Concurrent edits are checked against the original SHA. A save does not establish verification.", "只修改当前行区间，保存时按原始 SHA 检查并发变动；保存不代表验证通过。"))}</p>
  </section>`;
}
function bindCodeSourceEditor(source) {
  const host = els.codeGraphInspector;
  host?.querySelector("[data-code-edit]")?.addEventListener("click", () => beginCodeSourceEdit(source));
  const draft = codeSourceDraft(source);
  if (!draft) return;
  host.querySelector("[data-code-draft]")?.addEventListener("input", event => {
    if (draft.status === "saving") return;
    draft.text = event.currentTarget.value;
    const preview = host.querySelector("[data-code-draft-diff]");
    if (preview) preview.textContent = draft.text;
  });
  host.querySelector("[data-code-save]")?.addEventListener("click", () => void saveCodeSourceDraft(draft));
  host.querySelector("[data-code-reload]")?.addEventListener("click", () => void openRepositoryFile(source.path));
  host.querySelector("[data-code-discard]")?.addEventListener("click", () => {
    if (draft.status === "saving") return;
    codeSourceDrafts.delete(draft.key);
    renderCodeGraphInspector();
  });
}
async function saveCodeSourceDraft(draft) {
  if (!draft || draft.status !== "editing" || !observationCurrent(draft.stamp)
      || draft.workspace !== state.current || state.codeGraphSource?.current_sha256 !== draft.source.current_sha256
      || state.codeGraphSource?.start_line !== draft.source.start_line || state.codeGraphSource?.end_line !== draft.source.end_line
      || codeSourceDraft(state.codeGraphSource) !== draft) return false;
  if (new TextEncoder().encode(draft.text).length > 131072 || draft.text.split("\n").length > 240) {
    draft.error = localized("Draft exceeds 240 lines or 128 KiB.", "草稿超过 240 行或 128 KiB。");
    renderCodeGraphInspector(); return false;
  }
  const controller = new AbortController(), stamp = observationStamp(), source = draft.source;
  draft.status = "saving"; draft.error = ""; draft.controller = controller;
  renderCodeGraphInspector();
  const current = () => observationCurrent(stamp) && codeSourceDrafts.get(draft.key) === draft
    && draft.controller === controller && state.codeGraphSource?.current_sha256 === source.current_sha256
    && codeSourceDraft(state.codeGraphSource) === draft;
  try {
    const result = await uiJson("/intelligence/code-source", "POST", {
      node_id:source.node_id, snapshot_id:source.snapshot_id, expected_sha256:source.current_sha256,
      start_line:source.start_line, end_line:source.end_line, old_text:source.content, new_text:draft.text,
    }, { workspace:draft.workspace, signal:controller.signal, timeout:60000 });
    if (result?.workspace !== draft.workspace || result?.code !== "source_updated"
        || result?.edit?.path !== source.path || result?.edit?.sha256_before !== source.current_sha256
        || !/^[0-9a-f]{64}$/i.test(result?.edit?.sha256_after || "")
        || !Number.isSafeInteger(result?.edit?.bytes_written) || result.edit.bytes_written < 0) {
      throw new Error("Invalid guarded edit response");
    }
    const apply = current();
    draft.status = "saved";
    if (apply) {
      renderCodeGraphInspector();
      void openRepositoryFile(source.path);
      void refresh();
    }
    return true;
  } catch (error) {
    draft.status = error.status === 409 ? "conflict" : "uncertain";
    draft.error = error.status === 409
      ? localized("Source changed. Draft retained; inspect and compare current source.", "源码已变化，草稿已保留；请检查并比较当前源码。")
      : localized("Save was not confirmed. Draft retained. Inspect current source before any new save.", "保存尚未确认，草稿已保留。再次保存前请先检查当前源码。");
    if (current()) renderCodeGraphInspector();
    return false;
  } finally {
    if (draft.controller === controller) {
      draft.controller = null;
      if (observationCurrent(stamp) && codeSourceDrafts.get(draft.key) === draft) renderCodeGraphInspector();
    }
  }
}
