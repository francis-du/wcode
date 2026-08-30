function architectureData() {
  return state.project?.architecture || {
    components: [],
    dependencies: [],
    desired_edges: 0,
    observed_edges: 0,
    aligned_edges: 0,
    blocking_drift_edges: 0,
    advisory_edges: 0,
    unverified_edges: 0,
    components_with_implementation: 0,
    observed_drift_percent: 0,
    evidence_coverage_percent: 0,
    implementation_coverage_percent: 0,
  };
}
function architectureEdgeTone(edge) {
  if (edge.blocking) return "drift";
  if (edge.status === "aligned") return "aligned";
  if (edge.status === "unverified_actual") return "unverified";
  return "observed";
}
function architectureEdgeVisible(edge) {
  return state.architectureMode === "overlay" ||
    (state.architectureMode === "design" && edge.desired) ||
    (state.architectureMode === "actual" && edge.actual);
}
function architectureMetric(label, value, detail, progress, tone = "") {
  const bounded = Math.max(0, Math.min(100, Number(progress || 0)));
  return `<div class="architecture-metric ${tone}"><div class="metric-label">${
    esc(label)
  }</div><div class="metric-value">${
    esc(value)
  }</div><div class="metric-detail">${
    esc(detail)
  }</div><progress class="metric-progress" max="100" value="${bounded}"></progress></div>`;
}
function renderArchitectureMetrics() {
  const a = architectureData(),
    drift = Math.round(Number(a.observed_drift_percent || 0) * 10) / 10,
    evidence = Math.round(Number(a.evidence_coverage_percent || 0) * 10) / 10,
    implementation =
      Math.round(Number(a.implementation_coverage_percent || 0) * 10) / 10;
  const html = [
    architectureMetric(
      t("Observed drift"),
      `${drift}%`,
      t("strong drift denominator"),
      100 - drift,
      drift > 0 ? "drift" : "good",
    ),
    architectureMetric(
      t("Evidence coverage"),
      `${evidence}%`,
      t("coverage denominator"),
      evidence,
      "info",
    ),
    architectureMetric(
      t("Implementation coverage"),
      `${implementation}%`,
      t("implementation denominator"),
      implementation,
      "info",
    ),
    architectureMetric(
      t("Architecture size"),
      unit(a.components.length, "component", "components", "个组件"),
      localized(
        `${a.desired_edges} design / ${a.observed_edges} observed dependencies`,
        `${a.desired_edges} 条设计依赖 / ${a.observed_edges} 条实际依赖`,
      ),
      100,
      "",
    ),
  ].join("");
  setHtml("architectureMetrics", els.architectureMetrics, html);
}
function architecturePositions(components) {
  const ids = new Set(components.map((component) => component.id)),
    dependencies = new Map(
      components.map(
        (component) => [
          component.id,
          (component.depends_on || []).filter((id) => ids.has(id)),
        ],
      ),
    ),
    memo = new Map();
  const depth = (id, stack = new Set()) => {
    if (memo.has(id)) return memo.get(id);
    if (stack.has(id)) return 0;
    const next = new Set(stack);
    next.add(id);
    let value = 0;
    for (const dependency of dependencies.get(id) || []) {
      value = Math.max(value, 1 + depth(dependency, next));
    }
    value = Math.min(value, 7);
    memo.set(id, value);
    return value;
  };
  const grouped = new Map();
  for (const component of components) {
    const d = depth(component.id);
    if (!grouped.has(d)) grouped.set(d, []);
    grouped.get(d).push(component);
  }
  const columns = [];
  for (const d of [...grouped.keys()].sort((a, b) => b - a)) {
    const group = grouped.get(d).sort((a, b) => a.name.localeCompare(b.name));
    for (let i = 0; i < group.length; i += 7) {
      columns.push(group.slice(i, i + 7));
    }
  }
  const positions = new Map(),
    nodeWidth = 190,
    nodeHeight = 58,
    columnGap = 235,
    rowGap = 88;
  columns.forEach((column, columnIndex) =>
    column.forEach((component, rowIndex) =>
      positions.set(component.id, {
        x: 55 + columnIndex * columnGap + nodeWidth / 2,
        y: 58 + rowIndex * rowGap + nodeHeight / 2,
        w: nodeWidth,
        h: nodeHeight,
      })
    )
  );
  return {
    positions,
    width: Math.max(760, 110 + Math.max(columns.length, 1) * columnGap),
    height: Math.max(
      410,
      100 + Math.max(...columns.map((column) => column.length), 1) * rowGap,
    ),
  };
}
function architectureNodeTone(component, dependencies) {
  const incident = dependencies.filter((edge) =>
    edge.from === component.id || edge.to === component.id
  );
  if (incident.some((edge) => edge.blocking)) return "drift";
  if (component.changed) return "changed";
  if (incident.some((edge) => edge.status !== "aligned")) return "uncertain";
  return "aligned";
}
function renderArchitectureGraph() {
  const a = architectureData(),
    components = a.components || [],
    allEdges = a.dependencies || [];
  if (!components.length) {
    setHtml(
      "architectureGraph",
      els.architectureGraph,
      `<div class="section empty">${
        esc(
          localized(
            "No architecture components are declared.",
            "没有声明架构组件。",
          ),
        )
      }</div>`,
    );
    return;
  }
  if (
    !state.selectedComponent ||
    !components.some((component) => component.id === state.selectedComponent)
  ) {
    const driftEdge = allEdges.find((edge) => edge.blocking),
      preferred = components.find((component) =>
        driftEdge &&
        (component.id === driftEdge.from || component.id === driftEdge.to)
      ) || components.find((component) => component.changed) || components[0];
    state.selectedComponent = preferred.id;
  }
  const edges = allEdges.filter(architectureEdgeVisible),
    layout = architecturePositions(components),
    positions = layout.positions;
  const edgeSvg = edges.map((edge) => {
    const from = positions.get(edge.from), to = positions.get(edge.to);
    if (!from || !to) return "";
    const direction = to.x >= from.x ? 1 : -1,
      sx = from.x + direction * from.w / 2,
      tx = to.x - direction * to.w / 2,
      curve = Math.max(35, Math.abs(tx - sx) * .45),
      c1 = sx + direction * curve,
      c2 = tx - direction * curve,
      tone = architectureEdgeTone(edge),
      label = edge.blocking
        ? localized("drift", "偏离")
        : edge.status === "unverified_actual"
        ? "?"
        : "";
    return `<g><path class="arch-edge ${tone}" d="M ${sx} ${from.y} C ${c1} ${from.y}, ${c2} ${to.y}, ${tx} ${to.y}" marker-end="url(#arrow-${tone})"><title>${
      esc(
        `${edge.from_name} → ${edge.to_name} · ${statusLabel(edge.status)} · ${
          statusLabel(edge.precision)
        }`,
      )
    }</title></path>${
      label
        ? `<text class="arch-edge-label" x="${(sx + tx) / 2}" y="${
          (from.y + to.y) / 2 - 4
        }">${esc(label)}</text>`
        : ""
    }</g>`;
  }).join("");
  const nodeSvg = components.map((component) => {
    const pos = positions.get(component.id),
      tone = architectureNodeTone(component, allEdges),
      selected = component.id === state.selectedComponent ? " selected" : "",
      name = component.name.length > 24
        ? `${component.name.slice(0, 22)}…`
        : component.name,
      meta = localized(
        `${component.implementation_files} files · ${component.requirements.length} req`,
        `${component.implementation_files} 个文件 · ${component.requirements.length} 个需求`,
      );
    return `<g class="arch-node ${tone}${selected}" data-component="${
      esc(component.id)
    }" transform="translate(${pos.x - pos.w / 2} ${
      pos.y - pos.h / 2
    })"><rect width="${pos.w}" height="${pos.h}" rx="9"></rect><text class="arch-node-title" x="10" y="20">${
      esc(name)
    }</text><text class="arch-node-id" x="10" y="34">${
      esc(component.id)
    }</text><text class="arch-node-meta" x="10" y="49">${
      esc(meta)
    }</text><title>${esc(component.name)}</title></g>`;
  }).join("");
  const html =
    `<svg class="architecture-svg" viewBox="0 0 ${layout.width} ${layout.height}" aria-label="${
      esc(localized("Architecture drift graph", "架构偏离图"))
    }"><defs><marker id="arrow-aligned" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path class="arch-marker aligned" d="M0,0 L7,3.5 L0,7 z"></path></marker><marker id="arrow-unverified" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path class="arch-marker unverified" d="M0,0 L7,3.5 L0,7 z"></path></marker><marker id="arrow-observed" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path class="arch-marker observed" d="M0,0 L7,3.5 L0,7 z"></path></marker><marker id="arrow-drift" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path class="arch-marker drift" d="M0,0 L7,3.5 L0,7 z"></path></marker></defs>${edgeSvg}${nodeSvg}</svg>`;
  setHtml(
    "architectureGraph",
    els.architectureGraph,
    html,
    () =>
      els.architectureGraph.querySelectorAll("[data-component]").forEach(
        (node) =>
          node.addEventListener("click", () => {
            state.selectedComponent = node.dataset.component;
            invalidate("architectureGraph", "componentInspector");
            renderArchitectureGraph();
            renderComponentInspector();
          }),
      ),
  );
}
function renderComponentInspector() {
  const a = architectureData(),
    components = a.components || [],
    component = components.find((item) => item.id === state.selectedComponent);
  if (!component) {
    setHtml(
      "componentInspector",
      els.componentInspector,
      `<div class="empty">${esc(t("No component selected."))}</div>`,
    );
    return;
  }
  const deps = (a.dependencies || []).filter((edge) =>
      edge.from === component.id || edge.to === component.id
    ),
    blocking = deps.filter((edge) => edge.blocking).length,
    uncertain =
      deps.filter((edge) => !edge.blocking && edge.status !== "aligned").length,
    status = blocking
      ? t("Architecture drift")
      : uncertain
      ? t("Needs stronger evidence")
      : t("Architecture aligned"),
    tone = blocking ? "bad" : uncertain ? "info" : "good";
  const dependencyHtml = deps.length
    ? deps.map((edge) => {
      const outgoing = edge.from === component.id,
        other = outgoing ? edge.to_name : edge.from_name;
      return `<div class="inspector-item inspector-dep"><div><b>${
        esc(outgoing ? t("outgoing") : t("incoming"))
      }</b> · ${esc(other)}<div class="panel-meta">${
        esc(statusLabel(edge.status))
      } · ${esc(statusLabel(edge.precision))}</div></div>${
        pill(
          edge.desired && edge.actual
            ? localized("design + actual", "设计 + 实际")
            : edge.desired
            ? t("design edge")
            : t("actual edge"),
          architectureEdgeTone(edge),
        )
      }</div>`;
    }).join("")
    : `<div class="empty">${esc(t("No dependency edges."))}</div>`;
  const implementations = (component.implementation_targets || []).length
    ? `<div class="inspector-list">${
      component.implementation_targets.map((target) =>
        `<div class="inspector-item"><code>${esc(target)}</code></div>`
      ).join("")
    }</div>`
    : `<div class="empty">${esc(t("No implementation mapping."))}</div>`;
  const requirements = (component.requirements || []).length
    ? `<div class="pills">${
      component.requirements.map((id) =>
        `<button class="inspector-link" type="button" data-inspector-req="${
          esc(id)
        }">${esc(id)}</button>`
      ).join("")
    }</div>`
    : `<div class="empty">${esc(t("No related requirements."))}</div>`;
  const changes = (component.changed_paths || []).length
    ? `<div class="inspector-list">${
      component.changed_paths.map((path) =>
        `<div class="inspector-item"><code>${esc(path)}</code></div>`
      ).join("")
    }</div>`
    : `<div class="empty">${esc(t("No current component changes."))}</div>`;
  const scopes = (component.product_scopes || []).length
    ? `<div class="pills">${
      component.product_scopes.map((scope) => pill(scope, "accent")).join("")
    }</div>`
    : `<div class="empty">${esc(t("No product scope mapping."))}</div>`;
  const responsibilities = (component.responsibilities || []).length
    ? `<ul class="responsibilities">${
      component.responsibilities.map((item) => `<li>${esc(item)}</li>`).join("")
    }</ul>`
    : `<div class="empty">${esc(t("No responsibilities declared."))}</div>`;
  const html = `<div class="req-top"><div><h3>${
    esc(component.name)
  }</h3><div class="component-id">${esc(component.id)}</div></div>${
    pill(status, tone)
  }</div><div class="pills">${
    pill(unit(component.implementation_files, "file", "files", "个文件"))
  }${pill(unit(component.implementation_lines, "line", "lines", "行"))}${
    component.changed
      ? pill(statusLabel("changed"), "warn")
      : pill(statusLabel("stable"), "good")
  }</div><div class="inspector-section"><h4>${
    esc(t("Responsibilities"))
  }</h4>${responsibilities}</div><div class="inspector-section"><h4>${
    esc(t("Implementation mapping"))
  }</h4>${implementations}</div><div class="inspector-section"><h4>${
    esc(t("Dependencies"))
  }</h4>${dependencyHtml}</div><div class="inspector-section"><h4>${
    esc(t("Related requirements"))
  }</h4>${requirements}</div><div class="inspector-section"><h4>${
    esc(t("Changed paths"))
  }</h4>${changes}</div><div class="inspector-section"><h4>${
    esc(t("Product scopes"))
  }</h4>${scopes}</div>`;
  setHtml(
    "componentInspector",
    els.componentInspector,
    html,
    () =>
      els.componentInspector.querySelectorAll("[data-inspector-req]").forEach(
        (button) =>
          button.addEventListener("click", () => {
            state.selected = button.dataset.inspectorReq;
            invalidate("requirements", "detail");
            renderRequirements();
            renderDetail();
            els.detail.scrollIntoView({ behavior: "smooth", block: "start" });
          }),
      ),
  );
}
function renderArchitecture() {
  renderArchitectureMetrics();
  renderArchitectureGraph();
  renderComponentInspector();
}
