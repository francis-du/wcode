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
  const history = state.project?.history || [];
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
function codeGraphNodeCard(item, roots) {
  const node = item.node || {}, root = roots.has(node.id);
  const classes = [
    "code-graph-node",
    root ? "root" : "",
    state.selectedCodeNode === node.id ? "selected" : "",
    codeGraphPathChanged(node) ? "changed" : "",
  ].filter(Boolean).join(" ");
  return `<button type="button" class="${classes}" data-code-node="${esc(node.id || "")}">
    <span class="code-graph-node-head"><span class="code-graph-node-kind">${esc(statusLabel(node.kind || "symbol"))}</span><span class="code-graph-node-distance">${root ? localized("focus", "中心") : `+${item.distance || 0}`}</span></span>
    <span class="code-graph-node-label">${esc(node.label || node.id || "—")}</span>
    ${codeGraphPath(node) ? `<span class="code-graph-node-path">${esc(codeGraphPath(node))}</span>` : ""}
  </button>`;
}
function codeGraphLane(title, items, roots) {
  return `<section class="code-graph-lane"><div class="code-graph-lane-title"><span>${esc(title)}</span><span>${items.length}</span></div>${items.map(item => codeGraphNodeCard(item, roots)).join("") || `<div class="empty">${esc(localized("No nodes in this direction.", "这个方向没有节点。"))}</div>`}</section>`;
}
function codeGraphEdgeRow(edge, nodeById) {
  const from = nodeById.get(edge.from), to = nodeById.get(edge.to);
  const precision = edge.provenance?.precision || "unknown";
  const provider = edge.provenance?.provider || "unknown";
  return `<div class="code-graph-edge">
    <code title="${esc(from?.label || edge.from)}">${esc(from?.label || edge.from)}</code>
    <div class="code-graph-edge-meta"><span class="code-graph-edge-kind">${esc(statusLabel(edge.kind || "relation"))}</span><br>${esc(precision)} · ${esc(provider)}</div>
    <code title="${esc(to?.label || edge.to)}">${esc(to?.label || edge.to)}</code>
  </div>`;
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
    els.codeGraphMap.innerHTML = `<div class="code-graph-empty"><div><strong>${esc(localized("Search the living code graph", "搜索活的代码图谱"))}</strong><span>${esc(localized("Function, class, module or file. WCode will show callers, callees, impact paths and evidence provenance.", "输入函数、类、模块或文件。WCode 会展示调用者、被调用者、影响路径与证据来源。"))}</span>${error}</div></div>`;
    renderCodeGraphInspector();
    return;
  }
  const roots = new Set(graph.root_ids || []);
  const upstream = [], focus = [], downstream = [];
  for (const item of graph.nodes) {
    if (roots.has(item.node.id) || (item.upstream && item.downstream)) focus.push(item);
    else if (item.upstream) upstream.push(item);
    else downstream.push(item);
  }
  const precision = codeGraphPrecisionCounts(graph), relations = codeGraphRelationSummary(graph);
  els.codeGraphSummary.innerHTML = `<div class="code-graph-summary-main">
    <strong>${graph.nodes.length} ${esc(localized("nodes", "个节点"))} · ${graph.edges.length} ${esc(localized("edges", "条边"))}</strong>
    <span class="code-graph-chip warn">${graph.upstream_nodes || 0} ${esc(localized("upstream", "上游"))}</span>
    <span class="code-graph-chip good">${graph.downstream_nodes || 0} ${esc(localized("downstream", "下游"))}</span>
    ${graph.truncated ? `<span class="code-graph-chip warn">${esc(t("truncated"))}</span>` : ""}
  </div><div class="pills">${precision.map(([name, count]) => `<span class="code-graph-chip">${esc(name)} ${count}</span>`).join("")}${relations.map(([name, count]) => `<span class="code-graph-chip accent">${esc(statusLabel(name))} ${count}</span>`).join("")}</div>`;
  const nodeById = new Map(graph.nodes.map(item => [item.node.id, item.node]));
  const edges = (graph.edges || []).slice(0, 80).map(edge => codeGraphEdgeRow(edge, nodeById)).join("");
  els.codeGraphMap.innerHTML = `<div class="code-graph-lanes">
      ${codeGraphLane(localized("Upstream / blast radius", "上游 / 影响半径"), upstream, roots)}
      ${codeGraphLane(localized("Focus / context", "中心 / 上下文"), focus, roots)}
      ${codeGraphLane(localized("Downstream / dependencies", "下游 / 依赖"), downstream, roots)}
    </div>
    <div class="code-graph-context">
      <div class="code-graph-context-card"><strong>${esc(localized("Graph contract", "图谱契约"))}</strong><span>${esc(localized("Every relation keeps provider and precision. Syntax, semantic and runtime evidence are never flattened into one confidence claim.", "每条关系保留 provider 与 precision；syntax、semantic、runtime 证据不会被混成一个置信结论。"))}</span></div>
      <div class="code-graph-context-card"><strong>${esc(localized("Bounded exploration", "有界探索"))}</strong><span>${esc(localized(`Depth ${graph.depth} · mode ${graph.mode} · snapshot ${graph.snapshot_id}`, `深度 ${graph.depth} · 模式 ${graph.mode} · 快照 ${graph.snapshot_id}`))}</span></div>
    </div>
    <div class="code-graph-edge-list">${edges}</div>`;
  els.codeGraphMap.querySelectorAll("[data-code-node]").forEach(button => {
    button.addEventListener("click", () => {
      state.selectedCodeNode = button.dataset.codeNode;
      renderCodeGraph();
      renderCodeGraphInspector();
    });
    button.addEventListener("dblclick", () => {
      void loadCodeGraph({ nodeId: button.dataset.codeNode });
    });
  });
  renderCodeGraphInspector();
}
async function loadCodeGraph({ nodeId } = {}) {
  const query = (els.codeGraphSearch?.value || state.codeGraphQuery || "").trim();
  if (!nodeId && query.length < 2) {
    state.codeGraph = null; state.codeGraphError = ""; renderCodeGraph(); return false;
  }
  state.codeGraphController?.abort();
  const controller = new AbortController();
  state.codeGraphController = controller;
  state.codeGraphLoading = true; state.codeGraphError = ""; renderCodeGraph();
  const params = new URLSearchParams();
  if (nodeId) params.set("node_id", nodeId); else params.set("q", query);
  params.set("mode", state.codeGraphMode || "all");
  if (state.codeGraphSnapshot) params.set("snapshot_id", state.codeGraphSnapshot);
  params.set("depth", String(state.codeGraphDepth || 2));
  params.set("limit", "140");
  try {
    const data = await uiJson(`/intelligence/code-graph?${params}`, "GET", undefined, {
      workspace: state.current,
      signal: controller.signal,
      timeout: 30000,
    });
    if (controller.signal.aborted || data.workspace !== state.current) return false;
    state.codeGraph = data.graph; state.codeGraphWorkspace = data.workspace;
    state.codeGraphQuery = nodeId ? data.graph?.query || query : query;
    state.selectedCodeNode = data.graph?.root_ids?.[0] || "";
    renderCodeGraph();
    return true;
  } catch (error) {
    if (!controller.signal.aborted) {
      state.codeGraph = null;
      state.codeGraphError = requestFailureMessage(error);
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
function maybeLoadCodeGraph() {
  if (state.workspaceTab !== "architecture" || state.architectureView !== "codegraph" || state.codeGraphLoading) return;
  if (state.codeGraph && state.codeGraphWorkspace === state.current) return;
  if (els.codeGraphSearch && !els.codeGraphSearch.value.trim()) {
    const changed = state.project?.changes?.find(change => change.path)?.path || "";
    if (changed) els.codeGraphSearch.value = changed;
  }
  if ((els.codeGraphSearch?.value || "").trim().length >= 2) void loadCodeGraph();
}
function wireCodeGraph() {
  if (!els.codeGraphSearch) return;
  els.codeGraphSearch.addEventListener("keydown", event => {
    if (event.key === "Enter") { event.preventDefault(); void loadCodeGraph(); }
  });
  els.codeGraphSearch.addEventListener("search", () => void loadCodeGraph());
  document.querySelectorAll("[data-code-graph-mode]").forEach(button => {
    button.addEventListener("click", () => {
      state.codeGraphMode = button.dataset.codeGraphMode;
      document.querySelectorAll("[data-code-graph-mode]").forEach(item => item.classList.toggle("active", item === button));
      if ((els.codeGraphSearch.value || "").trim() || state.selectedCodeNode) void loadCodeGraph();
    });
  });
  els.codeGraphSnapshot?.addEventListener("change", () => {
    state.codeGraphSnapshot = els.codeGraphSnapshot.value || "";
    state.selectedCodeNode = "";
    if ((els.codeGraphSearch.value || "").trim()) void loadCodeGraph();
  });
  els.codeGraphDepth?.addEventListener("change", () => {
    state.codeGraphDepth = Number(els.codeGraphDepth.value) || 2;
    if ((els.codeGraphSearch.value || "").trim() || state.selectedCodeNode) void loadCodeGraph();
  });
}
