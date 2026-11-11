function subsystemComponents(subsystem) {
  const components = architectureData().components || [], ids = new Set(subsystem.component_ids || []);
  return components.filter(component => ids.has(component.id));
}
function projectNavigatorItems() {
  const project = state.project || {}, architecture = project.architecture || {}, items = [];
  for (const subsystem of architecture.subsystems || []) items.push({ kind: "subsystem", id: subsystem.id, label: subsystem.title, detail: subsystem.purpose });
  for (const component of architecture.components || []) items.push({ kind: "component", id: component.id, label: component.name, detail: (component.responsibilities || [])[0] || component.id });
  for (const requirement of project.requirements || []) items.push({ kind: "requirement", id: requirement.id, label: requirement.title || requirement.id, detail: requirement.intent || requirement.id });
  for (const change of project.changes || []) items.push({ kind: "change", id: change.path, label: change.path, detail: localized(`${change.status || "changed"} · ${(change.affected_components || []).length} components`, `${statusLabel(change.status || "changed")} · ${(change.affected_components || []).length} 个组件`) });
  for (const file of project.structure?.entries || []) items.push({ kind: "file", id: file.path, label: file.path, detail: localized(`${file.language || "source"} · ${num(file.lines || 0)} lines`, `${file.language || "源码"} · ${num(file.lines || 0)} 行`) });
  return items;
}
// The left object navigator keeps the loaded objects reachable without the
// search palette, and reuses the header navigator's item source so both
// surfaces describe the same snapshot.
// The right inspector keeps the selected object's identity, state and scope
// readable without leaving the current view. It is a summary of the same
// snapshot the views own; it does not duplicate their full detail.
function changeStatus(path) {
  if (typeof path !== "string" || !path) return "";
  return (state.project?.changes || []).find(item => item.path === path)?.status || "";
}
function objectInspectorFact(label, value) {
  return `<div class="inspector-fact"><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`;
}
function renderObjectInspector() {
  const host = els.objectInspector;
  if (!host) return;
  const project = state.project;
  const requirement = (project?.requirements || []).find(item => item.id === state.selected);
  const component = (architectureData().components || []).find(item => item.id === state.selectedComponent);
  let body = "";
  if (requirement) {
    const drift = (requirement.drift || []).slice(0, 3).join(", ");
    const changedPaths = requirementChanges(requirement).map(item => item.path).filter(Boolean);
    const changedFiles = changedPaths.length
      ? changedPaths.slice(0, 4).join(", ") + (changedPaths.length > 4 ? ` +${changedPaths.length - 4}` : "")
      : localized("none", "无");
    body = `<div class="inspector-kind">${esc(localized("Requirement", "需求"))}</div><strong class="inspector-title">${esc(requirement.title || requirement.id)}</strong><small class="inspector-id">${esc(requirement.id)}</small><div class="inspector-facts">${
      objectInspectorFact(localized("Priority", "优先级"), statusLabel(requirement.priority))
      + objectInspectorFact(localized("Convergence", "收敛状态"), statusLabel(requirement.convergence))
      + objectInspectorFact(localized("Components", "组件"), num((requirement.components || []).length))
      + objectInspectorFact(localized("Acceptance", "验收标准"), num((requirement.acceptance || []).length))
      + objectInspectorFact(localized("Drift paths", "偏离路径"), drift || "—")
      + objectInspectorFact(localized("Changed files", "变更文件"), changedFiles)
    }</div>`;
  } else if (component) {
    const alignment = (architectureData().dependencies || [])
      .filter(edge => edge.from === component.id || edge.to === component.id)
      .map(edge => statusLabel(edge.status)).join(", ");
    body = `<div class="inspector-kind">${esc(localized("Component", "组件"))}</div><strong class="inspector-title">${esc(component.name || component.id)}</strong><small class="inspector-id">${esc(component.id)}</small><div class="inspector-facts">${
      objectInspectorFact(localized("Responsibilities", "职责"), num((component.responsibilities || []).length))
      + objectInspectorFact(localized("Mapped files", "映射文件"), num(component.implementation_files || 0))
      + objectInspectorFact(localized("Dependencies", "依赖状态"), alignment || "—")
      + objectInspectorFact(localized("Changed", "已变更"), component.changed ? localized("yes", "是") : localized("no", "否"))
    }</div>`;
  } else if (state.codeReader?.path) {
    body = `<div class="inspector-kind">${esc(localized("Source", "源码"))}</div><strong class="inspector-title">${esc(state.codeReader.path)}</strong><div class="inspector-facts">${
      objectInspectorFact(localized("Layer", "层级"), statusLabel(state.codeReader.layer))
      + objectInspectorFact(localized("Revision", "版本"), String(state.codeReader.repositoryRevision?.code || "—").slice(0, 19))
      + objectInspectorFact(localized("Snapshot", "快照"), String(state.codeReader.snapshotId || "—").slice(0, 16))
    }</div>`;
  } else if (state.changeInspection?.path) {
    // The change's own size is the scope the operator needs before opening the
    // full diff. Counts come from the retained unified diff, so a binary or
    // not-yet-read change keeps them explicitly unknown instead of zero.
    const diff = state.changeInspection.data;
    let hunks = "—", added = "—", removed = "—";
    if (diff && typeof diff.content === "string" && diff.content.length) {
      let hunkHeaders = 0, addedLines = 0, removedLines = 0, inHunk = false;
      for (const line of diff.content.split("\n")) {
        if (line.startsWith("@@")) { hunkHeaders++; inHunk = true; continue; }
        if (!inHunk || line.startsWith("+++") || line.startsWith("---")) continue;
        if (line.startsWith("+")) addedLines++;
        else if (line.startsWith("-")) removedLines++;
      }
      hunks = num(hunkHeaders); added = `+${num(addedLines)}`; removed = `-${num(removedLines)}`;
    }
    body = `<div class="inspector-kind">${esc(localized("Change", "变更"))}</div><strong class="inspector-title">${esc(state.changeInspection.path)}</strong><div class="inspector-facts">${
      objectInspectorFact(localized("Layer", "层级"), statusLabel(state.changeInspection.layer))
      + objectInspectorFact(localized("Status", "状态"), statusLabel(changeStatus(state.changeInspection.path)))
      + objectInspectorFact(localized("Hunks", "变更块"), hunks)
      + objectInspectorFact(localized("Added", "新增行"), added)
      + objectInspectorFact(localized("Removed", "删除行"), removed)
    }</div>`;
  } else if (state.selectedCodeNode && state.codeGraph) {
    // A code graph selection is an investigation object too, so the inspector
    // names it instead of leaving the previous object on screen.
    const entry = (state.codeGraph.nodes || []).find(item => item?.node?.id === state.selectedCodeNode);
    const node = entry?.node;
    if (node) {
      const direction = entry?.upstream && entry?.downstream
        ? localized("upstream + downstream", "上游 + 下游")
        : entry?.upstream
          ? localized("upstream / blast radius", "上游 / 影响半径")
          : localized("downstream / dependency", "下游 / 依赖");
      const relations = (state.codeGraph.edges || [])
        .filter(edge => edge.from === node.id || edge.to === node.id).length;
      body = `<div class="inspector-kind">${esc(localized("Symbol", "符号"))}</div><strong class="inspector-title">${esc(node.label || node.id)}</strong><small class="inspector-id">${esc(node.id)}</small><div class="inspector-facts">${
        objectInspectorFact(localized("Kind", "类型"), statusLabel(codeGraphDisplayKind(node)))
        + objectInspectorFact(localized("Path", "路径"), codeGraphPath(node) || "—")
        + objectInspectorFact(localized("Provider", "提供者"), node.provenance?.provider || "—")
        + objectInspectorFact(localized("Precision", "精度"), String(node.provenance?.precision || "unknown"))
        + objectInspectorFact(localized("Direction", "方向"), direction)
        + objectInspectorFact(localized("Relations", "关系数"), num(relations))
      }</div>`;
    }
  }
  if (!body) {
    setHtml("objectInspector", host, `<p class="inspector-hint">${esc(localized("Select an object to inspect its identity, state and scope.", "选择对象以查看其身份、状态与范围。"))}</p>`);
    return;
  }
  setHtml("objectInspector", host, body);
}
function objectNavigatorCommand(item) {
  if (item.kind === "component") return `object:component:${item.id}`;
  if (item.kind === "requirement") return `object:requirement:${item.id}`;
  if (item.kind === "change") return `object:change:${item.id}`;
  if (item.kind === "file") return `object:file:${item.id}`;
  return "";
}
function renderObjectNavigator() {
  const host = els.objectNavigator;
  if (!host) return;
  const items = projectNavigatorItems().filter((item) => item.kind !== "subsystem");
  if (!items.length) {
    setHtml("objectNavigator", host, `<p class="object-navigator-empty">${esc(localized("No object is loaded in this snapshot.", "此快照未加载任何对象。"))}</p>`);
    return;
  }
  const shown = items.slice(0, 60);
  const rows = shown.map((item, index) =>
    `<button type="button" class="object-navigator-item" data-object-index="${index}" aria-current="${item.id === state.selected || item.id === state.selectedComponent ? "true" : "false"}"><span class="object-navigator-kind">${esc(navigatorKindLabel(item.kind))}</span><span class="object-navigator-copy"><strong>${esc(item.label)}</strong><small>${esc(item.detail || "")}</small></span></button>`
  ).join("");
  setHtml("objectNavigator", host,
    `<div class="object-navigator-head"><strong>${esc(localized("Objects", "对象"))}</strong><span>${items.length > shown.length ? `60 / ${items.length}` : items.length}</span></div><div class="object-navigator-list">${rows}</div>`,
    () => host.querySelectorAll("[data-object-index]").forEach((button) => button.addEventListener("click", () => {
      const command = objectNavigatorCommand(shown[Number(button.dataset.objectIndex)]);
      if (command) executeCommand(command);
    })));
}
function focusSelectedSubsystemCard({ focus = false } = {}) {
  const target = [...(els.architectureBlueprint?.querySelectorAll("[data-subsystem-card]") || [])]
    .find(button => button.dataset.subsystemCard === state.selectedSubsystem);
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
  if (focus) target.focus({ preventScroll: true });
}
function focusSelectedComponentCard({ focus = false } = {}) {
  const target = [...(els.componentCards?.querySelectorAll("[data-card-component]") || [])]
    .find(button => button.dataset.cardComponent === state.selectedComponent);
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "nearest" });
  if (focus) target.focus({ preventScroll: true });
}
function focusSelectedRequirement({ focus = false } = {}) {
  const target = [...(els.requirements?.querySelectorAll(".req") || [])]
    .find(button => button.dataset.id === state.selected);
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
  if (focus) target.focus({ preventScroll: true });
}
function activateProjectNavigatorItem(item) {
  if (!item) return;
  if (item.kind === "subsystem") {
    const subsystem = (architectureData().subsystems || []).find(value => value.id === item.id), first = subsystem && subsystemComponents(subsystem)[0];
    state.selectedSubsystem = item.id;
    if (first) state.selectedComponent = first.id;
    state.architectureView = "blueprint";
    renderArchitecture(); revealSection("architectureSection");
    requestAnimationFrame(() => focusSelectedSubsystemCard({ focus: true }));
  } else if (item.kind === "component") {
    state.selectedComponent = item.id; state.architectureView = "components";
    renderArchitecture(); revealSection("architectureSection");
    requestAnimationFrame(() => focusSelectedComponentCard({ focus: true }));
  } else if (item.kind === "requirement") {
    setRequirementFilter("all"); state.selected = item.id; els.search.value = "";
    invalidate("requirements", "detail"); renderRequirements(); renderDetail(); revealSection("requirementsSection");
    requestAnimationFrame(() => focusSelectedRequirement({ focus: true }));
  } else if (item.kind === "change") {
    revealSection("changesSection");
    void openChangeInspection(item.id);
    requestAnimationFrame(() => focusSelectedChangePath(item.id));
  } else if (item.kind === "file") {
    revealSection("filesSection");
    void openCodeReader(item.id);
  }
  els.projectNavigator.value = "";
  els.navigatorResults.classList.add("hidden");
  els.projectNavigator.setAttribute("aria-expanded", "false");
  els.projectNavigator.removeAttribute("aria-activedescendant");
}
function focusSelectedChangePath(path) {
  const target = [...(els.changes?.querySelectorAll("[data-change-path]") || [])]
    .find(button => button.dataset.changePath === path);
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "center", inline: "nearest" });
  target.focus({ preventScroll: true });
}
function navigatorKindLabel(kind) {
  return ({ subsystem: localized("Subsystem", "子系统"), component: localized("Component", "组件"), requirement: localized("Requirement", "需求"), change: localized("Change", "变更"), file: localized("File", "文件") })[kind] || kind;
}
// The navigator only searches the snapshot this page already loaded, so it must
// say what it covered instead of reading as a whole-repository search.
function navigatorScopeNote() {
  const project = state.project || {}, architecture = architectureData();
  const counts = [
    [`${num((architecture.subsystems || []).length)} ${localized("systems", "系统")}`, (architecture.subsystems || []).length],
    [`${num((architecture.components || []).length)} ${localized("components", "组件")}`, (architecture.components || []).length],
    [`${num((project.requirements || []).length)} ${localized("requirements", "需求")}`, (project.requirements || []).length],
    [`${num((project.changes || []).length)} ${localized("changes", "变更")}`, (project.changes || []).length],
    [`${num(project.structure?.entries?.length || 0)} ${localized("files", "文件")}`, project.structure?.entries?.length || 0],
  ];
  const covered = counts.filter(([, count]) => count > 0).map(([label]) => label);
  const partial = Boolean(project.structure?.truncated || project.git_review?.truncated);
  const body = covered.length
    ? covered.join(" · ")
    : localized("no loaded objects", "已加载对象为空");
  return `${localized("Searching the loaded snapshot", "仅搜索已加载的快照")} · ${body}${partial ? ` · ${localized("partial", "不完整")}` : ""}`;
}
function renderProjectNavigator({ open = false } = {}) {
  if (!els.projectNavigator || !els.navigatorResults || !els.navigatorListbox) return;
  const query = els.projectNavigator.value.trim().toLowerCase();
  if (!query || (!open && els.projectNavigator.getAttribute("aria-expanded") !== "true")) {
    els.navigatorResults.classList.add("hidden");
    els.projectNavigator.setAttribute("aria-expanded", "false");
    els.projectNavigator.removeAttribute("aria-activedescendant");
    return;
  }
  const matches = projectNavigatorItems().filter(item => `${item.kind} ${item.label} ${item.detail}`.toLowerCase().includes(query)).slice(0, 14);
  const html = matches.length ? matches.map((item, index) => `<button id="navigator-result-${index}" type="button" class="navigator-result" data-nav-index="${index}" data-nav-kind="${esc(item.kind)}" data-nav-id="${esc(item.id)}" role="option" aria-selected="${index === 0}"><span class="navigator-kind">${esc(navigatorKindLabel(item.kind))}</span><span class="navigator-copy"><strong>${esc(item.label)}</strong><small>${esc(item.detail)}</small></span></button>`).join("") : `<div class="empty">${esc(localized("No project item matches this search.", "没有匹配的项目项。"))}</div>`;
  setHtml("navigatorListbox", els.navigatorListbox, html, () => els.navigatorListbox.querySelectorAll("[data-nav-index]").forEach(button => button.addEventListener("click", () => activateProjectNavigatorItem(matches[Number(button.dataset.navIndex)]))));
  if (els.navigatorScope) els.navigatorScope.textContent = navigatorScopeNote();
  els.navigatorResults.classList.remove("hidden");
  els.projectNavigator.setAttribute("aria-expanded", "true");
  const selected = [...els.navigatorListbox.querySelectorAll("[data-nav-index]")].find(button => button.getAttribute("aria-selected") === "true");
  if (matches.length) els.projectNavigator.setAttribute("aria-activedescendant", selected?.id || "navigator-result-0");
  else els.projectNavigator.removeAttribute("aria-activedescendant");
}
function subsystemBlueprintTone(subsystem) {
  const drift = Number(subsystem.blocking_drift_edges || 0), changed = Number(subsystem.changed_components || 0), advisory = Number(subsystem.advisory_edges || 0);
  const a = architectureData(), ids = new Set(subsystem.component_ids || []);
  const candidates = (a.dependency_candidates || []).some(evidence =>
    [...(evidence.from_owners || []), ...(evidence.to_owners || [])].some(owner => ids.has(owner.component)));
  const observed = (a.dependencies || []).some(edge => edge.actual && (ids.has(edge.from) || ids.has(edge.to)));
  const incomplete = advisory || candidates || architectureOwnerSamplesIncomplete() || a.dependency_candidates_truncated || state.project?.structure?.truncated || !observed;
  return drift ? "drift" : changed ? "changed" : incomplete ? "uncertain" : "aligned";
}
function engineeringStage(tool) {
  const name = String(tool || "").toLowerCase();
  if (/(verify|verification|review_changes|language_quality|evidence|test|clippy|build)/.test(name)) return "prove";
  if (/(apply|write|create|move|delete|replace|run_command|reconciliation|authorize)/.test(name)) return "change";
  if (/(agent_context|software_context|search|read_|find_symbol|symbol_context|semantic|graph|design|traceability|impact|risk|scope|project_context|convention)/.test(name)) return "understand";
  return "observe";
}
function engineeringAge(at, active = false) {
  if (active) return localized("now", "现在");
  // A future or malformed timestamp is an observation problem, not something
  // that just happened; clamping it to zero would claim an impossible age.
  if (!Number.isFinite(at)) return localized("unknown time", "时间未知");
  const delta = Math.round((Date.now() - at) / 1000);
  if (delta < 0) return localized("in the future", "时间在未来");
  const seconds = delta;
  if (seconds < 60) return localized(`${seconds}s ago`, `${seconds} 秒前`);
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return localized(`${minutes}m ago`, `${minutes} 分钟前`);
  const hours = Math.round(minutes / 60);
  if (hours < 48) return localized(`${hours}h ago`, `${hours} 小时前`);
  const days = Math.round(hours / 24);
  return localized(`${days}d ago`, `${days} 天前`);
}
function renderEngineeringTimeline() {
  if (!state.project) return;
  const events = [], now = Date.now(), journal = state.project.engineering_journal || {};
  const milestones = journal.available === true ? journal.records || [] : [];
  for (const milestone of milestones) {
    const paths = milestone.paths || [], checks = milestone.checks_run == null ? "" : localized(` · checks ${num(milestone.checks_run)} / failed ${num(milestone.checks_failed || 0)}`, ` · 检查 ${num(milestone.checks_run)} / 失败 ${num(milestone.checks_failed || 0)}`);
    events.push({
      at: Number(milestone.timestamp_ms || 0), active: false,
      stage: milestone.stage === "model" ? "understand" : milestone.stage === "converge" ? "change" : milestone.stage,
      title: milestone.tool,
      detail: localized(`${milestone.outcome} · ${num(milestone.duration_ms)}ms${checks}${paths.length ? ` · ${paths.slice(0, 3).join(", ")}${paths.length > 3 ? ` +${paths.length - 3}` : ""}` : ""}`, `${statusLabel(milestone.outcome)} · ${num(milestone.duration_ms)}ms${checks}${paths.length ? ` · ${paths.slice(0, 3).join("、")}${paths.length > 3 ? ` +${paths.length - 3}` : ""}` : ""}`),
      status: milestone.outcome === "failed" ? "failed" : ["partial", "blocked"].includes(milestone.outcome) ? "blocked" : "completed",
      meta: localized("Persistent milestone", "持久工程里程碑"), persistent: true,
    });
  }
  const activity = state.activitySnapshot?.activity || state.project.activity;
  for (const task of activity?.recent || []) {
    const active = task.status === "running" || task.status === "queued";
    const at = active || task.finished_ago_ms == null ? now : now - Number(task.finished_ago_ms || 0);
    if (!active && milestones.some(milestone => milestone.tool === task.tool && Math.abs(Number(milestone.timestamp_ms || 0) - at) < 5000)) continue;
    const run = task.run_ms == null ? "—" : task.run_ms < 1000 ? `${Math.round(task.run_ms)}ms` : `${(task.run_ms / 1000).toFixed(1)}s`;
    events.push({
      at, active,
      stage: engineeringStage(task.tool),
      title: task.tool || localized("Harness task", "Harness 任务"),
      detail: localized(`task #${num(task.id)} · ${task.status} · wait ${Math.round(task.wait_ms || 0)}ms · run ${run}`, `任务 #${num(task.id)} · ${statusLabel(task.status)} · 等待 ${Math.round(task.wait_ms || 0)}ms · 执行 ${run}`),
      status: task.status,
      meta: localized("Harness activity", "Harness 活动"),
    });
  }
  const proofItems = state.project.proof?.effective?.items || [];
  for (const item of proofItems.slice(0, 6)) {
    if (!item.timestamp_ms) continue;
    events.push({
      at: Number(item.timestamp_ms), active: false, stage: "prove",
      title: item.subject || localized("Check results", "验证证据"),
      detail: localized(`${item.producer || "unknown producer"} · ${item.result || "unknown"} · ${item.policy || "no policy"}`, `${item.producer || "未知来源"} · ${statusLabel(item.result || "unknown")} · ${item.policy || "无策略"}`),
      status: item.result === "pass" ? "completed" : item.result === "fail" ? "failed" : "completed",
      meta: localized("Revision-bound evidence", "版本绑定证据"),
    });
  }
  const delta = state.project.latest_delta;
  if (delta?.to_captured_at_ms) {
    events.push({
      at: Number(delta.to_captured_at_ms), active: false, stage: "observe",
      title: localized("Architecture revision observed", "观测到架构版本变化"),
      detail: localized(`nodes +${num(delta.added_nodes)} / -${num(delta.removed_nodes)} / ~${num(delta.changed_nodes)} · edges +${num(delta.added_edges)} / -${num(delta.removed_edges)} / ~${num(delta.changed_edges)}`, `节点 +${num(delta.added_nodes)} / -${num(delta.removed_nodes)} / ~${num(delta.changed_nodes)} · 依赖 +${num(delta.added_edges)} / -${num(delta.removed_edges)} / ~${num(delta.changed_edges)}`),
      status: "completed",
      meta: localized("Software Graph revision", "软件图谱版本"),
    });
  }
  events.sort((left, right) => right.at - left.at);
  const stageLabel = stage => ({ understand: localized("Understand", "理解"), change: localized("Change", "修改"), prove: localized("Prove", "证明"), learn: localized("Learn", "学习"), observe: localized("Observe", "观测") })[stage] || stage;
  const rows = events.slice(0, 16).map(event => {
    const tone = event.status === "failed" ? "failed" : event.active ? "active" : "";
    return `<div class="engineering-event ${tone}"><span class="event-time">${esc(engineeringAge(event.at, event.active))}</span><span class="event-stage">${esc(stageLabel(event.stage))}</span><div class="event-main"><strong>${esc(event.title)}</strong><small>${esc(event.detail)}</small></div><span class="event-meta">${esc(event.meta)}</span></div>`;
  }).join("");
  setHtml("engineeringTimeline", els.engineeringTimeline, `<div class="engineering-timeline">${rows || `<div class="empty">${esc(localized("No retained engineering events yet.", "暂无保留的工程事件。"))}</div>`}</div>`);
}
function systemMapTiers(subsystems) {
  return [
    { id: "orchestration", label: localized("Orchestration", "编排层"), detail: localized("Coordinates agents and complex workflows", "协调 Agent 与复杂工作流"), items: subsystems.filter(item => Number(item.layer) >= 3) },
    { id: "composition", label: localized("Composition", "组合层"), detail: localized("Core services and domain capabilities", "核心服务与领域能力"), items: subsystems.filter(item => Number(item.layer) === 2) },
    { id: "consumers", label: localized("Consumers", "消费层"), detail: localized("User-facing interfaces and integrations", "面向用户的界面与集成"), items: subsystems.filter(item => Number(item.layer) === 1) },
    { id: "foundation", label: localized("Foundation", "基础层"), detail: localized("Platform, runtime and infrastructure", "平台、运行时与基础设施"), items: subsystems.filter(item => Number(item.layer) === 0) },
  ].filter(tier => tier.items.length);
}
function subsystemIconName(subsystem, tierIndex = 0) {
  const label = `${subsystem?.title || ""} ${subsystem?.id || ""}`.toLowerCase();
  if (/(security|auth|policy)/.test(label)) return "shield";
  if (/(storage|database|index|runtime)/.test(label)) return "database";
  if (/(graph|reconciliation|dependency)/.test(label)) return "network";
  if (/(context|knowledge)/.test(label)) return "layers";
  if (/(schedule|planner|verification)/.test(label)) return label.includes("schedule") ? "calendar" : "document";
  if (/(cli|command)/.test(label)) return "terminal";
  if (/(web|tui|ui|experience|observ)/.test(label)) return label.includes("observ") ? "chart" : "monitor";
  if (/(api|connector|integration)/.test(label)) return "link";
  return ["settings", "layers", "monitor", "cube"][tierIndex] || "cube";
}
function subsystemToneLabel(subsystem) {
  const tone = subsystemBlueprintTone(subsystem);
  return {
    aligned: [localized("Dependencies observed", "依赖已观测"), "good"],
    changed: [localized("Changed", "有变更"), "warn"],
    uncertain: [localized("Advisory", "提示"), "info"],
    drift: [localized("Drift", "偏离"), "bad"],
  }[tone] || [localized("Observed", "已观测"), "info"];
}
function subsystemDependencyIds(subsystem) {
  return [...new Set([...(subsystem.depends_on || []), ...(subsystem.designed_depends_on || []), ...(subsystem.observed_depends_on || [])])];
}
function subsystemAliases(subsystems) {
  const aliases = new Map(), titleCounts = new Map();
  for (const subsystem of subsystems) {
    aliases.set(subsystem.id, subsystem);
    const title = String(subsystem.title || "").trim();
    if (title) titleCounts.set(title, (titleCounts.get(title) || 0) + 1);
  }
  for (const subsystem of subsystems) {
    const title = String(subsystem.title || "").trim();
    if (title && titleCounts.get(title) === 1) aliases.set(title, subsystem);
  }
  return aliases;
}
function subsystemDependencyTargets(subsystem, aliases) {
  const targets = new Map();
  for (const id of subsystemDependencyIds(subsystem)) {
    const target = aliases.get(id);
    if (target && target.id !== subsystem.id) targets.set(target.id, target);
  }
  return [...targets.values()];
}
function clampSystemMapScale(value) {
  return Math.min(1.35, Math.max(.5, Number(value) || 1));
}
function applySystemMapScale() {
  const scale = clampSystemMapScale(state.systemMapScale);
  state.systemMapScale = scale;
  els.architectureBlueprint?.style?.setProperty?.("--system-map-scale", String(scale));
  if (els.systemMapZoomValue) els.systemMapZoomValue.textContent = `${Math.round(scale * 100)}%`;
  if (els.systemMapFit) els.systemMapFit.setAttribute("aria-pressed", String(Boolean(state.systemMapFit)));
  if (els.systemMapZoomOut) els.systemMapZoomOut.disabled = scale <= .501;
  if (els.systemMapZoomIn) els.systemMapZoomIn.disabled = scale >= 1.349;
}
function setSystemMapScale(value, { fit = false } = {}) {
  state.systemMapScale = clampSystemMapScale(value);
  state.systemMapFit = fit;
  applySystemMapScale();
}
function fitSystemMap() {
  const canvas = document.querySelector(".architecture-canvas"), map = els.architectureBlueprint?.querySelector(".system-map");
  if (!canvas || !map || typeof map.getBoundingClientRect !== "function") return setSystemMapScale(1, { fit: true });
  const current = clampSystemMapScale(state.systemMapScale), rect = map.getBoundingClientRect(),
    rawWidth = rect.width / current,
    availableWidth = Math.max(1, canvas.clientWidth - 8),
    scale = Math.min(1, availableWidth / Math.max(1, rawWidth));
  setSystemMapScale(scale, { fit: true });
  canvas.scrollTo?.({ left: 0, top: canvas.scrollTop, behavior: "smooth" });
}
function syncSystemMapFullControl() {
  document.querySelector(".architecture-layout")?.classList.toggle("full-map", state.systemMapFull);
  if (!els.systemMapFull) return;
  els.systemMapFull.textContent = state.systemMapFull
    ? localized("Exit full map", "退出全图")
    : localized("Open full map", "打开全图");
  els.systemMapFull.setAttribute("aria-pressed", String(state.systemMapFull));
}
function setSystemMapFull(enabled) {
  state.systemMapFull = Boolean(enabled);
  syncSystemMapFullControl();
  if (state.systemMapFit) requestAnimationFrame(fitSystemMap);
}
function renderSubsystemInspector() {
  const a = architectureData(), subsystems = a.subsystems || [];
  if (!subsystems.length) {
    els.componentInspector.classList.remove("open");
    return setHtml("componentInspector", els.componentInspector, "");
  }
  const subsystem = subsystems.find(item => item.id === state.selectedSubsystem);
  if (!subsystem) {
    els.componentInspector.classList.remove("open");
    setHtml("componentInspector", els.componentInspector, "");
    return;
  }
  const aliases = subsystemAliases(subsystems);
  const outgoing = subsystemDependencyTargets(subsystem, aliases);
  const incoming = subsystems.filter(item => subsystemDependencyTargets(item, aliases).some(target => target.id === subsystem.id));
  const components = subsystemComponents(subsystem);
  const requirements = new Set(components.flatMap(component => component.requirements || []));
  const evidence = (state.project?.proof?.effective?.items || []).filter(item => {
    const subject = String(item.subject || "").toLowerCase();
    return subject.includes(String(subsystem.id || "").toLowerCase()) || components.some(component => subject.includes(String(component.id || "").toLowerCase()));
  }).slice(0, 4);
  const [status, statusTone] = subsystemToneLabel(subsystem);
  const chipList = (items, empty) => items.length
    ? `<div class="inspector-chip-list">${items.slice(0, 5).map(item => `<span class="inspector-chip">${esc(item.title || item.name || item.id)}</span>`).join("")}${items.length > 5 ? `<span class="inspector-chip more">+${items.length - 5}</span>` : ""}</div>`
    : `<span class="panel-meta">${esc(empty)}</span>`;
  const evidenceHtml = evidence.length
    ? `<div class="inspector-evidence-list">${evidence.map(item => `<div><strong>${esc(item.subject || localized("Evidence", "证据"))}</strong><span>${esc(statusLabel(item.result))} · ${esc(time(item.timestamp_ms))}</span></div>`).join("")}</div>`
    : `<span class="panel-meta">${esc(localized("No directly linked current evidence", "暂无直接关联的当前证据"))}</span>`;
  const html = `<div class="inspector-panel-head"><div><span class="inspector-panel-glyph">${uiIcon("network")}</span><strong>${esc(localized("Subsystem inspector", "子系统检查器"))}</strong></div><button type="button" class="inspector-collapse" data-subsystem-inspector-close aria-label="${esc(localized("Open full map", "打开全图"))}">${uiIcon("chevron-up")}</button></div><div class="subsystem-inspector-head"><div class="subsystem-inspector-icon">${uiIcon(subsystemIconName(subsystem))}</div><div><div class="subsystem-inspector-title"><h3>${esc(subsystem.title)}</h3><span class="depth-chip">L${num(subsystem.layer || 0)}</span></div><p>${esc(subsystem.purpose || localized("Subsystem responsibility is not declared yet.", "尚未声明该子系统职责。"))}</p></div>${pill(status, statusTone)}</div><div class="inspector-health compact subsystem-inspector-quick-stats"><div><strong>${num(subsystem.components || components.length)}</strong><span>${esc(localized("components", "组件"))}</span></div><div><strong>${num(subsystem.implementation_files || 0)}</strong><span>${esc(localized("files", "文件"))}</span></div><div><strong>${num(subsystem.requirements || requirements.size)}</strong><span>${esc(localized("requirements", "需求"))}</span></div><div><strong>${num(subsystem.changed_components || 0)}</strong><span>${esc(localized("changed", "变更"))}</span></div></div>
    <section class="subsystem-inspector-section"><h4>${esc(localized("Owned components", "所属组件"))}<span>${num(components.length)}</span></h4>${chipList(components, localized("No mapped components", "没有映射组件"))}<button type="button" class="inspector-link" data-inspector-components>${esc(localized("View all components", "查看全部组件"))}<span class="inline-arrow" aria-hidden="true"></span></button></section>
    <section class="subsystem-inspector-section"><h4>${esc(localized("Upstream dependencies", "上游依赖"))}<span>${num(outgoing.length)}</span></h4>${chipList(outgoing, localized("No upstream subsystem dependencies", "没有上游子系统依赖"))}<button type="button" class="inspector-link" data-inspector-dependencies>${esc(localized("View dependency ledger", "查看依赖账本"))}<span class="inline-arrow" aria-hidden="true"></span></button></section>
    <section class="subsystem-inspector-section"><h4>${esc(localized("Downstream consumers", "下游消费方"))}<span>${num(incoming.length)}</span></h4>${chipList(incoming, localized("No downstream subsystem consumers", "没有下游消费方"))}</section>
    <section class="subsystem-inspector-section"><h4>${esc(localized("Drift status", "偏离状态"))}</h4><div class="inspector-status-line">${pill(status, statusTone)}<span>${esc(localized(`${num(subsystem.blocking_drift_edges || 0)} blocking · ${num(subsystem.advisory_edges || 0)} advisory`, `${num(subsystem.blocking_drift_edges || 0)} 阻塞 · ${num(subsystem.advisory_edges || 0)} 提示`))}</span></div></section>
    <section class="subsystem-inspector-section"><h4>${esc(localized("Linked evidence", "关联证据"))}<span>${num(evidence.length)}</span></h4>${evidenceHtml}<button type="button" class="inspector-link" data-inspector-proof>${esc(localized("View verification evidence", "查看验证证据"))}</button></section>`;
  els.componentInspector.classList.add("open");
  setHtml("componentInspector", els.componentInspector, html, () => {
    els.componentInspector.querySelector("[data-subsystem-inspector-close]")?.addEventListener("click", () => setSystemMapFull(true));
    els.componentInspector.querySelector("[data-inspector-components]")?.addEventListener("click", () => { state.architectureView = "components"; renderArchitecture(); });
    els.componentInspector.querySelector("[data-inspector-dependencies]")?.addEventListener("click", () => { state.architectureView = "graph"; renderArchitecture(); });
    els.componentInspector.querySelector("[data-inspector-proof]")?.addEventListener("click", () => revealSection("proofSection"));
  });
}
function renderArchitectureBlueprint() {
  const a = architectureData(), subsystems = a.subsystems || [];
  if (!subsystems.length) {
    setHtml("architectureBlueprint", els.architectureBlueprint, `<div class="empty">${esc(localized("No subsystem map can be derived yet. Add Design components or implementation ownership.", "暂时无法推导系统地图。请补充 Design Component 或实现归属。"))}</div>`);
    els.componentInspector.classList.remove("open");
    return;
  }
  if (state.selectedSubsystem && !subsystems.some(item => item.id === state.selectedSubsystem)) state.selectedSubsystem = "";
  if (!state.selectedSubsystem) {
    const preferred = subsystems.find(item => Number(item.blocking_drift_edges || 0) > 0) ||
      subsystems.find(item => Number(item.changed_components || 0) > 0) ||
      [...subsystems].sort((left, right) => Number(right.layer || 0) - Number(left.layer || 0) || left.title.localeCompare(right.title))[0];
    state.selectedSubsystem = preferred?.id || "";
    const first = preferred && subsystemComponents(preferred)[0];
    if (first) state.selectedComponent = first.id;
  }
  const project = state.project || {}, systemName = project.product || project.project || project.workspace || "Project";
  const runtimeDrift = (project.risk?.drift?.findings || []).filter(item => item.kind === "runtime_drift"),
    runtimeDeviation = runtimeDrift.reduce((value, item) => Math.max(value, Number(item.deviation?.deviation_percent || 0)), 0),
    // The peak keeps its own values so the percentage never stands alone.
    runtimePeak = runtimeDrift.reduce((peak, item) => Number(item.deviation?.deviation_percent || 0) > Number(peak?.deviation_percent || 0) ? item.deviation : peak, null),
    structuralDrift = Number(a.blocking_drift_edges || 0), hasDrift = structuralDrift > 0 || runtimeDrift.length > 0;
  const aliases = subsystemAliases(subsystems);
  const relationEdges = subsystems.flatMap(source => subsystemDependencyTargets(source, aliases).map(target => ({ source, target })));
  const changedSubsystems = subsystems.filter(item => Number(item.changed_components || 0) > 0);
  const driftSubsystems = subsystems.filter(item => Number(item.blocking_drift_edges || 0) > 0);
  const evidenceCount = Number(project.proof?.effective?.total || project.proof?.current_evidence || 0);
  const tiers = systemMapTiers(subsystems);
  const root = `<div class="system-map-root"><div class="system-root-mark">${uiIcon("cube")}</div><div class="system-root-main"><div class="system-root-title"><strong>${esc(systemName)}</strong><span class="root-kind">${esc(localized("System root", "系统根节点"))}</span></div><div class="system-root-stats"><span>${uiIcon("cube")} ${num(a.components?.length || 0)} ${esc(localized("components", "组件"))}</span><span>${uiIcon("document")} ${num(project.code?.source_files || project.structure?.entries?.length || 0)} ${esc(localized("files", "文件"))}</span><span>${uiIcon("check")} ${num((project.requirements || []).length)} ${esc(localized("requirements", "需求"))}</span>${runtimeDrift.length ? `<span>${uiIcon("chart")} ${num(runtimeDrift.length)} ${esc(localized("runtime drift", "运行时偏离"))}${runtimePeak && runtimePeak.observed != null && runtimePeak.expected_max != null ? ` · ${num(runtimeDeviation)}% (${num(runtimePeak.observed)} / ${num(runtimePeak.expected_max)} ${runtimePeak.unit || ""})` : ""}</span>` : ""}</div></div>${pill(hasDrift ? localized("Needs attention", "需要处理") : architectureObservationIncomplete() ? localized("Needs stronger evidence", "待补证据") : localized("Dependencies observed", "依赖已观测"), hasDrift ? (structuralDrift ? "bad" : "warn") : architectureObservationIncomplete() ? "info" : "good")}</div>`;
  const overview = `<div class="system-map-kpis"><div><span>${esc(localized("Subsystems", "子系统"))}</span><strong>${num(subsystems.length)}</strong></div><div><span>${esc(localized("Dependency paths", "依赖路径"))}</span><strong>${num(relationEdges.length)}</strong></div><div class="${changedSubsystems.length ? "warn" : ""}"><span>${esc(localized("Changed subsystems", "变更子系统"))}</span><strong>${num(changedSubsystems.length)}</strong></div><div class="${driftSubsystems.length ? "bad" : ""}"><span>${esc(localized("Blocking drift", "阻塞偏离"))}</span><strong>${num(driftSubsystems.length)}</strong></div><div><span>${esc(localized("Check results", "验证证据"))}</span><strong>${num(evidenceCount)}</strong></div></div>`;
  const priorityRelations = [...relationEdges].sort((left, right) => Number(right.source.blocking_drift_edges || 0) - Number(left.source.blocking_drift_edges || 0) || Number(right.source.changed_components || 0) - Number(left.source.changed_components || 0) || left.source.title.localeCompare(right.source.title)).slice(0, 12);
  const relationOverview = priorityRelations.length ? `<section class="system-map-relations"><header><div><strong>${esc(localized("Architecture flow", "架构流向"))}</strong><span>${esc(localized("Highest-signal subsystem dependencies; select a path to inspect its source subsystem.", "优先展示高信号子系统依赖；点击路径可检查来源子系统。"))}</span></div><small>${num(relationEdges.length)} ${esc(localized("relations", "条关系"))}</small></header><div class="system-map-relation-grid">${priorityRelations.map(({ source, target }) => `<button type="button" class="system-map-relation ${subsystemBlueprintTone(source)}" data-subsystem-card="${esc(source.id)}"><strong>${esc(source.title)}</strong><span aria-hidden="true">→</span><b>${esc(target.title)}</b><small>L${num(source.layer || 0)} → L${num(target.layer || 0)}</small></button>`).join("")}</div></section>` : "";
  const tierHtml = tiers.map((tier, tierIndex) => {
    const cards = [...tier.items].sort((left, right) => Number(right.layer || 0) - Number(left.layer || 0) || left.title.localeCompare(right.title)).map(subsystem => {
      const [status, tone] = subsystemToneLabel(subsystem), selected = subsystem.id === state.selectedSubsystem;
      const outgoing = subsystemDependencyTargets(subsystem, aliases);
      const incoming = subsystems.filter(item => subsystemDependencyTargets(item, aliases).some(target => target.id === subsystem.id));
      const relationChips = outgoing.slice(0, 2).map(item => `<span class="subsystem-relation-chip outgoing">→ ${esc(item.title)}</span>`).concat(incoming.slice(0, 2).map(item => `<span class="subsystem-relation-chip incoming">← ${esc(item.title)}</span>`));
      const hiddenRelations = Math.max(0, outgoing.length + incoming.length - relationChips.length);
      return `<button type="button" class="subsystem-card ${subsystemBlueprintTone(subsystem)}${selected ? " selected" : ""}" data-subsystem-card="${esc(subsystem.id)}" aria-pressed="${selected}"><span class="subsystem-card-top"><span class="subsystem-icon">${uiIcon(subsystemIconName(subsystem, tierIndex))}</span><strong>${esc(subsystem.title)}</strong></span><span class="subsystem-card-purpose">${esc(subsystem.purpose || localized("Responsibility not declared", "职责尚未声明"))}</span><span class="subsystem-card-state"><span class="depth-chip">L${num(subsystem.layer || 0)}</span>${pill(status, tone)}</span><span class="subsystem-card-relations">${relationChips.join("") || `<span class="subsystem-relation-chip quiet">${esc(localized("No subsystem dependency", "无子系统依赖"))}</span>`}${hiddenRelations ? `<span class="subsystem-relation-chip more">+${hiddenRelations}</span>` : ""}</span><span class="subsystem-card-bottom"><span class="subsystem-mini-stat">${uiIcon("cube")}<b>${num(subsystem.components || 0)}</b><small>${esc(localized("components", "组件"))}</small></span><span class="subsystem-mini-stat">${uiIcon("document")}<b>${num(subsystem.implementation_files || 0)}</b><small>${esc(localized("files", "文件"))}</small></span><span class="subsystem-mini-stat">${uiIcon("check")}<b>${num(subsystem.requirements || 0)}</b><small>${esc(localized("requirements", "需求"))}</small></span><span class="subsystem-mini-stat">${uiIcon("chart")}<b>${num(subsystem.changed_components || 0)}</b><small>${esc(localized("changed", "变更"))}</small></span></span></button>`;
    }).join("");
    const bridge = tierIndex < tiers.length - 1 ? `<div class="tier-flow-bridge" aria-hidden="true"></div>` : "";
    return `<div class="system-tier-block"><section class="system-tier tier-${tier.id}"><header class="system-tier-head"><span class="tier-index">L${tierIndex + 1}</span><div><strong>${esc(tier.label)}</strong><span>${esc(tier.detail)}</span></div><small>${num(tier.items.length)} ${esc(localized("subsystems", "个子系统"))}</small></header><div class="system-tier-grid">${cards}</div></section>${bridge}</div>`;
  }).join("");
  const html = `<div class="system-map">${root}${overview}${relationOverview}${tierHtml}</div>`;
  setHtml("architectureBlueprint", els.architectureBlueprint, html, () => {
    els.architectureBlueprint.querySelectorAll("[data-subsystem-card]").forEach(button => button.addEventListener("click", () => {
      setSystemMapFull(false);
      state.selectedSubsystem = button.dataset.subsystemCard;
      const subsystem = subsystems.find(item => item.id === state.selectedSubsystem), first = subsystem && subsystemComponents(subsystem)[0];
      if (first) state.selectedComponent = first.id;
      invalidate("architectureBlueprint", "componentInspector");
      renderArchitectureBlueprint();
      renderSubsystemInspector();
      syncInvestigationHash();
      requestAnimationFrame(() => focusSelectedSubsystemCard({ focus: true }));
    }));
  });
  applySystemMapScale();
  setSystemMapFull(state.systemMapFull);
  renderSubsystemInspector();
}
function renderArchitectureGraph() {
  const a = architectureData(), components = a.components || [], allEdges = (a.dependencies || []).filter(architectureEdgeVisible);
  const candidates = ["overlay", "unverified"].includes(state.architectureMode) ? architectureCandidateEvidence() : "";
  const componentNames = new Map(components.map(component => [component.id, component.name]));
  const rows = [...allEdges].sort((left, right) => Number(Boolean(right.blocking)) - Number(Boolean(left.blocking)) || String(left.from_name || left.from).localeCompare(String(right.from_name || right.from))).map(edge => {
    const tone = architectureEdgeTone(edge), source = edge.from_name || componentNames.get(edge.from) || edge.from, target = edge.to_name || componentNames.get(edge.to) || edge.to;
    const evidence = edge.desired && edge.actual ? localized("Design + observed", "设计 + 已观测") : edge.desired ? localized("Declared, unobserved", "设计已声明、未观测") : localized("Observed, undeclared", "已观测、设计未声明");
    return `<div class="dependency-record"><button type="button" class="dependency-row ${tone}" data-dependency-component="${esc(edge.from)}" role="row"><span role="cell"><strong>${esc(source)}</strong><small>${esc(edge.from)}</small></span><span class="dependency-direction" role="cell">${esc(localized("depends on", "依赖"))}</span><span role="cell"><strong>${esc(target)}</strong><small>${esc(edge.to)}</small></span><span role="cell">${pill(evidence, tone === "drift" ? "bad" : tone === "unverified" ? "warn" : "info")}</span><span role="cell"><strong>${esc(statusLabel(edge.status))}</strong><small>${esc(statusLabel(edge.precision))}</small></span></button>${architectureEdgeEvidence(edge)}</div>`;
  }).join("");
  const ledger = rows
    ? `<div class="dependency-ledger-head" role="row"><span>${esc(localized("Source", "来源"))}</span><span>${esc(localized("Relation", "关系"))}</span><span>${esc(localized("Target", "目标"))}</span><span>${esc(localized("Evidence", "证据"))}</span><span>${esc(localized("State", "状态"))}</span></div><div class="dependency-rows">${rows}</div>`
    : `<div class="empty">${esc(localized("No confirmed component dependency in this view.", "当前视图没有已确认的组件依赖。"))}</div>`;
  setHtml("architectureGraph", els.architectureGraph, architectureObservationNote() + ledger + candidates, () => {
    const rows = [...els.architectureGraph.querySelectorAll("[data-dependency-component]")];
    rows.forEach((button, index) => {
      button.addEventListener("click", () => {
        state.selectedComponent = button.dataset.dependencyComponent;
        renderComponentInspector();
        syncInvestigationHash();
      });
      button.addEventListener("keydown", event => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1 : event.key === "ArrowDown" ? Math.min(rows.length - 1, index + 1) : Math.max(0, index - 1);
        rows[nextIndex]?.focus();
        rows[nextIndex]?.scrollIntoView({ block: "nearest" });
      });
    });
  });
}
function renderArchitectureInspector() {
  if (state.architectureView === "blueprint") renderSubsystemInspector();
  else renderComponentInspector();
}
function renderEngineeringFlow() {
  if (!state.project || !els.engineeringFlow) return;
  const project = state.project, activity = state.activitySnapshot?.activity || project.activity,
    recent = activity?.recent || [], learning = project.verified_learning || {}, proof = effectiveProof(),
    liveFor = stage => recent.filter(task => engineeringStage(task.tool) === stage && ["queued", "running"].includes(task.status)).length,
    planTasks = recent.filter(task => /(plan|impact|reconciliation|review_changes|verification_impact)/i.test(String(task.tool || ""))),
    stages = [
      { id: "understand", icon: "book", title: localized("System understanding", "系统理解"), en: "Understand", input: localized("Architecture · requirements · context", "架构 · 需求 · 上下文"), bullets: [localized("Identify system boundaries", "识别系统边界"), localized("Locate target modules", "定位目标模块")], metric: localized(`${num(liveFor("understand"))} live context tasks`, `${num(liveFor("understand"))} 项实时理解任务`) },
      { id: "plan", icon: "document", title: localized("Plan change", "规划变更"), en: "Plan", input: localized("Current state · change goal", "系统现状 · 变更目标"), bullets: [localized("Bound the change scope", "明确变更范围"), localized("Choose verification strategy", "选择验证策略")], metric: localized(`${num(planTasks.length)} planning signals`, `${num(planTasks.length)} 项规划信号`) },
      { id: "implement", icon: "code", title: localized("Implement", "执行改动"), en: "Implement", input: localized("Change plan · codebase", "变更计划 · 代码库"), bullets: [localized("Edit code / configuration", "修改代码 / 配置"), localized("Record the change chain", "记录变更链")], metric: localized(`${num(liveFor("change"))} live change tasks`, `${num(liveFor("change"))} 项实时变更任务`) },
      { id: "prove", icon: "check", title: localized("Prove", "验证证明"), en: "Prove", input: localized("Change set · verification strategy", "变更内容 · 验证策略"), bullets: [localized("Run effective checks", "运行有效检查"), localized("Bind evidence to revision", "生成版本绑定证据")], metric: localized(`${num(proof.current_evidence || 0)} current evidence`, `${num(proof.current_evidence || 0)} 条当前证据`) },
      { id: "learn", icon: "database", title: localized("Learn", "经验沉淀"), en: "Learn", input: localized("Evidence · stable experience", "验证结果 · 稳定经验"), bullets: [localized("Keep successful patterns", "保留成功模式"), localized("Update reusable experience", "更新上下文与经验")], metric: localized(`${num(learning.records || 0)} verified records`, `${num(learning.records || 0)} 条已验证经验`) },
      { id: "observe", icon: "chart", title: localized("Observe again", "重新观测"), en: "Observe", input: localized("Current system · history baseline", "当前系统 · 历史基线"), bullets: [localized("Compare design and actual", "比较设计与实现"), localized("Identify drift and risk", "识别偏离与风险")], metric: localized(`${num(project.architecture?.blocking_drift_edges || 0)} confirmed drift`, `${num(project.architecture?.blocking_drift_edges || 0)} 条确认偏离`) },
    ];
  const stageHtml = stages.map((stage, index) => `<article class="cycle-stage cycle-${stage.id}"><div class="cycle-stage-top"><span class="cycle-number">0${index + 1}</span><span class="cycle-glyph">${uiIcon(stage.icon)}</span></div><h3>${esc(stage.title)}</h3><span class="cycle-en">${esc(stage.en)}</span><div class="cycle-input"><small>${esc(localized("Input", "输入"))}</small><strong>${esc(stage.input)}</strong></div><ul>${stage.bullets.map(item => `<li>${esc(item)}</li>`).join("")}</ul><span class="cycle-metric">${esc(stage.metric)}</span></article>`).join("");
  const support = [
    ["document", localized("Design State", "Design State"), localized(`${num((project.requirements || []).length)} requirements · ${project.design_valid === false ? localized("needs attention", "需要处理") : localized("valid", "有效")}`, `${num((project.requirements || []).length)} 个需求 · ${project.design_valid === false ? "需要处理" : "有效"}`), "design"],
    ["code", localized("Codebase", "代码库"), project.git_review?.available === true
      ? localized(`${num(project.code?.source_files || project.code?.files || project.structure?.entries?.length || 0)} files · ${num(project.changes?.length || 0)} changed`, `${num(project.code?.source_files || project.code?.files || project.structure?.entries?.length || 0)} 个文件 · ${num(project.changes?.length || 0)} 个变更`)
      : localized(`${num(project.code?.source_files || project.code?.files || project.structure?.entries?.length || 0)} files · changed unknown`, `${num(project.code?.source_files || project.code?.files || project.structure?.entries?.length || 0)} 个文件 · 变更未知`), "code"],
    ["check", localized("Verification", "验证"), localized(`${num(proof.current_verification_ready || 0)} ready · ${num(proof.current_failed || 0)} failed`, `${num(proof.current_verification_ready || 0)} 个就绪 · ${num(proof.current_failed || 0)} 个失败`), "verify"],
    ["database", localized("Evidence Store", "证据库"), localized(`${num(proof.current_evidence || 0)} current · ${num(learning.records || 0)} learned`, `${num(proof.current_evidence || 0)} 条当前证据 · ${num(learning.records || 0)} 条经验`), "evidence"],
  ].map(([icon, title, detail, tone]) => `<div class="cycle-support cycle-support-${tone}"><span class="cycle-support-glyph">${uiIcon(icon)}</span><div><strong>${esc(title)}</strong><small>${esc(detail)}</small></div></div>`).join("");
  const html = `<div class="cycle-loop" aria-hidden="true"><span>${esc(localized("Continuous feedback · improve again", "持续反馈 · 不断改进"))}</span></div><div class="engineering-cycle-main">${stageHtml}</div><div class="cycle-support-grid">${support}</div>`;
  setHtml("engineeringFlow", els.engineeringFlow, html);
}

function renderTraceabilityMap() {
  if (!state.project || !els.traceabilityMap) return;
  const project = state.project, proof = project.proof || {}, acceptance = proof.acceptance || {},
    requirements = (project.requirements || []).slice(0, 3), componentIndex = new Map();
  for (const requirement of project.requirements || []) for (const component of requirement.components || []) if (!componentIndex.has(component.id)) componentIndex.set(component.id, component);
  const components = [...componentIndex.values()].slice(0, 4), codeRefs = [];
  for (const component of componentIndex.values()) for (const implementation of component.implementation || []) if (implementation?.target && !codeRefs.some(item => item.target === implementation.target)) codeRefs.push(implementation);
  const checks = [];
  for (const requirement of project.requirements || []) for (const criterion of requirement.acceptance || []) for (const verification of criterion.verification || []) if (verification?.target && !checks.some(item => item.target === verification.target)) checks.push({ ...verification, criterion: criterion.title || criterion.id });
  const evidence = (proof.effective?.items || []).slice(0, 4);
  const card = (title, detail, meta, attrs = "") => `<button type="button" class="trace-card" ${attrs}><strong>${esc(title)}</strong><span>${esc(detail || "—")}</span><small>${esc(meta || "")}</small></button>`;
  const columns = [
    [localized("What must change?", "要做什么？"), localized("Requirements", "需求"), "document", requirements.map(item => card(item.id, item.title, item.intent, `data-trace-req="${esc(item.id)}"`))],
    [localized("Who owns it?", "由谁负责？"), localized("Components", "组件"), "cube", components.map(item => card(item.name || item.id, (item.responsibilities || [])[0] || localized("Mapped implementation owner", "实现归属组件"), localized(`${num(item.implementation?.length || 0)} implementation refs`, `${num(item.implementation?.length || 0)} 个实现引用`), `data-trace-component="${esc(item.id)}"`))],
    [localized("Where is the code?", "落在哪些文件？"), localized("Code ownership", "代码"), "code", codeRefs.slice(0, 4).map(item => card(item.target, statusLabel(item.resolved ? "resolved" : "unresolved"), `${item.provider || "—"} / ${statusLabel(item.precision || "unknown")}`))],
    [localized("How is it verified?", "如何验证？"), localized("Checks", "验证"), "check", checks.slice(0, 4).map(item => card(item.target, item.criterion || localized("Verification check", "验证检查"), `${item.provider || "—"} / ${statusLabel(item.precision || "unknown")}`))],
    [localized("What proof remains?", "留下什么证据？"), localized("Evidence", "证据"), "database", evidence.map(item => card(statusLabel(item.result || "unknown"), item.subject || localized("Revision-bound evidence", "版本绑定证据"), `${item.producer || "—"} · ${time(item.timestamp_ms)}`))],
  ];
  const flow = columns.map(([question, title, icon, items], index) => `<section class="trace-column"><span class="trace-question">${esc(question)}</span><header><span class="trace-column-glyph">${uiIcon(icon)}</span><div><strong>${esc(title)}</strong><small>${["Requirements", "Components", "Code Ownership", "Checks", "Evidence"][index]}</small></div></header><div class="trace-column-items">${items.join("") || `<div class="trace-empty">${esc(localized("No current data", "暂无当前数据"))}</div>`}</div></section>`).join('<span class="trace-arrow" aria-hidden="true"></span>');
  const metrics = [["Mapped", acceptance.mapped, "accent"], ["Executed", acceptance.executed, "info"], ["Passed", acceptance.passed, "good"], ["Fresh", acceptance.fresh, "fresh"]].map(([label, value, tone]) => `<div class="convergence-chip ${tone}"><strong>${num(value || 0)} / ${num(acceptance.total || 0)}</strong><span>${esc(label)}</span></div>`).join("");
  const html = `<div class="trace-legend"><span><i class="legend-dot primary"></i>${esc(localized("Trace chain", "追踪主链"))}</span><span><i class="legend-dot good"></i>${esc(localized("Design mapping", "设计映射"))}</span><span><i class="legend-dot warn"></i>${esc(localized("Verification result", "验证结果"))}</span></div><div class="trace-flow">${flow}</div><div class="trace-convergence"><div><strong>${esc(localized("Convergence criteria", "收敛判断"))}</strong><small>${esc(localized("Independent dimensions — never substitute one for another.", "相互独立的判断维度，不可互相替代或混淆。"))}</small></div>${metrics}</div>`;
  setHtml("traceabilityMap", els.traceabilityMap, html, () => {
    els.traceabilityMap.querySelectorAll("[data-trace-req]").forEach(button => button.addEventListener("click", () => {
      setRequirementFilter("all"); state.selected = button.dataset.traceReq; els.search.value = ""; invalidate("requirements", "detail"); renderRequirements(); renderDetail(); syncInvestigationHash(); requestAnimationFrame(() => els.detail.scrollIntoView({ behavior: "smooth", block: "start" }));
    }));
    els.traceabilityMap.querySelectorAll("[data-trace-component]").forEach(button => button.addEventListener("click", () => {
      state.selectedComponent = button.dataset.traceComponent; state.architectureView = "components"; revealSection("architectureSection"); renderArchitecture(); syncInvestigationHash();
    }));
  });
}

// A convergence card names the object it talks about. Revealing only the
// section would drop the investigation context, so the card carries the id of
// the first object in its set and the destination focuses that object.
function convergenceFocus(target, id) {
  if (target === "architectureSection" && id) {
    if (!(architectureData().components || []).some(component => component.id === id)) { revealSection(target); return; }
    state.selectedComponent = id;
    state.architectureView = "components";
    renderArchitecture();
    revealSection(target);
    requestAnimationFrame(() => focusSelectedComponentCard({ focus: true }));
    return;
  }
  if (target === "requirementsSection" && id) {
    if (!(state.project?.requirements || []).some(item => item.id === id)) { revealSection(target); return; }
    setRequirementFilter("all");
    state.selected = id;
    if (els.search) els.search.value = "";
    invalidate("requirements", "detail");
    renderRequirements();
    renderDetail();
    revealSection(target);
    requestAnimationFrame(() => focusSelectedRequirement({ focus: true }));
    return;
  }
  if (target === "overviewSection" && id === "acceptance") {
    revealSection(target);
    const binding = document.getElementById("acceptancePolicyBinding");
    if (binding) { binding.open = true; binding.scrollIntoView({ block: "nearest" }); binding.focus({ preventScroll: true }); }
    return;
  }
  if (target === "overviewSection" && id === "attention") {
    revealSection(target);
    els.attention?.scrollIntoView({ behavior: "smooth", block: "start" });
    return;
  }
  revealSection(target);
}
function renderChangeConvergenceMap() {
  if (!state.project || !els.changeConvergenceMap) return;
  const project = state.project, changes = project.changes || [], impact = project.verification_impact || {}, proof = effectiveProof(),
    reviewAvailable = project.git_review?.available === true,
    components = new Set(), requirements = new Set();
  for (const change of changes) { for (const id of change.affected_components || []) components.add(id); for (const id of change.affected_requirements || []) requirements.add(id); }
  // Only objects the destination can actually show are named; an unknown id
  // must not be invented just to make the card look precise.
  const firstComponent = reviewAvailable
    ? (architectureData().components || []).find(component => components.has(component.id))?.id || "" : "";
  const firstRequirement = reviewAvailable
    ? (state.project?.requirements || []).find(item => requirements.has(item.id))?.id || "" : "";
  const reasons = impact.reasons || [], contracts = reasons.filter(item => item.kind === "contract_bridge"),
    configChanges = changes.filter(item => /(config|toml|json|ya?ml|lock|schema|proto|graphql|openapi|dependency|cargo)/i.test(String(item.path || ""))),
    risks = project.risk?.risks || [], drift = Number(project.architecture?.blocking_drift_edges || 0),
    currentEvidence = Number(proof.current_evidence || 0), currentFailed = Number(proof.current_failed || 0),
    acceptanceDecision = acceptanceView(),
    confidence = acceptanceStateLabel(acceptanceDecision.status),
    confidenceTone = acceptanceDecision.status === "ready" ? "good" : acceptanceDecision.status === "blocked" ? "bad" : "info";
  const inputCard = (icon, title, value, detail, tone) => `<article class="impact-input-card ${tone}"><span class="impact-glyph">${uiIcon(icon)}</span><div><strong>${esc(title)}</strong><b>${esc(value)}</b><small>${esc(detail)}</small></div></article>`;
  const inputHtml = [
    inputCard("code", localized("Code file changes", "代码文件变更"), reviewAvailable ? num(changes.length) : "—", reviewAvailable ? localized(`${num(project.code?.additions || 0)} additions · ${num(project.code?.deletions || 0)} deletions`, `+${num(project.code?.additions || 0)} / -${num(project.code?.deletions || 0)}`) : localized("review unavailable", "检查不可用"), "primary"),
    inputCard("network", localized("Shared contract / schema", "共享契约 / Schema"), num(contracts.length), localized("API, generated-code and cross-island bridges", "API、生成代码与跨项目岛桥接"), "info"),
    inputCard("settings", localized("Config & dependency", "配置与依赖"), num(configChanges.length), localized("Configuration, manifests and dependency surfaces", "配置、清单与依赖面"), "good"),
  ].join("");
  const impactCard = (icon, title, value, detail, tone, target, id = "") => `<button type="button" class="impact-analysis-card ${tone}" data-convergence-target="${target}"${id ? ` data-convergence-id="${esc(id)}"` : ""}><span class="impact-glyph">${uiIcon(icon)}</span><div><strong>${esc(title)}</strong><b>${esc(value)}</b><small>${esc(detail)}</small></div></button>`;
  const middleHtml = [
    impactCard("layers", localized("Affected components", "受影响组件"), reviewAvailable ? num(components.size) : "—", reviewAvailable ? localized("Owners, modules and dependency surface", "服务、模块与依赖关系") : localized("unknown", "未知"), "primary", "architectureSection", firstComponent),
    impactCard("document", localized("Affected requirements", "受影响需求"), reviewAvailable ? num(requirements.size) : "—", reviewAvailable ? localized("Intent, scenarios and acceptance criteria", "业务意图、场景与验收标准") : localized("unknown", "未知"), "info", "requirementsSection", firstRequirement),
    impactCard("warning", localized("Risk / drift", "风险 / 偏离"), num(risks.length + drift), localized(`${num(drift)} confirmed architecture drift`, `${num(drift)} 条确认架构偏离`), "warn", "overviewSection", "attention"),
    impactCard("check", localized("Suggested verification", "建议验证集"), impact.affected_islands?.length ? num(impact.affected_islands.length) : "—", impact.selective ? localized("Selective verification is available", "可进行选择性验证") : impact.affected_islands ? localized("Broad fallback / incomplete topology", "宽范围回退 / 拓扑不完整") : localized("No verification impact selection is recorded", "未记录验证影响选择"), "accent", "proofSection"),
  ].join("");
  const outcomeCard = (icon, title, value, detail, tone, target, id = "") => `<button type="button" class="convergence-outcome ${tone}" data-convergence-target="${target}"${id ? ` data-convergence-id="${esc(id)}"` : ""}><span class="impact-glyph">${uiIcon(icon)}</span><div><strong>${esc(title)}</strong><b>${esc(value)}</b><small>${esc(detail)}</small></div></button>`;
  const outcomeHtml = [
    outcomeCard("check", localized("Effective evidence", "有效证据"), num(currentEvidence), localized(`${num(proof.current_passed || 0)} passing · ${num(currentFailed)} failing`, `${num(proof.current_passed || 0)} 通过 · ${num(currentFailed)} 失败`), currentFailed ? "warn" : "good", "proofSection"),
    outcomeCard("warning", localized("Residual risk", "剩余风险"), num(risks.filter(item => ["critical", "high", "medium"].includes(item.level)).length), localized("Uncovered or unresolved risk signals", "尚未覆盖或未解决的风险信号"), "warn", "overviewSection", "attention"),
    outcomeCard("network", localized("Drift state", "偏离状态"), num(drift), drift ? localized("Confirmed design-vs-actual drift", "存在确认的设计 / 实现偏离") : localized("No confirmed blocking drift", "没有确认的阻塞偏离"), drift ? "warn" : "good", "architectureSection"),
    outcomeCard("chart", localized("Change acceptance", "变更验收"), confidence, localized("Canonical record; evidence counts do not establish readiness", "统一记录；证据数量不能决定就绪状态"), confidenceTone, "overviewSection", "acceptance"),
  ].join("");
  const acceptance = proof.acceptance || {}, coverage = Number(acceptance.total || 0) ? Math.round(Number(acceptance.fresh || 0) * 100 / Number(acceptance.total || 1)) : 0,
    activity = state.activitySnapshot?.activity || project.activity, evidenceAge = proof.latest_current_evidence_at_ms ? engineeringAge(Number(proof.latest_current_evidence_at_ms)) : "—";
  const signals = [["link", localized("Change chain", "变更链"), num(changes.length), localized("files", "文件")], ["sync", localized("Runtime signals", "运行时信号"), activity?.available === true ? `${num(activity.active || 0)} / ${num(activity.queued || 0)}` : "—", localized("active / queued", "执行 / 排队")], ["shield", localized("Verification health", "验证健康"), `${num(proof.current_passed || 0)} / ${num(proof.current_evidence || 0)}`, localized("pass / evidence", "通过 / 证据")], ["clock", localized("Evidence freshness", "证据新鲜度"), evidenceAge, localized("latest current proof", "最近当前证据")], ["chart", localized("Coverage", "覆盖趋势"), acceptance.total ? `${num(acceptance.fresh || 0)} / ${num(acceptance.total || 0)}` : "—", acceptance.total ? localized(`fresh acceptance coverage · ${coverage}%`, `新鲜验收覆盖 · ${coverage}%`) : localized("fresh acceptance coverage", "新鲜验收覆盖")]].map(([icon, label, value, detail]) => `<div class="impact-signal"><span>${uiIcon(icon)}</span><div><small>${esc(label)}</small><strong>${esc(value)}</strong><em>${esc(detail)}</em></div></div>`).join("");
  const html = `<div class="impact-legend"><span><i class="legend-dot primary"></i>${esc(localized("See impact", "看清影响"))}</span><span><i class="legend-dot good"></i>${esc(localized("Sufficient proof", "验证充分"))}</span><span><i class="legend-dot warn"></i>${esc(localized("Controlled risk", "风险可控"))}</span><span><i class="legend-dot info"></i>${esc(localized("Change acceptance", "变更验收"))}</span></div><div class="change-convergence-flow"><section class="impact-zone impact-input-zone"><header><strong>${esc(localized("Change inputs", "变更输入"))}</strong><small>${esc(localized("Build complete context from multiple change sources", "从多源变更构建完整上下文"))}</small></header>${inputHtml}</section><span class="impact-flow-arrow" aria-hidden="true"></span><section class="impact-zone impact-analysis-zone"><header><strong>${esc(localized("Impact propagation", "影响扩散"))}</strong><small>${esc(localized("Identify affected scope, risk and proof", "识别影响范围、风险与验证需求"))}</small></header><div class="impact-analysis-grid">${middleHtml}</div></section><span class="impact-flow-arrow" aria-hidden="true"></span><section class="impact-zone convergence-zone"><header><strong>${esc(localized("Convergence outcome", "收敛结果"))}</strong><small>${esc(localized("Judge confidence from proof and residual risk", "基于证据与剩余风险判断信心"))}</small></header>${outcomeHtml}</section></div><div class="impact-signal-strip"><div class="impact-signal-title"><strong>${esc(localized("Engineering signals", "工程信号"))}</strong><small>${esc(localized("Key indicators from change, runtime and verification", "来自代码、运行与验证的关键指标"))}</small></div>${signals}</div>`;
  setHtml("changeConvergenceMap", els.changeConvergenceMap, html, () => els.changeConvergenceMap.querySelectorAll("[data-convergence-target]").forEach(button => button.addEventListener("click", () => convergenceFocus(button.dataset.convergenceTarget, button.dataset.convergenceId))));
}
function renderChangeStory() {
  const p = state.project;
  if (!p) return;
  // The change counts below come from the working-tree review. When that review
  // is unavailable they are unknown, not zero, and must not be presented as an
  // all-clear next to an attention list that says the review is unavailable.
  const reviewAvailable = p.git_review?.available === true;
  const changes = p.changes || [], components = new Set(), requirements = new Set();
  changes.forEach(change => { (change.affected_components || []).forEach(id => components.add(id)); (change.affected_requirements || []).forEach(id => requirements.add(id)); });
  const impact = p.verification_impact, drift = Number(p.architecture?.blocking_drift_edges || 0), islands = impact?.affected_islands?.length || 0;
  const facts = [
    [localized("Changed files", "变更文件"), reviewAvailable ? changes.length : "—", reviewAvailable ? localized(`+${num(p.code?.additions || 0)} / -${num(p.code?.deletions || 0)}`, `+${num(p.code?.additions || 0)} / -${num(p.code?.deletions || 0)}`) : localized("review unavailable", "检查不可用")],
    [localized("Components", "组件"), reviewAvailable ? components.size : "—", reviewAvailable ? localized("mapped owners", "映射归属") : localized("unknown", "未知")],
    [localized("Requirements", "需求"), reviewAvailable ? requirements.size : "—", reviewAvailable ? localized("affected intent", "受影响意图") : localized("unknown", "未知")],
    [localized("Verification islands", "验证项目岛"), impact ? islands : "—", impact
      ? (impact.selective ? localized("selective", "选择性") : localized("broad fallback", "宽范围回退"))
      : localized("unknown", "未知")],
    [localized("Confirmed drift", "确认偏离"), drift, drift ? localized("needs attention", "需要处理") : localized("none", "无")],
  ];
  const note = reviewAvailable ? "" : `<div class="change-impact-unknown">${esc(localized("Working-tree review is unavailable, so the change counts above are unknown rather than zero.", "工作树检查不可用，以上变更计数未知，不能当作零。"))}</div>`;
  const html = facts.map(([label, value, detail]) => `<div class="change-impact-card"><span>${esc(label)}</span><strong>${Number.isFinite(value) ? num(value) : esc(value)}</strong><small>${esc(detail)}</small></div>`).join("");
  setHtml("changeStory", els.changeStory, `${note}<div class="change-impact-grid">${html}</div>`);
}
function renderRuntimeTopology() {
  if (!state.project || !els.runtimeTopology) return;
  const project = state.project, activity = state.activitySnapshot?.activity || project.activity, tunnel = state.tunnelSnapshot, tunnels = tunnel?.tunnels || [], active = tunnels.filter(item => item.role === "active"), pending = pendingCount(), precision = project.graph_precision || {}, proof = effectiveProof();
  const items = [
    [localized("Endpoint", "入口"), !tunnel ? localized("telemetry unavailable", "遥测不可用") : active.length ? active.map(item => `${item.provider} · ${item.state}`).join(" / ") : localized("local / pending", "本地 / 等待"), !tunnel ? "warn" : active.length ? (active.some(item => item.state === "verified") ? "good" : "warn") : "info"],
    [localized("OAuth & MCP", "OAuth 与 MCP"), pending == null ? localized("approval unknown", "授权未知") : localized(`${pending} pending`, `${pending} 待授权`), pending == null ? "info" : pending ? "warn" : "good"],
    [localized("Workspace guard", "工作区边界"), project.git_review?.available === true ? localized("enforced · Git visible", "已执行 · Git 可见") : localized("enforced · Git unavailable", "已执行 · Git 不可用"), project.git_review?.available === true ? "good" : "warn"],
    [localized("Harness", "Harness"), activity?.available === true ? localized(`${num(activity.active || 0)} active · ${num(activity.queued || 0)} queued`, `${num(activity.active || 0)} 执行中 · ${num(activity.queued || 0)} 排队`) : localized("telemetry unavailable", "遥测不可用"), Number(activity?.queued || 0) ? "warn" : "info"],
    [localized("Repository model", "仓库模型"), localized(`${statusLabel(precision.primary || "unknown")} precision`, `${statusLabel(precision.primary || "unknown")} 精度`), ["semantic", "runtime", "deterministic"].includes(precision.primary) ? "good" : "info"],
    [localized("Verification", "验证"), Number(proof.current_evidence || 0) ? localized(`${num(proof.current_evidence)} current evidence`, `${num(proof.current_evidence)} 条当前证据`) : localized("not verified", "尚未验证"), Number(proof.current_failed || 0) ? "bad" : Number(proof.current_evidence || 0) ? "good" : "warn"],
  ];
  const html = items.map(([title, value, tone]) => `<div class="runtime-status-card ${tone}"><span>${esc(title)}</span><strong>${esc(value)}</strong></div>`).join("");
  setHtml("runtimeTopology", els.runtimeTopology, `<div class="runtime-status-grid">${html}</div>`);
}
function renderArchitecture() {
  renderEngineeringFlow();
  renderChangeStory();
  renderEngineeringTimeline();
  renderRuntimeTopology();
  const view = ["blueprint", "components", "graph", "codegraph", "consistency"].includes(state.architectureView) ? state.architectureView : "blueprint";
  state.architectureView = view;
  const blueprint = view === "blueprint", components = view === "components", graph = view === "graph", codegraph = view === "codegraph", consistency = view === "consistency";
  if (!blueprint && state.systemMapFull) setSystemMapFull(false);
  if (!codegraph && state.codeGraphFull) setCodeGraphFull(false);
  document.querySelectorAll("[data-architecture-view]").forEach(button => {
    const active = button.dataset.architectureView === view;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  els.architectureLayout?.classList.toggle("hidden", codegraph || consistency);
  els.architectureDrilldown?.classList.toggle("hidden", consistency);
  els.codeGraphSection?.classList.toggle("hidden", !codegraph);
  document.querySelector(".system-map-controls")?.classList.toggle("hidden", !blueprint);
  els.architectureBlueprint.classList.toggle("hidden", !blueprint);
  els.componentToolbar.classList.toggle("hidden", !components);
  els.componentCards.classList.toggle("hidden", !components);
  els.architectureGraph.closest(".architecture-graph-shell")?.classList.toggle("hidden", !graph);
  if (codegraph) maybeLoadCodeGraph();
  else if (consistency) renderConsistencyMatrix();
  else if (blueprint) renderArchitectureBlueprint();
  else if (graph) renderArchitectureGraph();
  else renderComponentCards();
  if (!codegraph) renderArchitectureInspector();
}
