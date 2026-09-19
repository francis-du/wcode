function setCodeGraphFull(full) {
  const active = Boolean(full);
  state.codeGraphFull = active;
  const section = els.codeGraphSection;
  section?.classList.toggle("code-graph-fullscreen", active);
  if (active) {
    section?.setAttribute("role", "dialog");
    section?.setAttribute("aria-modal", "true");
  } else {
    section?.removeAttribute("role");
    section?.removeAttribute("aria-modal");
  }
  els.codeGraphFull?.setAttribute("aria-pressed", String(active));
  const label = active
    ? localized("Exit full screen code graph", "退出代码图谱全屏")
    : localized("Full screen code graph", "全屏查看代码图谱");
  els.codeGraphFull?.setAttribute("aria-label", label);
  if (els.codeGraphFull) els.codeGraphFull.title = label;
  document.documentElement.classList.toggle("code-graph-fullscreen-open", active);
}
function codeGraphPath(node) {
  return node?.attributes?.path || "";
}
function codeGraphPrecisionCounts(graph) {
  return Object.entries(graph?.precision_counts || {}).sort((a, b) => b[1] - a[1]);
}
function codeGraphPathChanged(node) {
  const path = codeGraphPath(node);
  return Boolean(path && (state.project?.changes || []).some(change => change.path === path));
}
function syncCodeGraphSnapshots() {
  if (!els.codeGraphSnapshot) return;
  const history = (state.project?.history || []).filter(item =>
    item && typeof item === "object" && !Array.isArray(item) &&
    typeof item.id === "string" && item.id.length > 0 &&
    Number.isFinite(Number(item.captured_at_ms))
  );
  const ids = new Set(history.map(item => item.id));
  if (state.codeGraphSnapshot && !ids.has(state.codeGraphSnapshot)) state.codeGraphSnapshot = "";
  const options = [`<option value="">${esc(localized("Latest graph", "最新图谱"))}</option>`]
    .concat(history.map(item => {
      const stamp = item.captured_at_ms ? new Date(Number(item.captured_at_ms)).toLocaleString(state.language === "zh-CN" ? "zh-CN" : "en") : item.id;
      return `<option value="${esc(item.id)}" ${item.id === state.codeGraphSnapshot ? "selected" : ""}>${esc(stamp)} · ${esc(item.id.slice(0, 10))}</option>`;
    }));
  setHtml("codeGraphSnapshots", els.codeGraphSnapshot, options.join(""));
}
function codeGraphRelationSummary(graph) {
  const counts = new Map();
  for (const edge of graph?.edges || []) counts.set(edge.kind || "relation", (counts.get(edge.kind || "relation") || 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8);
}
function codeGraphShortText(value, limit) {
  const text = String(value || "");
  return text.length > limit ? `${text.slice(0, Math.max(1, limit - 1))}…` : text;
}
function codeGraphLayer(item, roots) {
  const node = item.node || {};
  if (roots.has(node.id) || (item.upstream && item.downstream)) return 0;
  const distance = Math.max(1, Number(item.distance) || 1);
  return item.upstream ? -distance : distance;
}
function codeGraphLayout(graph) {
  const roots = new Set(graph.root_ids || []);
  const groups = new Map();
  for (const item of graph.nodes || []) {
    const layer = codeGraphLayer(item, roots);
    if (!groups.has(layer)) groups.set(layer, []);
    groups.get(layer).push(item);
  }
  const layers = [...groups.keys()].sort((a, b) => a - b);
  const nodeWidth = 210, nodeHeight = 72, rowGap = 26, columnGap = 38, layerGap = 86;
  const maxRows = 8, padX = 44, padY = 44;
  const positions = new Map();
  let cursorX = padX, maxRowsUsed = 1;
  for (const layer of layers) {
    const items = groups.get(layer);
    items.sort((a, b) => {
      const left = String(a.node?.label || a.node?.id || "");
      const right = String(b.node?.label || b.node?.id || "");
      return left < right ? -1 : left > right ? 1 : 0;
    });
    const columns = Math.max(1, Math.ceil(items.length / maxRows));
    const bandWidth = columns * nodeWidth + Math.max(0, columns - 1) * columnGap;
    items.forEach((item, index) => {
      const column = Math.floor(index / maxRows), row = index % maxRows;
      positions.set(item.node.id, {
        x: cursorX + column * (nodeWidth + columnGap),
        y: padY + row * (nodeHeight + rowGap),
        layer,
        item,
      });
    });
    maxRowsUsed = Math.max(maxRowsUsed, Math.min(maxRows, items.length));
    cursorX += bandWidth + layerGap;
  }
  return {
    roots,
    positions,
    nodeWidth,
    nodeHeight,
    width: Math.max(760, cursorX - layerGap + padX),
    height: Math.max(420, padY * 2 + maxRowsUsed * nodeHeight + Math.max(0, maxRowsUsed - 1) * rowGap),
  };
}
function codeGraphEdgePath(edge, layout) {
  const from = layout.positions.get(edge.from), to = layout.positions.get(edge.to);
  if (!from || !to) return "";
  const sameBand = Math.abs(from.x - to.x) < 8;
  if (sameBand) {
    const x1 = from.x + layout.nodeWidth / 2, y1 = from.y + layout.nodeHeight;
    const x2 = to.x + layout.nodeWidth / 2, y2 = to.y;
    const bend = Math.max(52, Math.abs(y2 - y1) * .35);
    return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 + bend} ${y2}, ${x2} ${y2}`;
  }
  const forward = to.x >= from.x;
  const x1 = from.x + (forward ? layout.nodeWidth : 0), y1 = from.y + layout.nodeHeight / 2;
  const x2 = to.x + (forward ? 0 : layout.nodeWidth), y2 = to.y + layout.nodeHeight / 2;
  const bend = Math.max(48, Math.abs(x2 - x1) * .46), sign = forward ? 1 : -1;
  return `M ${x1} ${y1} C ${x1 + bend * sign} ${y1}, ${x2 - bend * sign} ${y2}, ${x2} ${y2}`;
}
function codeGraphDiagram(graph) {
  const layout = codeGraphLayout(graph);
  const selected = state.selectedCodeNode || graph.root_ids?.[0] || "";
  const nodeById = new Map(graph.nodes.map(item => [item.node.id, item.node]));
  const edges = (graph.edges || []).map(edge => {
    const path = codeGraphEdgePath(edge, layout);
    if (!path) return "";
    const precision = edge.provenance?.precision || "unknown";
    const provider = edge.provenance?.provider || "unknown";
    const active = edge.from === selected || edge.to === selected;
    const from = nodeById.get(edge.from), to = nodeById.get(edge.to);
    const title = `${from?.label || edge.from} → ${to?.label || edge.to} · ${statusLabel(edge.kind || "relation")} · ${precision} · ${provider}`;
    return `<path class="code-graph-edge-path${active ? " selected" : ""}" data-edge-from="${esc(edge.from)}" data-edge-to="${esc(edge.to)}" data-precision="${esc(precision)}" d="${path}" marker-end="url(#codeGraphArrow)"><title>${esc(title)}</title></path>`;
  }).join("");
  const nodes = graph.nodes.map(item => {
    const node = item.node || {}, position = layout.positions.get(node.id);
    if (!position) return "";
    const root = layout.roots.has(node.id);
    const changed = codeGraphPathChanged(node);
    const classes = ["code-graph-svg-node", root ? "root" : "", selected === node.id ? "selected" : "", changed ? "changed" : ""].filter(Boolean).join(" ");
    const direction = root ? localized("focus", "中心") : item.upstream ? `↑${item.distance || 1}` : `↓${item.distance || 1}`;
    const label = codeGraphShortText(node.label || node.id || "—", 27);
    const path = codeGraphShortText(codeGraphPath(node), 34);
    const title = [node.label || node.id, codeGraphPath(node), statusLabel(node.kind || "symbol")].filter(Boolean).join(" · ");
    return `<g class="${classes}" transform="translate(${position.x} ${position.y})" data-code-node="${esc(node.id || "")}" role="button" tabindex="0" aria-label="${esc(title)}">
      <title>${esc(title)}</title>
      <rect width="${layout.nodeWidth}" height="${layout.nodeHeight}" rx="14"></rect>
      ${changed ? `<line class="code-graph-node-change" x1="1" y1="14" x2="1" y2="${layout.nodeHeight - 14}"></line>` : ""}
      <text class="code-graph-svg-kind" x="14" y="19">${esc(statusLabel(node.kind || "symbol"))}</text>
      <text class="code-graph-svg-distance" x="${layout.nodeWidth - 14}" y="19" text-anchor="end">${esc(direction)}</text>
      <text class="code-graph-svg-label" x="14" y="43">${esc(label)}</text>
      ${path ? `<text class="code-graph-svg-path" x="14" y="61">${esc(path)}</text>` : ""}
    </g>`;
  }).join("");
  return `<div class="code-graph-viewport" role="region" aria-label="${esc(localized("Interactive code graph", "交互式代码图谱"))}" tabindex="0">
    <svg class="code-graph-diagram" width="${layout.width}" height="${layout.height}" viewBox="0 0 ${layout.width} ${layout.height}" aria-label="${esc(localized("Code dependency graph", "代码依赖图"))}">
      <defs>
        <pattern id="codeGraphGrid" width="28" height="28" patternUnits="userSpaceOnUse"><path d="M 28 0 L 0 0 0 28" class="code-graph-grid-line"></path></pattern>
        <marker id="codeGraphArrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L8,4 L0,8 z" class="code-graph-arrow"></path></marker>
      </defs>
      <rect class="code-graph-grid" width="${layout.width}" height="${layout.height}" fill="url(#codeGraphGrid)"></rect>
      <g class="code-graph-edges">${edges}</g>
      <g class="code-graph-nodes">${nodes}</g>
    </svg>
  </div>`;
}
function selectCodeGraphNode(nodeId) {
  state.selectedCodeNode = nodeId;
  els.codeGraphMap?.querySelectorAll("[data-code-node]").forEach(node =>
    node.classList.toggle("selected", node.dataset.codeNode === nodeId));
  els.codeGraphMap?.querySelectorAll("[data-edge-from]").forEach(edge =>
    edge.classList.toggle("selected", edge.dataset.edgeFrom === nodeId || edge.dataset.edgeTo === nodeId));
  renderCodeGraphInspector();
}
function renderCodeGraphInspector() {
  if (!els.codeGraphInspector) return;
  const graph = state.codeGraph;
  if (!graph?.nodes?.length) {
    els.codeGraphInspector.innerHTML = `<div class="empty">${esc(localized("Select a code node to inspect provenance and relationships.", "选择代码节点以查看来源与关系。"))}</div>`;
    return;
  }
  const item = graph.nodes.find(entry => entry.node?.id === state.selectedCodeNode)
    || graph.nodes.find(entry => graph.root_ids?.includes(entry.node?.id))
    || graph.nodes[0];
  state.selectedCodeNode = item.node.id;
  const node = item.node, edges = (graph.edges || []).filter(edge => edge.from === node.id || edge.to === node.id);
  const nodeById = new Map((graph.nodes || []).map(entry => [entry.node?.id, entry.node]));
  const precision = node.provenance?.precision || "unknown", provider = node.provenance?.provider || "unknown";
  const why = edges.slice(0, 10).map(edge => {
    const outgoing = edge.from === node.id;
    const otherId = outgoing ? edge.to : edge.from;
    const other = nodeById.get(otherId);
    const edgePrecision = edge.provenance?.precision || "unknown";
    const edgeProvider = edge.provenance?.provider || "unknown";
    return `<div><span>${esc(outgoing ? localized("outgoing", "下游") : localized("incoming", "上游"))} · ${esc(statusLabel(edge.kind || "relation"))}</span><b>${esc(other?.label || otherId)}</b><small>${esc(edgePrecision)} · ${esc(edgeProvider)}</small></div>`;
  }).join("");
  const direction = graph.root_ids?.includes(node.id)
    ? localized("focus node", "中心节点")
    : item.upstream && item.downstream
      ? localized("upstream + downstream", "上游 + 下游")
      : item.upstream
        ? localized("upstream / blast radius", "上游 / 影响半径")
        : localized("downstream / dependency", "下游 / 依赖");
  els.codeGraphInspector.innerHTML = `<div class="inspector-kind">${esc(statusLabel(node.kind || "symbol"))}</div>
    <h3>${esc(node.label || node.id)}</h3>
    <div class="inspector-path">${esc(codeGraphPath(node) || node.id)}</div>
    <div class="pills">
      <span class="code-graph-chip accent">${esc(direction)}</span>
      <span class="code-graph-chip">${esc(precision)}</span>
      <span class="code-graph-chip">${edges.length} ${esc(localized("relations", "条关系"))}</span>
    </div>
    <dl>
      <dt>${esc(localized("Provider", "Provider"))}</dt><dd>${esc(provider)}</dd>
      <dt>${esc(localized("Precision", "精度"))}</dt><dd>${esc(precision)}</dd>
      <dt>${esc(localized("Distance", "距离"))}</dt><dd>${item.distance || 0}</dd>
      <dt>${esc(localized("Node ID", "节点 ID"))}</dt><dd><code>${esc(node.id)}</code></dd>
      <dt>${esc(localized("Changed now", "当前变更"))}</dt><dd>${esc(codeGraphPathChanged(node) ? localized("yes", "是") : localized("no", "否"))}</dd>
    </dl>
    <div class="code-graph-why"><strong>${esc(localized("Why this node is related", "为什么这个节点相关"))}</strong>${why || `<p>${esc(localized("No retained relation touches this node in the current bounded graph.", "当前有界图谱中没有保留与该节点相连的关系。"))}</p>`}</div>
    <button type="button" class="primary code-graph-focus" data-code-focus="${esc(node.id)}">${esc(localized("Focus graph here", "以此节点为中心"))}</button>`;
  els.codeGraphInspector.querySelector("[data-code-focus]")?.addEventListener("click", event => {
    void loadCodeGraph({ nodeId: event.currentTarget.dataset.codeFocus });
  });
}
function codeGraphSourcePath(value) {
  const text = String(value || "").trim();
  return text.includes("::") ? text.slice(0, text.lastIndexOf("::")) : text;
}
function codeGraphIsCodePath(value) {
  const path = codeGraphSourcePath(value);
  if (!path || path.startsWith(".wcode/") || path.startsWith(".git/")) return false;
  const entries = state.project?.structure?.entries || [];
  if (entries.some(item => (typeof item === "string" ? item : item?.path) === path)) return true;
  return /\.(?:rs|js|mjs|cjs|jsx|ts|tsx|py|go|java|kt|kts|c|h|cc|cpp|cxx|hpp|hh|cs|swift|rb|php|dart|ex|exs|erl|hrl|lua|r|sh|bash|zsh|fish|scala|clj|cljs|ml|mli|html?|css|scss|sass|less|vue|svelte)$/i.test(path);
}
function codeGraphLooksLikeDesignPath(value) {
  const path = codeGraphSourcePath(value);
  return path.startsWith(".wcode/design/") || path === ".wcode/project.yaml" || path === ".wcode/project.yml";
}
function codeGraphSuggestedQueries() {
  const candidates = [], seen = new Set();
  const add = value => {
    const query = String(value || "").trim();
    if (query.length < 2 || seen.has(query)) return;
    seen.add(query); candidates.push(query);
  };
  for (const change of state.project?.changes || []) if (codeGraphIsCodePath(change?.path)) add(change.path);
  for (const component of state.project?.architecture?.components || []) {
    for (const target of component?.implementation_targets || []) {
      const value = String(target || "").trim();
      if (!codeGraphIsCodePath(value)) continue;
      if (value.includes("::")) {
        add(value.split("::").pop());
        add(codeGraphSourcePath(value));
      } else add(value);
    }
  }
  for (const entry of state.project?.structure?.entries || []) {
    const path = typeof entry === "string" ? entry : entry?.path;
    if (codeGraphIsCodePath(path)) add(path);
  }
  return candidates.slice(0, 6);
}
function codeGraphEmptyActions() {
  const suggestions = codeGraphSuggestedQueries();
  if (!suggestions.length) return "";
  return `<div class="code-graph-suggestions"><small>${esc(localized("Start from observed project signals", "从当前项目真实信号开始"))}</small><div>${suggestions.map(query => `<button type="button" data-code-graph-query="${esc(query)}"><span>${esc(codeGraphShortText(query, 44))}</span><b aria-hidden="true">→</b></button>`).join("")}</div></div>`;
}
function wireCodeGraphEmptyActions() {
  els.codeGraphMap?.querySelectorAll("[data-code-graph-query]").forEach(button => button.addEventListener("click", () => {
    const query = button.dataset.codeGraphQuery || "";
    if (!query || !els.codeGraphSearch) return;
    els.codeGraphSearch.value = query;
    void loadCodeGraph();
  }));
}
function renderCodeGraph() {
  if (!els.codeGraphMap || !els.codeGraphSummary) return;
  syncCodeGraphSnapshots();
  const graph = state.codeGraph;
  if (state.codeGraphLoading && !graph) {
    els.codeGraphMap.innerHTML = `<div class="code-graph-empty code-graph-loading"><div><strong>${esc(localized("Building the engineering graph…", "正在构建工程图谱…"))}</strong><span>${esc(localized("Calls, impact and provenance stay bounded by the selected depth.", "调用、影响与来源关系会按选定深度有界展开。"))}</span></div></div>`;
    return;
  }
  if (!graph?.nodes?.length) {
    const error = state.codeGraphError ? `<div class="bad">${esc(state.codeGraphError)}</div>` : "";
    els.codeGraphSummary.innerHTML = "";
    els.codeGraphMap.innerHTML = `<div class="code-graph-empty"><div><strong>${esc(localized("Search the living code graph", "搜索活的代码图谱"))}</strong><span>${esc(localized("Function, class, module or file. WCode will show callers, callees, impact paths and evidence provenance.", "输入函数、类、模块或文件。WCode 会展示调用者、被调用者、影响路径与证据来源。"))}</span>${error}${codeGraphEmptyActions()}</div></div>`;
    wireCodeGraphEmptyActions();
    renderCodeGraphInspector();
    return;
  }
  const precision = codeGraphPrecisionCounts(graph), relations = codeGraphRelationSummary(graph);
  els.codeGraphSummary.innerHTML = `<div class="code-graph-summary-main">
    <strong>${graph.nodes.length} ${esc(localized("nodes", "个节点"))} · ${graph.edges.length} ${esc(localized("edges", "条边"))}</strong>
    <span class="code-graph-chip warn">${graph.upstream_nodes || 0} ${esc(localized("upstream", "上游"))}</span>
    <span class="code-graph-chip good">${graph.downstream_nodes || 0} ${esc(localized("downstream", "下游"))}</span>
    ${graph.truncated ? `<span class="code-graph-chip warn">${esc(t("truncated"))}</span>` : ""}
  </div><div class="pills">${precision.map(([name, count]) => `<span class="code-graph-chip">${esc(name)} ${count}</span>`).join("")}${relations.map(([name, count]) => `<span class="code-graph-chip accent">${esc(statusLabel(name))} ${count}</span>`).join("")}</div>`;
  els.codeGraphMap.innerHTML = `${codeGraphDiagram(graph)}
    <div class="code-graph-context">
      <div class="code-graph-context-card"><strong>${esc(localized("Graph contract", "图谱契约"))}</strong><span>${esc(localized("Every relation keeps provider and precision. Syntax, semantic and runtime evidence are never flattened into one confidence claim.", "每条关系保留 provider 与 precision；syntax、semantic、runtime 证据不会被混成一个置信结论。"))}</span></div>
      <div class="code-graph-context-card"><strong>${esc(localized("Bounded exploration", "有界探索"))}</strong><span>${esc(localized(`Depth ${graph.depth} · mode ${graph.mode} · snapshot ${graph.snapshot_id}`, `深度 ${graph.depth} · 模式 ${graph.mode} · 快照 ${graph.snapshot_id}`))}</span></div>
    </div>`;
  els.codeGraphMap.querySelectorAll("[data-code-node]").forEach(node => {
    node.addEventListener("click", () => selectCodeGraphNode(node.dataset.codeNode));
    node.addEventListener("keydown", event => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        selectCodeGraphNode(node.dataset.codeNode);
      }
    });
    node.addEventListener("dblclick", () => {
      void loadCodeGraph({ nodeId: node.dataset.codeNode });
    });
  });
  renderCodeGraphInspector();
}
function validCodeGraphResponse(data) {
  const graph = data?.graph;
  const precisions = ["declared", "syntax", "semantic", "runtime", "deterministic", "heuristic", "mixed"];
  const nodeKinds = ["package", "module", "file", "symbol", "function", "struct", "trait", "class", "interface", "api", "test"];
  const edgeKinds = ["contains", "defines", "references", "calls", "imports", "depends_on", "implements", "extends", "runtime_calls"];
  const nonNegativeInteger = value => Number.isInteger(Number(value)) && Number(value) >= 0;
  if (!(
    graph && typeof graph === "object" && !Array.isArray(graph) &&
    typeof graph.snapshot_id === "string" && graph.snapshot_id.length > 0 &&
    nonNegativeInteger(graph.captured_at_ms) &&
    typeof graph.provider === "string" && graph.provider.length > 0 &&
    precisions.includes(graph.precision) &&
    typeof graph.query === "string" && graph.query.trim().length > 0 &&
    ["calls", "impact", "all"].includes(graph.mode) &&
    Number.isInteger(Number(graph.depth)) && Number(graph.depth) >= 1 && Number(graph.depth) <= 4 &&
    Array.isArray(graph.root_ids) && graph.root_ids.length >= 1 && graph.root_ids.length <= 8 &&
    graph.root_ids.every(id => typeof id === "string" && id.length > 0) &&
    new Set(graph.root_ids).size === graph.root_ids.length &&
    Array.isArray(graph.nodes) && graph.nodes.length >= 1 && graph.nodes.length <= 140 &&
    Array.isArray(graph.edges) && graph.edges.length <= 420 &&
    nonNegativeInteger(graph.upstream_nodes) && Number(graph.upstream_nodes) <= graph.nodes.length &&
    nonNegativeInteger(graph.downstream_nodes) && Number(graph.downstream_nodes) <= graph.nodes.length &&
    typeof graph.truncated === "boolean" &&
    graph.precision_counts && typeof graph.precision_counts === "object" &&
    !Array.isArray(graph.precision_counts) &&
    Object.entries(graph.precision_counts).every(([precision, count]) =>
      precisions.includes(precision) && nonNegativeInteger(count)
    )
  )) return false;
  const provenanceValid = provenance =>
    provenance && typeof provenance === "object" && !Array.isArray(provenance) &&
    typeof provenance.provider === "string" && provenance.provider.length > 0 &&
    precisions.includes(provenance.precision) &&
    typeof provenance.revision === "string" && provenance.revision.length > 0;
  const nodeIds = new Set();
  let upstreamNodes = 0, downstreamNodes = 0;
  for (const item of graph.nodes) {
    const node = item?.node;
    if (!(
      item && typeof item === "object" && !Array.isArray(item) &&
      node && typeof node === "object" && !Array.isArray(node) &&
      typeof node.id === "string" && node.id.length > 0 &&
      nodeKinds.includes(node.kind) &&
      typeof node.label === "string" &&
      (node.attributes === undefined ||
        (node.attributes && typeof node.attributes === "object" && !Array.isArray(node.attributes))) &&
      provenanceValid(node.provenance) &&
      Number.isInteger(Number(item.distance)) && Number(item.distance) >= 0 &&
      Number(item.distance) <= Number(graph.depth) &&
      typeof item.upstream === "boolean" && typeof item.downstream === "boolean"
    ) || nodeIds.has(node.id)) return false;
    if (item.upstream) upstreamNodes++;
    if (item.downstream) downstreamNodes++;
    nodeIds.add(node.id);
  }
  if (upstreamNodes !== Number(graph.upstream_nodes) || downstreamNodes !== Number(graph.downstream_nodes)) return false;
  if (!graph.root_ids.every(id => nodeIds.has(id))) return false;
  return graph.edges.every(edge =>
    edge && typeof edge === "object" && !Array.isArray(edge) &&
    typeof edge.from === "string" && nodeIds.has(edge.from) &&
    typeof edge.to === "string" && nodeIds.has(edge.to) &&
    edgeKinds.includes(edge.kind) &&
    provenanceValid(edge.provenance)
  );
}
function codeGraphResponseMatchesRequest(graph, expected) {
  return graph.query === expected.query &&
    graph.mode === expected.mode &&
    Number(graph.depth) === expected.depth &&
    (!expected.snapshot || graph.snapshot_id === expected.snapshot);
}
function clearCodeGraphSearchState() {
  const controller = state.codeGraphController;
  state.codeGraphController = null;
  state.codeGraphLoading = false;
  controller?.abort();
  state.codeGraph = null;
  state.codeGraphWorkspace = "";
  state.codeGraphQuery = "";
  state.selectedCodeNode = "";
  state.codeGraphError = "";
  renderCodeGraph();
}
async function loadCodeGraph({ nodeId, query: requestedQuery } = {}) {
  const inputQuery = (els.codeGraphSearch?.value || "").trim();
  const query = nodeId
    ? (state.codeGraphQuery || inputQuery).trim()
    : String(requestedQuery ?? inputQuery).trim();
  if (!nodeId && query.length < 2) {
    clearCodeGraphSearchState();
    return false;
  }
  if (!nodeId && codeGraphLooksLikeDesignPath(query)) {
    state.codeGraph = null;
    state.codeGraphWorkspace = "";
    state.selectedCodeNode = "";
    state.codeGraphError = localized("Code Graph only shows source-code entities and relationships. Search a source file, function, type or module instead.", "代码图谱只展示源码实体和代码关系。请搜索源码文件、函数、类型或模块。");
    renderCodeGraph();
    return false;
  }
  const expected = {
    query: nodeId || query,
    mode: state.codeGraphMode || "all",
    depth: Number(state.codeGraphDepth || 2),
    snapshot: state.codeGraphSnapshot || "",
  };
  state.codeGraphController?.abort();
  const controller = new AbortController();
  state.codeGraphController = controller;
  state.codeGraphLoading = true; state.codeGraphError = ""; renderCodeGraph();
  const params = new URLSearchParams();
  if (nodeId) params.set("node_id", nodeId); else params.set("q", query);
  params.set("mode", expected.mode);
  if (expected.snapshot) params.set("snapshot_id", expected.snapshot);
  params.set("depth", String(expected.depth));
  params.set("limit", "140");
  try {
    const data = await uiJson(`/intelligence/code-graph?${params}`, "GET", undefined, {
      workspace: state.current,
      signal: controller.signal,
      timeout: 30000,
    });
    if (controller.signal.aborted || data.workspace !== state.current) return false;
    if (!validCodeGraphResponse(data) || !codeGraphResponseMatchesRequest(data.graph, expected)) {
      const invalid = new Error(localized("Invalid code graph response", "代码图谱响应无效"));
      invalid.code = "invalid_response";
      throw invalid;
    }
    state.codeGraph = data.graph; state.codeGraphWorkspace = data.workspace;
    state.codeGraphQuery = nodeId ? data.graph?.query || query : query;
    state.selectedCodeNode = data.graph?.root_ids?.[0] || "";
    renderCodeGraph();
    return true;
  } catch (error) {
    if (!controller.signal.aborted) {
      state.codeGraph = null;
      state.codeGraphWorkspace = "";
      state.selectedCodeNode = "";
      state.codeGraphError = error?.status === 400 && /no code graph symbol matches the requested chain root/i.test(error?.message || "")
        ? localized("No observable graph node matches this query yet. Try a project signal below or refresh the latest graph.", "当前还没有可观测图谱节点匹配该查询。可从下方项目真实信号进入，或刷新最新图谱。")
        : requestFailureMessage(error);
      renderCodeGraph();
    }
    return false;
  } finally {
    if (state.codeGraphController === controller) {
      state.codeGraphController = null; state.codeGraphLoading = false;
      renderCodeGraph();
    }
  }
}
function codeGraphDefaultQuery() {
  const changed = state.project?.changes?.find(change => codeGraphIsCodePath(change?.path))?.path;
  if (changed) return changed;
  const components = state.project?.architecture?.components || [];
  const component = components.find(item => item.id === state.selectedComponent &&
      (item.implementation_targets || []).length)
    || components.find(item => (item.implementation_targets || []).length);
  const target = (component?.implementation_targets || []).find(codeGraphIsCodePath);
  if (target) {
    const value = String(target).trim();
    const symbol = value.includes("::") ? value.split("::").pop().trim() : "";
    return symbol.length >= 2 ? symbol : value;
  }
  const entry = (state.project?.structure?.entries || []).find(item => {
    const path = typeof item === "string" ? item : item?.path;
    return codeGraphIsCodePath(path);
  });
  return typeof entry === "string" ? entry : String(entry?.path || "");
}
function maybeLoadCodeGraph() {
  if (state.workspaceTab !== "architecture" || state.architectureView !== "codegraph" || state.codeGraphLoading) return;
  if (state.codeGraph && state.codeGraphWorkspace === state.current) {
    renderCodeGraph();
    return;
  }
  // Paint the empty/loading state first. Automatic discovery may use a bounded
  // source-code seed internally, but the search box remains user-owned input.
  renderCodeGraph();
  const inputQuery = (els.codeGraphSearch?.value || "").trim();
  if (inputQuery.length >= 2) {
    void loadCodeGraph();
    return;
  }
  const seed = codeGraphDefaultQuery();
  if (seed) void loadCodeGraph({ query: seed });
}
function reloadCodeGraphFromCurrentContext() {
  const inputQuery = (els.codeGraphSearch?.value || "").trim();
  if (inputQuery) return void loadCodeGraph();
  if (state.selectedCodeNode) return void loadCodeGraph({ nodeId: state.selectedCodeNode });
  if (state.codeGraphQuery) void loadCodeGraph({ query: state.codeGraphQuery });
}
function wireCodeGraph() {
  if (!els.codeGraphSearch) return;
  els.codeGraphSearch.addEventListener("keydown", event => {
    if (event.key === "Enter") { event.preventDefault(); void loadCodeGraph(); }
  });
  els.codeGraphSearch.addEventListener("input", () => {
    if (!(els.codeGraphSearch.value || "").trim()) clearCodeGraphSearchState();
  });
  els.codeGraphSearch.addEventListener("search", () => {
    if ((els.codeGraphSearch.value || "").trim()) void loadCodeGraph();
    else clearCodeGraphSearchState();
  });
  document.querySelectorAll("[data-code-graph-mode]").forEach(button => {
    button.addEventListener("click", () => {
      state.codeGraphMode = button.dataset.codeGraphMode;
      document.querySelectorAll("[data-code-graph-mode]").forEach(item => item.classList.toggle("active", item === button));
      reloadCodeGraphFromCurrentContext();
    });
  });
  els.codeGraphSnapshot?.addEventListener("change", () => {
    state.codeGraphSnapshot = els.codeGraphSnapshot.value || "";
    state.selectedCodeNode = "";
    reloadCodeGraphFromCurrentContext();
  });
  els.codeGraphDepth?.addEventListener("change", () => {
    state.codeGraphDepth = Number(els.codeGraphDepth.value) || 2;
    reloadCodeGraphFromCurrentContext();
  });
  els.codeGraphFull?.addEventListener("click", () => setCodeGraphFull(!state.codeGraphFull));
}
