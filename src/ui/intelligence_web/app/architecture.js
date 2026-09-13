function architectureData() {
  const defaults = {
    subsystems: [],
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
  return { ...defaults, ...(state.project?.architecture || {}) };
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
function architectureNodeTone(component, dependencies) {
  const incident = dependencies.filter((edge) =>
    edge.from === component.id || edge.to === component.id
  );
  if (incident.some((edge) => edge.blocking)) return "drift";
  if (component.changed) return "changed";
  if (!incident.some(edge => edge.actual) || incident.some(edge => edge.status !== "aligned")) return "uncertain";
  return "aligned";
}
function renderComponentInspector() {
  const a = architectureData(),
    components = a.components || [],
    component = components.find((item) => item.id === state.selectedComponent);
  if (!component) {
    els.componentInspector.classList.remove("open");
    setHtml("componentInspector", els.componentInspector, "");
    return;
  }
  els.componentInspector.classList.add("open");
  const deps = (a.dependencies || []).filter(edge => edge.from === component.id || edge.to === component.id),
    outgoing = deps.filter(edge => edge.from === component.id),
    incoming = deps.filter(edge => edge.to === component.id),
    designed = outgoing.filter(edge => edge.desired),
    observed = outgoing.filter(edge => edge.actual),
    designOnly = outgoing.filter(edge => edge.desired && !edge.actual),
    actualOnly = outgoing.filter(edge => edge.actual && !edge.desired),
    blockingEdges = deps.filter(edge => edge.blocking),
    uncertain = deps.filter(edge => !edge.blocking && edge.status !== "aligned"),
    subsystem = (a.subsystems || []).find(item => item.id === component.subsystem),
    linkedRequirements = (state.project?.requirements || []).filter(requirement =>
      (requirement.components || []).some(item => item.id === component.id)
    ),
    acceptance = linkedRequirements.flatMap(requirement => requirement.acceptance || []),
    verificationRefs = acceptance.flatMap(item => item.verification || []),
    resolvedVerificationRefs = verificationRefs.filter(item => item.resolved).length,
    stableRequirements = linkedRequirements.filter(item => item.convergence === "stable").length,
    convergenceNeeds = linkedRequirements.filter(item => ["needs_convergence", "incomplete"].includes(item.convergence)).length,
    blockers = [...new Set(linkedRequirements.flatMap(item => [
      ...(item.convergence_blockers || []),
      ...(item.drift || []),
    ]))].slice(0, 8),
    blocking = blockingEdges.length,
    status = blocking
      ? t("Architecture drift")
      : uncertain.length
      ? t("Needs stronger evidence")
      : deps.some(edge => edge.actual) ? t("Architecture aligned") : t("Needs stronger evidence"),
    tone = blocking ? "bad" : uncertain.length || !deps.some(edge => edge.actual) ? "info" : "good";
  const depNames = (edges, actual = false) => edges.length
    ? `<div class="inspector-list">${edges.map(edge => `<div class="inspector-item"><strong>${esc(edge.to_name)}</strong><small>${esc(actual ? `${statusLabel(edge.precision)} · ${statusLabel(edge.status)}` : statusLabel(edge.status))}</small></div>`).join("")}</div>`
    : `<div class="empty">${esc(localized("None", "无"))}</div>`;
  const deltaItems = [
    ...designOnly.map(edge => localized(`Declared, not observed: ${edge.to_name}`, `设计已声明、尚未观测：${edge.to_name}`)),
    ...actualOnly.map(edge => localized(`Observed, not declared: ${edge.to_name}`, `实际已观测、设计未声明：${edge.to_name}`)),
    ...blockingEdges.map(edge => localized(`Confirmed drift: ${edge.from_name} → ${edge.to_name}`, `已确认偏离：${edge.from_name} → ${edge.to_name}`)),
  ];
  const comparison = `<div class="inspector-compare"><section><h5>${esc(localized("DESIGN", "设计"))}</h5>${depNames(designed)}</section><section><h5>${esc(localized("ACTUAL", "实际"))}</h5>${depNames(observed, true)}</section><section class="${blocking ? "bad" : deltaItems.length ? "warn" : ""}"><h5>${esc(localized("DELTA", "差异"))}</h5>${deltaItems.length ? `<div class="inspector-list">${deltaItems.map(item => `<div class="inspector-item">${esc(item)}</div>`).join("")}</div>` : `<div class="empty">${esc(localized("No dependency delta is currently confirmed.", "当前未确认依赖差异。"))}</div>`}</section></div>`;
  const dependencyHtml = deps.length
    ? deps.map(edge => {
      const isOutgoing = edge.from === component.id,
        other = isOutgoing ? edge.to_name : edge.from_name;
      return `<div class="inspector-item inspector-dep"><div><b>${esc(isOutgoing ? t("outgoing") : t("incoming"))}</b> · ${esc(other)}<div class="panel-meta">${esc(statusLabel(edge.status))} · ${esc(statusLabel(edge.precision))}</div></div>${pill(edge.desired && edge.actual ? localized("design + actual", "设计 + 实际") : edge.desired ? t("design edge") : t("actual edge"), architectureEdgeTone(edge))}</div>`;
    }).join("")
    : `<div class="empty">${esc(t("No dependency edges."))}</div>`;
  const changedPaths = new Set(component.changed_paths || []);
  const implementations = (component.implementation_targets || []).length
    ? `<div class="inspector-list">${component.implementation_targets.map(target => {
      const path = target.split("::")[0], changed = changedPaths.has(path);
      return `<div class="inspector-item inspector-implementation"><code>${esc(target)}</code>${changed ? pill(statusLabel("changed"), "warn") : ""}</div>`;
    }).join("")}</div>`
    : `<div class="empty">${esc(t("No implementation mapping."))}</div>`;
  const requirements = linkedRequirements.length
    ? `<div class="inspector-requirements">${linkedRequirements.map(requirement => {
      const reqTone = requirement.convergence === "stable" ? "good" : requirement.convergence === "changing" ? "warn" : "bad";
      return `<button class="inspector-requirement" type="button" data-inspector-req="${esc(requirement.id)}"><span><strong>${esc(requirement.id)}</strong><small>${esc(requirement.title || requirement.intent || "")}</small></span>${pill(statusLabel(requirement.convergence), reqTone)}</button>`;
    }).join("")}</div>`
    : `<div class="empty">${esc(t("No related requirements."))}</div>`;
  const changes = changedPaths.size
    ? `<div class="inspector-list">${[...changedPaths].map(path => `<div class="inspector-item"><code>${esc(path)}</code></div>`).join("")}</div>`
    : `<div class="empty">${esc(t("No current component changes."))}</div>`;
  const scopes = (component.product_scopes || []).length
    ? `<div class="pills">${component.product_scopes.map(scope => pill(scope, "accent")).join("")}</div>`
    : `<div class="empty">${esc(t("No product scope mapping."))}</div>`;
  const responsibilities = (component.responsibilities || []).length
    ? `<ul class="responsibilities">${component.responsibilities.map(item => `<li>${esc(item)}</li>`).join("")}</ul>`
    : `<div class="empty">${esc(t("No responsibilities declared."))}</div>`;
  const convergence = blockers.length
    ? `<div class="inspector-list">${blockers.map(item => `<div class="inspector-item">${esc(item)}</div>`).join("")}</div>`
    : `<div class="empty">${esc(localized("No requirement convergence blocker is recorded for this component.", "该组件当前没有记录的需求收敛阻塞项。"))}</div>`;
  const health = `<div class="inspector-health"><div><strong>${num(blocking)}</strong><span>${esc(localized("confirmed drift", "已确认偏离"))}</span></div><div><strong>${num(stableRequirements)} / ${num(linkedRequirements.length)}</strong><span>${esc(localized("stable requirements", "稳定需求"))}</span></div><div><strong>${num(resolvedVerificationRefs)} / ${num(verificationRefs.length)}</strong><span>${esc(localized("verification refs mapped", "验证引用已映射"))}</span></div><div><strong>${num(changedPaths.size)}</strong><span>${esc(localized("changed paths", "变更路径"))}</span></div></div>`;
  const verification = `<div class="inspector-proof-note"><strong>${esc(localized("Verification mapping, not execution proof", "验证映射，不代表已执行证明"))}</strong><span>${esc(localized(`${acceptance.length} acceptance criteria · ${resolvedVerificationRefs}/${verificationRefs.length} verification references resolve. Current execution evidence is revision-bound and shown in Verification evidence.`, `${acceptance.length} 个验收条件 · ${resolvedVerificationRefs}/${verificationRefs.length} 个验证引用已解析。当前执行证据绑定版本，统一在“验证证据”中查看。`))}</span><button type="button" class="inspector-link" data-inspector-proof>${esc(localized("Open verification evidence", "打开验证证据"))}</button></div>`;
  const inspectorSection = (title, body, open = false) => `<details class="inspector-section inspector-disclosure"${open ? " open" : ""}><summary><span>${esc(title)}</span><span class="disclosure-mark" aria-hidden="true"></span></summary><div class="inspector-section-body">${body}</div></details>`;
  const html = `<button type="button" class="inspector-close" data-inspector-close aria-label="${esc(localized("Close inspector", "关闭检查器"))}">${uiIcon("close")}</button><div class="req-top inspector-title"><div><h3>${esc(component.name)}</h3><div class="component-id">${esc(component.id)}</div><div class="component-subsystem">${esc(localized("Subsystem", "子系统"))}: ${esc(subsystem?.title || component.subsystem || localized("Unscoped", "未归类"))}</div></div>${pill(status, tone)}</div><div class="pills">${pill(unit(component.implementation_files, "file", "files", "个文件"))}${pill(unit(component.implementation_lines, "line", "lines", "行"))}${component.changed ? pill(statusLabel("changed"), "warn") : pill(statusLabel("stable"), "good")}${convergenceNeeds ? pill(localized(`${convergenceNeeds} need convergence`, `${convergenceNeeds} 项需收敛`), "warn") : ""}</div>${health}${inspectorSection(localized("Design vs actual", "设计 vs 实际"), comparison, true)}${inspectorSection(t("Responsibilities"), responsibilities)}${inspectorSection(t("Implementation mapping"), implementations)}${inspectorSection(localized("Upstream / downstream dependencies", "上下游依赖"), dependencyHtml)}${inspectorSection(t("Related requirements"), requirements)}${inspectorSection(localized("Verification & proof", "验证与证明"), verification)}${inspectorSection(localized("Convergence & drift", "收敛与偏离"), convergence)}${inspectorSection(t("Changed paths"), changes)}${inspectorSection(t("Product scopes"), scopes)}`;
  setHtml("componentInspector", els.componentInspector, html, () => {
    els.componentInspector.querySelector("[data-inspector-close]")?.addEventListener("click", () => {
      state.selectedComponent = "";
      els.componentInspector.classList.remove("open");
    });
    els.componentInspector.querySelectorAll("[data-inspector-req]").forEach(button => button.addEventListener("click", () => {
      state.filter = "all"; els.search.value = "";
      document.querySelectorAll(".filter").forEach(item => {
        item.classList.toggle("active", item.dataset.filter === "all");
        item.setAttribute("aria-pressed", String(item.dataset.filter === "all"));
      });
      state.selected = button.dataset.inspectorReq;
      revealSection("requirementsSection");
      invalidate("requirements", "detail");
      renderRequirements();
      renderDetail();
      els.detail.scrollIntoView({ behavior: "smooth", block: "start" });
    }));
    els.componentInspector.querySelector("[data-inspector-proof]")?.addEventListener("click", () => revealSection("proofSection"));
  });
}
function renderComponentCards() {
  const a = architectureData(), query = els.componentSearch.value.trim().toLowerCase();
  const components = (a.components || []).filter(item => [item.name, item.id, ...(item.responsibilities || []), ...(item.product_scopes || [])].join(" ").toLowerCase().includes(query));
  if (!components.some(item => item.id === state.selectedComponent)) {
    const priority = components.find(item => architectureNodeTone(item, a.dependencies || []) === "drift") || components.find(item => item.changed) || components[0];
    state.selectedComponent = priority?.id || "";
  }
  els.componentCount.textContent = localized(`${components.length} / ${(a.components || []).length} components`, `${components.length} / ${(a.components || []).length} 个组件`);
  const groups = new Map();
  for (const component of components) {
    const scope = (component.product_scopes || [])[0] || localized("Unscoped", "未归类");
    if (!groups.has(scope)) groups.set(scope, []);
    groups.get(scope).push(component);
  }
  const html = [...groups].map(([scope, items]) => `<section class="component-group"><h3>${esc(scope)} <span>${items.length}</span></h3><div class="component-grid">${items.map(component => {
    const tone = architectureNodeTone(component, a.dependencies || []);
    const label = tone === "drift" ? localized("Strong drift", "强证据偏离") : tone === "changed" ? localized("Changed", "有变更") : tone === "uncertain" ? localized("Needs evidence", "待补证据") : localized("Dependencies observed", "依赖已观测");
    return `<button type="button" class="component-tile ${tone}" data-card-component="${esc(component.id)}" aria-pressed="${component.id === state.selectedComponent}"><span class="component-tile-top"><strong>${esc(component.name)}</strong><span class="pill ${tone === "drift" ? "bad" : tone === "changed" ? "warn" : "info"}">${esc(label)}</span></span><span class="component-purpose">${esc((component.responsibilities || [])[0] || localized("No responsibility declared", "未声明职责"))}</span><span class="component-tile-meta">${esc(localized(`${num(component.implementation_files)} mapped files · ${(component.depends_on || []).length} declared dependencies`, `${num(component.implementation_files)} 个映射文件 · ${(component.depends_on || []).length} 条声明依赖`))}</span></button>`;
  }).join("")}</div></section>`).join("") || `<div class="empty">${esc(localized("No matching components. Clear the search or add architecture mappings.", "没有匹配组件。请清除搜索，或补充架构映射。"))}</div>`;
  setHtml("componentCards", els.componentCards, html, () => {
    const buttons = [...els.componentCards.querySelectorAll("[data-card-component]")];
    const activate = (button, { focus = true } = {}) => {
      if (!button) return;
      state.selectedComponent = button.dataset.cardComponent;
      renderComponentCards();
      renderComponentInspector();
      requestAnimationFrame(() => {
        const selected = [...els.componentCards.querySelectorAll("[data-card-component]")].find(item => item.dataset.cardComponent === state.selectedComponent);
        selected?.scrollIntoView({ behavior: "smooth", block: "nearest" });
        if (focus) selected?.focus({ preventScroll: true });
        if (window.matchMedia("(max-width: 900px)").matches) els.componentInspector.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    };
    buttons.forEach((button, index) => {
      button.addEventListener("click", () => activate(button));
      button.addEventListener("keydown", event => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : event.key === "ArrowDown" ? Math.min(buttons.length - 1, index + 1) : Math.max(0, index - 1);
        activate(buttons[nextIndex]);
      });
    });
  });
}
