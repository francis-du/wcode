function renderWorkspaceOptions() {
  const options = (state.project?.workspace_options || []).map((workspace) =>
    `<option value="${esc(workspace.id)}" ${workspace.id === state.current ? "selected" : ""}>${esc(workspace.id)}</option>`
  ).join("");
  setHtml("workspaceOptions", els.workspace, options);
}
function renderSharedChrome() {
  renderWorkspaceOptions();
  renderLive(); renderStats(); renderAttention(); renderProjectNavigator();
}
function renderTabPanels(tab) {
  switch (tab) {
    case "architecture":
      renderArchitecture(); renderTraceabilityMap(); renderChangeConvergenceMap();
      // Code graph paint is owned by renderArchitecture → maybeLoadCodeGraph
      // so we do not rebuild the SVG tree on every architecture poll tick.
      break;
    case "overview":
      renderFitnessObservatory();
      renderCodeStats(); renderRevisions(); renderLanguageQuality();
      renderExecutionStatus(); renderAdaptiveVerification(); renderVerifiedLearning();
      break;
    case "activity":
      renderActivity(); renderExecutionStatus();
      break;
    case "proof":
      renderProofSummary(); renderAdaptiveVerification(); renderVerifiedLearning();
      break;
    case "requirements":
      renderRequirements(); renderDetail(); renderVerificationImpact();
      break;
    case "changes":
      renderChanges(); renderVerificationImpact();
      break;
    case "files":
      renderProjectStructure();
      break;
    default:
      break;
  }
}
function renderProject(force = false) {
  if (!state.project) return;
  if (force) state.rendered.clear();
  renderSharedChrome();
  // Full force still paints every surface so hidden tabs are warm when opened.
  // Incremental refreshes only repaint the active tab plus shared chrome.
  if (force) {
    renderArchitecture(); renderTraceabilityMap(); renderChangeConvergenceMap();
    renderRequirements(); renderDetail(); renderVerificationImpact(); renderChanges();
    renderProjectStructure();
    // Code graph is mounted lazily via maybeLoadCodeGraph when the operator
    // is on the codegraph architecture surface — not on every force refresh.
    renderFitnessObservatory();
    renderCodeStats(); renderRevisions(); renderLanguageQuality();
    renderExecutionStatus(); renderActivity(); renderProofSummary();
    renderAdaptiveVerification(); renderVerifiedLearning();
    return;
  }
  renderTabPanels(state.workspaceTab);
}
function worklistOwnershipHtml(worklist) {
  const historical = worklist?.available === false && worklist.last_known?.available === true;
  if (historical) worklist = worklist.last_known;
  const heading = esc(localized("Model task ownership", "模型任务所有权"));
  const unknown = `<section class="execution-workers execution-empty warn"><strong>${heading}</strong><span>${esc(localized("Ownership state is unknown; refresh the observed Worklist.", "任务所有权未知；请刷新观测到的 Worklist。"))}</span></section>`;
  if (!worklist || worklist.available === false || typeof worklist.exists !== "boolean"
    || (worklist.exists && !Array.isArray(worklist.items))) return unknown;
  if (historical && !worklist.exists) return unknown;
  if (!worklist.exists) return `<section class="execution-workers execution-empty"><strong>${heading}</strong><span>${esc(localized("No durable Worklist has been created.", "尚未创建持久化 Worklist。"))}</span></section>`;
  const items = worklist.items.filter(item => item && typeof item === "object" && !Array.isArray(item));
  if (items.length !== worklist.items.length) return unknown;
  const text = (value, fallback, limit = 300) => esc(typeof value === "string" ? value.slice(0, limit) : fallback);
  const ordered = items.slice().sort((a, b) => {
    const rank = item => item.claim && item.claim.expired === false ? 0 : item.claim ? 1 : item.result ? 2 : 3;
    return rank(a) - rank(b);
  });
  const rows = ordered.slice(0, 8).map(item => {
    const claim = item.claim && typeof item.claim === "object" ? item.claim : null;
    const result = item.result && typeof item.result === "object" ? item.result : null;
    const paths = Array.isArray(item.write_paths) ? item.write_paths.filter(path => typeof path === "string") : [];
    const validClaim = claim && typeof claim.actor === "string" && typeof claim.expired === "boolean"
      && Number.isFinite(claim.expires_at_ms) && claim.expires_at_ms > 0 && claim.expires_at_ms <= 8640000000000000;
    const expired = validClaim && (claim.expired || claim.expires_at_ms <= Date.now());
    const ownership = historical ? localized("Historical ownership", "历史所有权")
      : !claim ? localized("Unclaimed", "未领取")
      : !validClaim ? localized("Ownership unknown", "所有权未知")
      : expired ? localized("Lease expired", "租约已过期") : localized("Claimed", "已领取");
    const lease = validClaim ? time(claim.expires_at_ms) : "—";
    const scope = paths.length ? paths.slice(0, 4).map(path => text(path, "", 300)).join(" · ")
      : esc(localized("Read-only lane", "只读通道"));
    const outcome = {
      complete: localized("Worker reports complete", "Worker 报告已完成"),
      blocked: localized("Worker reports blocked", "Worker 报告阻塞"),
      incomplete: localized("Worker reports incomplete", "Worker 报告未完成"),
    }[result?.outcome] || localized("Worker result unknown", "Worker 结果未知");
    const evidenceCount = Array.isArray(result?.evidence) ? result.evidence.length : null;
    return `<article class="execution-worker"><div class="execution-worker-head"><strong>${text(item.title, item.id || localized("Untitled task", "未命名任务"))}</strong>${pill(ownership, historical || (claim && (!validClaim || expired)) ? "warn" : "info")}</div>
      <div class="execution-worker-facts"><span>${esc(localized("Actor", "执行者"))}<b>${claim ? text(claim.actor, "—", 128) : "—"}</b></span><span>${esc(localized("Lease expires", "租约到期"))}<b>${esc(lease)}</b></span><span>${esc(localized("Write scopes", "写入范围"))}<b>${scope}${paths.length > 4 ? ` · +${num(paths.length - 4)}` : ""}</b></span></div>
      ${result ? `<div class="execution-worker-result">${pill(outcome, "warn")}<span>${text(result.summary, localized("No bounded worker summary", "没有有界 Worker 摘要"), 500)}</span><small>${esc(localized("Reported by", "报告者"))} ${text(result.actor, "—", 128)} · ${evidenceCount == null ? esc(localized("Evidence references unknown", "Evidence 引用未知")) : `${num(evidenceCount)} ${esc(localized("Evidence references", "Evidence 引用"))}`}</small></div>` : ""}</article>`;
  }).join("");
  const bounded = worklist.truncated === true || items.length > 8;
  return `<section class="execution-workers execution-blockers"><div class="execution-worker-head"><span class="execution-label">${heading}</span><span class="panel-meta">${historical ? esc(localized("Last observed", "最后观测")) + " " : ""}Worklist #${num(worklist.revision || 0)}</span></div>
    <p class="panel-meta">${esc(localized("Worker reports require independent verification; they do not certify proof.", "Worker 报告需要独立验证；不会认证证明。"))}</p>
    ${historical ? `<p class="warning-inline">${esc(localized("Ownership state is unknown. These are historical observations; refresh before acting on claims.", "当前任务所有权未知。以下为历史观测；请刷新后再依赖领取信息。"))}</p>` : ""}
    ${state.snapshotStale && !historical ? `<p class="warning-inline">${esc(localized("Snapshot stale; refresh task ownership.", "快照已过期；请刷新任务所有权。"))}</p>` : ""}
    <div class="execution-worker-list">${rows || `<span class="panel-meta">${esc(localized("No items in the observed Worklist.", "观测到的 Worklist 没有任务。"))}</span>`}</div>
    ${bounded ? `<p class="panel-meta">${esc(localized("Bounded ownership view", "有界任务所有权视图"))} · ${num(Math.min(items.length, 8))} / ${num(items.length)} ${esc(localized("observed items", "已观测任务"))}</p>` : ""}</section>`;
}
function renderExecutionStatus() {
  const execution = state.project?.execution, ownershipHtml = worklistOwnershipHtml(state.project?.execution?.worklist);
  if (!execution || execution.available === false) {
    return setHtml("executionStatus", els.executionStatus, `<div class="execution-empty warn"><strong>${esc(localized("Execution state unavailable", "执行状态不可用"))}</strong><span>${esc(localized("The Observatory could not read the durable checkpoint; this is not an idle signal.", "观测台无法读取持久化检查点；这不代表当前没有任务。"))}</span></div>${ownershipHtml}`);
  }
  if (!execution.exists) {
    return setHtml("executionStatus", els.executionStatus, `<div class="execution-empty"><strong>${esc(localized("No durable Execution yet", "暂无持久化 Execution"))}</strong><span>${esc(localized("No Execution checkpoint has been created for this Workspace.", "当前工作区尚未创建 Execution 检查点。"))}</span></div>${ownershipHtml}`);
  }
  const checkpoint = execution.checkpoint || {}, phase = String(execution.phase || "executing"),
    tone = phase === "completed" ? "good" : phase === "blocked" ? "bad" : phase === "verifying" ? "warn" : "info",
    phaseLabel = {
      executing: localized("Executing", "执行中"), blocked: localized("Blocked", "阻塞"),
      verifying: localized("Verifying", "验证中"), completed: localized("Completed", "已完成"),
    }[phase] || phase;
  const runnable = (checkpoint.runnable || []).slice(0, 8), blockers = (checkpoint.blockers || []).slice(0, 12),
    repository = checkpoint.repository_revision || {}, codeRevision = repository.code ? String(repository.code).slice(0, 12) : "—",
    designRevision = repository.design ? String(repository.design).slice(0, 12) : "—";
  const laneHtml = runnable.length
    ? runnable.map(item => `<span class="execution-chip">${esc(item)}</span>`).join("")
    : `<span class="panel-meta">${esc(localized("No runnable lanes", "暂无可运行执行通道"))}</span>`;
  const blockerHtml = blockers.length
    ? blockers.map(item => `<span class="execution-blocker">${esc(item)}</span>`).join("")
    : `<span class="panel-meta">${esc(localized("No explicit blockers", "没有显式阻塞项"))}</span>`;
  const directive = execution.pending_directive && typeof execution.pending_directive === "object" ? execution.pending_directive : null,
    lineage = execution.lineage && typeof execution.lineage === "object" ? execution.lineage : null,
    verificationFloor = execution.verification_floor ? String(execution.verification_floor) : "—",
    steeringTone = execution.replan_required === true ? "bad" : directive ? "warn" : "info",
    steeringLabel = execution.replan_required === true
      ? localized("Replan required", "需要重新规划")
      : directive ? localized("Pending steering", "待应用 steering") : localized("Execution guard", "执行约束");
  const steeringHtml = directive || lineage || execution.verification_floor
    ? `<section class="execution-steering-card"><div class="execution-steering-head"><span class="execution-label">${esc(localized("Steering / handoff", "Steering / 交接"))}</span>${pill(steeringLabel, steeringTone)}</div>${directive ? `<strong class="execution-steering-summary">${esc(directive.summary || localized("Pending structured directive", "待应用结构化指令"))}</strong>` : ""}<div class="execution-steering-facts"><span><i>${esc(localized("Directive", "指令"))}</i><b>${esc(directive ? String(directive.kind || "steering").replace(/_/g, " ") : "—")}</b></span><span><i>${esc(localized("Verification floor", "验证下限"))}</i><b>${esc(verificationFloor)}</b></span><span><i>${esc(localized("Bound plan", "绑定计划"))}</i><b>${esc(directive?.reconciliation_plan_id || "—")}</b></span><span><i>${esc(localized("Handoff lineage", "交接血缘"))}</i><b>${esc(lineage ? `${lineage.parent_execution_id || "—"} · #${num(lineage.handoff_count || 0)}` : "—")}</b></span></div></section>`
    : "";
  const html = `<div class="execution-shell ${tone}">
    <div class="execution-primary"><div class="execution-heading"><div><span class="execution-id">${esc(execution.execution_id || "Execution")}</span><h3>${esc(execution.objective || localized("Untitled execution", "未命名执行"))}</h3></div>${pill(phaseLabel, tone)}</div><div class="execution-revision">${esc(localized("Execution revision", "Execution 版本"))} ${num(execution.revision || 0)} · ${esc(localized("Worklist revision", "Worklist 版本"))} ${num(checkpoint.worklist_revision || 0)}</div></div>
    <div class="execution-metrics"><div><span>${esc(localized("Done", "已完成"))}</span><strong>${num(checkpoint.done_items || 0)}</strong></div><div><span>${esc(localized("Open", "未完成"))}</span><strong>${num(checkpoint.open_items || 0)}</strong></div><div><span>${esc(localized("Blocked", "阻塞"))}</span><strong>${num(checkpoint.blocked_items || 0)}</strong></div><div><span>${esc(localized("Runnable", "可运行"))}</span><strong>${num(runnable.length)}</strong></div></div>
    <div class="execution-grid"><section><span class="execution-label">${esc(localized("Runnable lanes", "可运行执行通道"))}</span><div class="execution-chip-list">${laneHtml}</div></section><section><span class="execution-label">${esc(localized("Bound convergence / proof", "绑定的收敛 / 证明"))}</span><div class="execution-facts"><span>${esc(localized("Reconciliation", "收敛计划"))}<b>${esc(checkpoint.reconciliation_plan_id || "—")}</b><i>${checkpoint.reconciliation_converged === true ? esc(localized("converged", "已收敛")) : checkpoint.reconciliation_converged === false ? esc(localized("pending", "未收敛")) : "—"}</i></span><span>${esc(localized("Verification", "验证计划"))}<b>${esc(checkpoint.verification_plan_id || "—")}</b><i>${checkpoint.verification_ready === true ? esc(localized("ready", "已就绪")) : checkpoint.verification_ready === false ? esc(localized("pending", "未就绪")) : "—"}</i></span><span>${esc(localized("Code / Design revision", "代码 / 设计版本"))}<b>${esc(codeRevision)} / ${esc(designRevision)}</b></span></div></section></div>
    ${steeringHtml}
    ${ownershipHtml}
    <section class="execution-blockers"><span class="execution-label">${esc(localized("Blockers", "阻塞项"))}</span><div>${blockerHtml}</div></section>
  </div>`;
  setHtml("executionStatus", els.executionStatus, html);
}
const workspaceTabForSection = {
  architectureSection: "architecture",
  codeGraphSection: "architecture",
  overviewSection: "overview",
  activitySection: "activity",
  proofSection: "proof",
  engineeringSection: "overview",
  requirementsSection: "requirements",
  changesSection: "changes",
  filesSection: "files",
  diagnosticsSection: "overview",
  fitnessSection: "overview",
  qualitySection: "overview",
};
function renderWorkspaceHero(tab) {
  const copy = {
    overview: [localized("ENGINEERING OBSERVATORY", "工程观测台"), localized("System overview", "系统总览"), localized("Current project health, engineering signals, diagnostics and language quality in one operational summary.", "把当前项目健康度、工程信号、诊断与语言质量汇总在一个运行视图中。")],
    architecture: [localized("SYSTEM ARCHITECTURE", "系统架构"), localized("Engineering architecture", "工程架构"), localized("Read the system from semantic architecture down to the bounded code graph, callers, dependencies and proof context.", "从语义架构深入到有界代码图谱、调用关系、依赖与证明上下文。")],
    activity: [localized("LIVE WORK", "实时工作"), localized("Task activity", "任务活动"), localized("Running work, queue pressure and execution time without mixing waiting time into runtime.", "区分排队与执行时间，查看实时工作与资源压力。")],
    proof: [localized("REVISION-BOUND PROOF", "版本绑定证据"), localized("Verification evidence", "验证证据"), localized("Current-revision evidence, verification readiness and adaptive checks kept separate from historical passes.", "当前版本证据、验证就绪度与自适应检查，与历史通过记录分开呈现。")],
    requirements: [localized("DESIRED STATE", "目标状态"), localized("Requirements", "需求"), localized("Trace intent through implementation, acceptance evidence and convergence.", "把需求意图追踪到实现、验收证据与收敛状态。")],
    changes: [localized("WORKING TREE", "工作树"), localized("Current changes", "当前变更"), localized("Map current edits back to components, requirements and verification impact.", "把当前修改映射回组件、需求与验证影响。")],
    files: [localized("REPOSITORY", "仓库"), localized("Project files", "项目文件"), localized("Browse the bounded repository structure and the files that dominate maintenance cost.", "浏览有界仓库结构以及维护成本最高的文件。")],
  }[tab] || [];
  if (els.workspaceKicker) els.workspaceKicker.textContent = copy[0] || "";
  if (els.workspaceTitle) els.workspaceTitle.textContent = copy[1] || "";
  if (els.workspaceSubtitle) els.workspaceSubtitle.textContent = copy[2] || "";
}
function activateWorkspaceTab(tab, { scroll = false } = {}) {
  const valid = ["overview", "architecture", "activity", "proof", "changes", "requirements", "files"];
  const next = valid.includes(tab) ? tab : "architecture";
  state.workspaceTab = next;
  renderWorkspaceHero(next);
  let activeTabButton = null;
  document.querySelectorAll("[data-workspace-tab]").forEach(button => {
    const active = button.dataset.workspaceTab === next;
    button.setAttribute("aria-selected", String(active));
    button.tabIndex = active ? 0 : -1;
    if (active) activeTabButton = button;
  });
  activeTabButton?.scrollIntoView({ block: "nearest", inline: "nearest" });
  document.querySelectorAll("[data-workspace-panel]").forEach(panel => {
    const active = panel.dataset.workspacePanel === next;
    panel.classList.toggle("hidden", !active);
  });
  document.querySelector(".drawer-stack")?.classList.toggle("hidden", next === "architecture");
  if (next !== "architecture" && els.componentInspector?.classList.contains("open")) {
    els.componentInspector.classList.remove("open");
  } else if (next === "architecture" && state.architectureView !== "codegraph" && !state.systemMapFull) {
    const hasSelection = state.architectureView === "blueprint" ? state.selectedSubsystem : state.selectedComponent;
    if (hasSelection) els.componentInspector?.classList.add("open");
  }
  // Paint the newly visible surface from the latest snapshot without forcing a
  // full multi-panel cascade across every hidden tab.
  if (state.project) renderTabPanels(next);
  if (scroll) document.querySelector(".observatory-main")?.scrollIntoView({ behavior: "smooth", block: "start" });
}
function revealSection(id) {
  const section = document.getElementById(id);
  if (!section) return;
  if (id === "codeGraphSection") state.architectureView = "codegraph";
  activateWorkspaceTab(workspaceTabForSection[id] || state.workspaceTab);
  section.scrollIntoView({ behavior: "smooth", block: "start" });
  section.focus({ preventScroll: true });
}
function cacheWorkspaceSnapshot() {
  if (!state.current || !state.project) return;
  // Map.set() does not refresh insertion order for an existing key.
  // Touch the entry explicitly so the eight-workspace cache is true LRU.
  state.projectCache.delete(state.current);
  state.projectCache.set(state.current, {
    project: state.project,
    revisionKey: state.revisionKey,
    lastUpdated: state.lastUpdated,
    lastChecked: state.lastChecked,
    activitySnapshot: state.activitySnapshot,
    activityUpdated: state.activityUpdated,
  });
  while (state.projectCache.size > 8) state.projectCache.delete(state.projectCache.keys().next().value);
}
function restoreWorkspaceSnapshot(workspace) {
  if (typeof clearChangeInspection === "function") clearChangeInspection();
  const cached = state.projectCache.get(workspace);
  if (!cached) return false;
  state.project = cached.project;
  state.fitnessSnapshotFromCache = true;
  state.revisionKey = cached.revisionKey;
  state.lastUpdated = cached.lastUpdated;
  state.lastChecked = cached.lastChecked;
  state.activitySnapshot = cached.activitySnapshot;
  state.activityUpdated = cached.activityUpdated;
  state.syncError = false; state.syncFailure = null;
  try {
    renderProject(true);
  } catch (error) {
    // Cache restoration runs before the request try/finally. A bad cached
    // render must not reject the switch or prevent an authoritative retry.
    state.projectCache.delete(workspace);
    state.project = null; state.revisionKey = null;
    state.lastUpdated = 0; state.lastChecked = 0; state.rendered.clear();
    showRefreshFailure(error, "render");
    return false;
  }
  state.projectCache.delete(workspace);
  state.projectCache.set(workspace, cached);
  setSync("loading", localized("Cached snapshot · refreshing…", "已显示缓存 · 后台刷新…"));
  return true;
}
function refreshFailureCopy(failure = state.syncFailure) {
  const { code, status, phase } = failure || {};
  let title = t("Refresh failed"), detail = localized("Check the connection, then use Refresh to try again.", "请检查连接，然后点击刷新重试。");
  if (code === "authorization_required" || status === 401) {
    title = localized("UI authorization required", "需要重新授权访问");
    detail = localized("Open a new Observatory page from the wcode terminal (W) to authorize access. A link from an earlier runtime may no longer be valid.", "请从 wcode 终端按 W 重新打开观测台并授权访问。旧运行实例生成的链接可能已失效。");
  } else if (status === 403) {
    title = localized("Access denied", "访问被拒绝");
    detail = localized("Open the current Observatory link from the wcode terminal (W). Check the trusted host and origin; reusing an old tunnel address may be rejected.", "请从 wcode 终端按 W 打开当前观测台链接，并检查可信主机与来源。旧隧道地址可能被拒绝。");
  } else if (phase === "render") {
    title = localized("Page rendering failed", "页面渲染失败");
    detail = localized("The snapshot was received, but the page could not display it. Reload this page; this is not a network failure.", "已收到项目快照，但页面未能正确显示。请重新加载页面；这不是网络请求失败。");
  } else if (code === "timeout" || status === 408 || status === 504) {
    title = localized("Refresh timed out", "刷新请求超时");
    detail = localized("The snapshot request exceeded its deadline. Displayed data is not confirmed current; use Refresh to try again.", "快照请求超过等待上限。当前显示的数据尚未确认更新，请点击刷新重试。");
  } else if (code === "invalid_response" || phase === "response") {
    title = localized("Invalid response", "接口响应异常");
    detail = localized("The response is not a valid snapshot for this workspace. Reload the current wcode Observatory page and check the server or proxy.", "响应不是当前工作区的有效快照。请重新加载当前 wcode 观测台，并检查服务或代理。");
  } else if (status === 400) {
    title = localized("Workspace unavailable", "工作区不可用");
    detail = localized("The selected workspace could not be resolved. Reopen the Observatory from the wcode terminal and select an available workspace.", "无法解析所选工作区。请从 wcode 终端重新打开观测台，选择可用工作区。");
  } else if (status === 404) {
    title = localized("Endpoint unavailable", "接口不可用");
    detail = localized("Check that this page and its API belong to the same wcode runtime. Reopen the current Observatory link from the terminal.", "请检查页面与 API 是否属于同一个 wcode 运行实例，并从终端重新打开当前观测台链接。");
  } else if (status === 429) {
    title = localized("Server busy", "服务繁忙");
    detail = localized("The server is limiting requests. Avoid repeated clicks; use Refresh after the current work settles.", "服务正在限制请求。请避免连续点击，待当前任务缓解后再刷新。");
  } else if (status >= 500) {
    title = localized("Server error", "服务端错误");
    detail = localized("The server could not return the project state. Check its diagnostics, then use Refresh to try again.", "服务端未能返回项目状态。请检查服务诊断信息，然后点击刷新重试。");
  } else if (code === "network") {
    title = localized("Connection failed", "连接失败");
  }
  return { title: status ? `${title} · HTTP ${status}` : title, detail };
}
function showRefreshFailure(error, phase = "request") {
  if (typeof invalidateChangeInspection === "function") invalidateChangeInspection();
  state.syncError = true;
  state.syncFailure = {
    code: ["authorization_required", "invalid_response", "timeout", "network"].includes(error?.code) ? error.code : "",
    status: Number.isInteger(error?.status) && error.status >= 100 && error.status <= 599 ? error.status : null,
    phase,
  };
  const { title, detail } = refreshFailureCopy();
  setSync("error", title);
  els.syncState.title = detail;
  els.syncState.parentElement?.setAttribute("aria-label", `${title}. ${detail}`);
  // Error reporting must not invoke the same failing renderer unguarded.
  for (const render of [renderAttention, renderLive, renderFitnessObservatory]) {
    try { render(); } catch (renderError) { console.warn("wcode: refresh error view failed", renderError); }
  }
  if (!state.project || phase === "render") renderProjectPlaceholder(true);
}
function renderProjectPlaceholder(failed = false) {
  setHtml("observationCoverage", els.observationCoverage, "");
  if (typeof clearChangeInspection === "function") clearChangeInspection();
  const failure = refreshFailureCopy();
  const title = failed ? failure.title : t("Loading project state…");
  const detail = failure.detail;
  const content = failed
    ? `<div class="section empty connection-state"><div class="connection-state-icon" aria-hidden="true">!</div><div class="connection-state-copy"><span class="eyebrow">${esc(localized("LIVE CONNECTION", "实时连接"))}</span><strong>${esc(title)}</strong><p>${esc(detail)}</p><div class="connection-state-meta"><span>${esc(localized("No current project snapshot is confirmed.", "当前没有已确认的项目快照。"))}</span><span>${esc(localized("Workspace-scoped access remains unchanged.", "工作区级访问权限保持不变。"))}</span></div><div class="connection-state-actions"><button type="button" class="button-primary" data-summary-action="refresh">${esc(localized("Retry refresh", "重试刷新"))}</button><button type="button" class="quiet-action" data-summary-action="access">${esc(t("Manage access"))}</button></div></div></div>`
    : `<div class="section empty loading-state"><div class="loading-orb" aria-hidden="true"></div><div><span class="eyebrow">${esc(localized("PROJECT SNAPSHOT", "项目快照"))}</span><strong>${esc(title)}</strong><p>${esc(localized("Reading bounded repository evidence and runtime signals…", "正在读取有界仓库证据和运行时信号…"))}</p></div></div>`;
  for (const key of ["stats", "attention", "architectureBlueprint", "engineeringFlow", "changeStory", "runtimeTopology", "engineeringTimeline", "traceabilityMap", "changeConvergenceMap", "architectureGraph", "componentCards", "componentInspector", "requirements", "detail", "verificationImpact", "changes", "fileTree", "largeFiles", "codeStats", "revisions", "fitnessBenchmark", "fitnessObservatory", "languageQuality", "executionStatus", "activity", "resourceStatus", "proofSummary", "adaptiveVerification", "verifiedLearning"]) {
    if ((key === "activity" || key === "resourceStatus") && state.activitySnapshot) continue;
    setHtml(key, els[key], content, () => { if (failed) bindSummaryActions(els[key]); });
  }
  setHtml("statusSummary", els.statusSummary, failed ? content : `<div class="loading-state summary-loading"><div class="loading-orb" aria-hidden="true"></div><div><span class="eyebrow">${esc(localized("PROJECT PULSE", "项目状态"))}</span><h2>${esc(title)}</h2><p>${esc(localized("Preparing the first bounded snapshot…", "正在准备第一份有界项目快照…"))}</p></div></div>`, () => { if (typeof bindSummaryActions === "function") bindSummaryActions(els.statusSummary); });
}
function clearWorkspaceView({ preserveDom = false } = {}) {
  state.workspaceEpoch++;
  setHtml("observationCoverage", els.observationCoverage, "");
  if (typeof clearChangeInspection === "function") clearChangeInspection();
  state.pendingValue = null; state.pendingApplied = 0;
  state.accessRead = null;
  state.syncError = false; state.syncFailure = null;
  state.fitnessSnapshotFromCache = false;
  state.project = null; state.selected = ""; state.selectedComponent = ""; state.selectedSubsystem = ""; state.selectedEvidenceKey = ""; state.evidenceInspectorOpen = true;
  state.codeGraphController?.abort(); state.codeGraphController = null;
  state.codeGraphSearchController?.abort(); state.codeGraphSearchController = null;
  state.codeGraphSourceController?.abort(); state.codeGraphSourceController = null;
  state.codeGraphSource = null; state.codeGraphSourceKey = ""; state.codeGraphSourceStartLine = null; state.codeGraphSourceRepositoryRevision = null; state.codeGraphSourceLoading = false; state.codeGraphSourceError = "";
  clearTimeout(state.codeGraphSearchTimer); state.codeGraphSearchTimer = null;
  state.codeGraph = null; state.codeGraphOverview = null; state.codeGraphWorkspace = ""; state.codeGraphQuery = ""; state.codeGraphSnapshot = "";
  state.codeGraphView = "overview"; state.codeGraphSearchResults = [];
  state.codeGraphLoading = false; state.codeGraphError = ""; state.selectedCodeNode = "";
  state.codeGraphPaintKey = "";
  state.systemMapScale = 1; state.systemMapFit = true; state.systemMapFull = false;
  state.revisionKey = null; state.lastUpdated = 0; state.lastChecked = 0;
  state.activitySnapshot = null; state.activityUpdated = 0; state.activityError = false;
  if (els.projectNavigator) els.projectNavigator.value = "";
  if (els.navigatorResults) els.navigatorResults.classList.add("hidden");
  cancelTunnelRefresh(); state.tunnelSnapshot = null;
  setHtml("tunnels", els.tunnels, "");
  state.access = null; state.workspaceAccess = null; state.authorizations = [];
  state.accessLoaded = false; state.accessEpoch++; state.semanticRefreshPending = false;
  els.search.value = ""; els.componentSearch.value = "";
  if (els.fileSearch) els.fileSearch.value = "";
  if (els.codeGraphSearch) els.codeGraphSearch.value = "";
  if (els.fileSearchStatus) els.fileSearchStatus.textContent = "";
  state.activityController?.abort(); state.pollController?.abort();
  if (!preserveDom) {
    renderProjectPlaceholder();
    els.lastUpdated.textContent = "—"; els.precisionBadge.textContent = "—";
    if (els.precisionProviders) els.precisionProviders.textContent = "—";
    els.reqCount.textContent = "—"; els.componentCount.textContent = "—";
    els.structureSummary.textContent = "—"; els.qualitySummary.textContent = "—";
  }
  renderAccess(true);
}
const worklistRevisionSuffix = (signal) => signal?.available === true && typeof signal.exists === "boolean"
  && Number.isSafeInteger(signal.revision) && signal.revision >= 0
  ? `|worklist:${signal.exists ? `1:${signal.revision}` : "0:0"}` : "|worklist:unknown";
const revisionKey = (revision) => `${revision.fingerprint || "full"}|${revision.graph_signal || revision.graph_revision || ""}|${revision.proof_revision || ""}|${revision.engineering_revision || ""}${Object.prototype.hasOwnProperty.call(revision, "worklist_revision") ? worklistRevisionSuffix(revision.worklist_revision) : ""}`;
async function refreshProject({ workspace, reason = "auto", force = false, revision, preferCached = false } = {}) {
  if (workspace !== undefined && workspace !== state.current) {
    cacheWorkspaceSnapshot();
    const hasCachedSnapshot = state.projectCache.has(workspace);
    state.current = workspace;
    clearWorkspaceView({ preserveDom: hasCachedSnapshot });
    const restored = restoreWorkspaceSnapshot(workspace);
    preferCached = true;
    force = force && !restored;
  }
  if (state.inFlight && reason === "auto") return false;
  state.controller?.abort();
  // Reserve the epoch before the first await, including the revision request.
  const controller = new AbortController(), epoch = ++state.requestEpoch, selectedWorkspace = state.current;
  state.controller = controller; state.inFlight = true;
  const stamp = observationStamp();
  const current = () => epoch === state.requestEpoch && observationCurrent(stamp);
  const foregroundSync = reason === "manual" || reason === "initial";
  const continuationReason = foregroundSync ? reason : "background";
  const previousSnapshot = { project: state.project, revisionKey: state.revisionKey, lastUpdated: state.lastUpdated, lastChecked: state.lastChecked };
  let phase = "request";
  // Only acknowledge a revision observed before this project request. A
  // parallel/late signal can describe edits that this snapshot never included.
  const observedKey = revision ? revisionKey(revision) : state.revisionKey;
  const options = {
    workspace: selectedWorkspace,
    signal: controller.signal,
    timeout: preferCached ? 15000 : 120000,
    headers: preferCached ? {
      "X-Wcode-Prefer-Cached": "1",
      "X-Wcode-Background-Refresh": "1",
    } : undefined,
  };
  const scheduleSnapshotProbe = () => {
    setTimeout(() => {
      if (current() && !controller.signal.aborted && !document.hidden &&
          (state.autoRefresh || continuationReason !== "background")) {
        void refreshProject({
          workspace: stamp.workspace,
          reason: "background",
          revision,
          preferCached: true,
        });
      }
    }, 900);
  };
  if (foregroundSync) setSync("loading", t("Refreshing project state…"));
  try {
    const data = await uiJson("/intelligence/project", "GET", undefined, options);
    if (!current() || controller.signal.aborted) return false;
    phase = "response";
    if (typeof data.workspace !== "string" || (selectedWorkspace && data.workspace !== selectedWorkspace)) throw new Error("Workspace response mismatch");
    // Resolve an omitted workspace once, retaining this request's generation.
    // Deferred refreshes must use the server-selected project, not an empty id.
    if (!selectedWorkspace) {
      state.current = data.workspace;
      stamp.workspace = data.workspace;
    }
    observePending(data.pending_authorizations, stamp);
    if (data.snapshot_pending === true) {
      if (foregroundSync) setSync("loading", localized("Building project snapshot in background…", "正在后台构建项目快照…"));
      scheduleSnapshotProbe();
      return true;
    }
    data.pending_authorizations = state.pendingValue;
    if (data.execution?.worklist?.available === false && state.project?.workspace === data.workspace) {
      const previous = state.project.execution?.worklist;
      const lastKnown = previous?.available === true ? previous : previous?.last_known;
      if (lastKnown?.available === true && typeof lastKnown.exists === "boolean") {
        data.execution.worklist = { ...data.execution.worklist, last_known: lastKnown };
      }
    }
    const cachedResponse = typeof data.snapshot_cache === "string";
    const snapshotRevision = typeof data.snapshot_revision === "string"
      ? data.snapshot_revision
      : null;
    const snapshotRefreshing = data.snapshot_refreshing === true;
    phase = "render";
    if (typeof invalidateChangeInspection === "function") invalidateChangeInspection();
    state.project = data; state.current = data.workspace;
    state.fitnessSnapshotFromCache = false;
    if (state.activitySnapshot && state.activitySnapshot.workspace !== data.workspace) state.activitySnapshot = null;
    state.lastUpdated = Date.now(); state.lastChecked = state.lastUpdated; state.syncError = false; state.syncFailure = null;
    state.revisionKey = snapshotRevision || (!cachedResponse ? observedKey : null);
    if (state.selected && !data.requirements?.some(r => r.id === state.selected)) state.selected = "";
    renderProject(force);
    cacheWorkspaceSnapshot();
    if (snapshotRefreshing || (cachedResponse && !snapshotRevision)) {
      if (foregroundSync) setSync("loading", localized("Cached snapshot · refreshing…", "已显示缓存 · 后台刷新…"));
      scheduleSnapshotProbe();
    } else {
      setSync("ok", localized("Snapshot up to date", "快照已更新"));
    }
    return true;
  } catch (error) {
    if (current() && !controller.signal.aborted) {
      if (phase === "render") {
        // A rendering exception cannot certify or cache a newer revision.
        Object.assign(state, previousSnapshot);
        state.rendered.clear();
      }
      console.warn("wcode: project refresh failed", error);
      showRefreshFailure(error, phase);
    }
    return false;
  } finally {
    if (state.controller === controller) { state.inFlight = false; state.controller = null; }
  }
}
async function pollRevision() {
  if (state.pollController || state.inFlight) return;
  const controller = new AbortController(), workspace = state.current, epoch = state.requestEpoch;
  const stamp = observationStamp();
  state.pollController = controller;
  try {
    const revision = await uiJson("/intelligence/revision", "GET", undefined, { workspace, signal: controller.signal });
    if (controller.signal.aborted || workspace !== state.current || epoch !== state.requestEpoch) return;
    if (revision.workspace && revision.workspace !== workspace) throw new Error("Workspace response mismatch");
    if (state.project && typeof revision.pending_authorizations === "number") {
      const accepted = observePending(revision.pending_authorizations, stamp);
      // A rejected, pre-mutation observation must not invalidate a newer list.
      if (accepted && state.authorizations.length !== revision.pending_authorizations) state.accessLoaded = false;
      renderStats(); renderAttention();
    }
    if (!state.project || state.syncError || revision.full_refresh_required || revisionKey(revision) !== state.revisionKey) {
      await refreshProject({ reason: "auto", revision, preferCached: true });
    } else {
      state.lastChecked = Date.now(); renderLive();
      setSync("ok", localized("Snapshot up to date", "快照已更新"));
    }
  } catch (error) {
    if (!controller.signal.aborted && workspace === state.current && epoch === state.requestEpoch) {
      if (!state.project) {
        // Initial loading can still obtain the project snapshot when the cheap
        // revision signal is unavailable.
        await refreshProject({ reason: "auto" });
      } else {
        // Keep the last known snapshot visible and retry the cheap signal on
        // the next poll. A successful later signal sees syncError and forces
        // exactly one full refresh, without amplifying a signal outage into a
        // heavy project rebuild every interval.
        state.lastChecked = Date.now();
        console.warn("wcode: revision refresh failed", error);
        showRefreshFailure(error);
      }
    }
  } finally { if (state.pollController === controller) state.pollController = null; }
}
async function refreshActivity() {
  if (state.activityController) return;
  const controller = new AbortController(), workspace = state.current;
  const stamp = observationStamp();
  const previous = {
    snapshot: state.activitySnapshot,
    updated: state.activityUpdated,
    projectActivity: state.project?.activity,
  };
  let published = false;
  state.activityController = controller;
  try {
    const data = await uiJson("/intelligence/activity", "GET", undefined, { workspace, signal: controller.signal, timeout: 10000 });
    if (controller.signal.aborted || !observationCurrent(stamp)) return;
    if (typeof data.workspace !== "string" || (workspace && data.workspace !== workspace)) throw new Error("Workspace response mismatch");
    if (!data.activity || typeof data.activity !== "object" || Array.isArray(data.activity) ||
        (data.activity.available === true && !Array.isArray(data.activity.recent))) {
      throw new Error("Invalid activity response");
    }
    observePending(data.pending_authorizations, stamp);
    state.activitySnapshot = data; state.activityUpdated = Date.now(); state.activityError = false;
    if (state.project) state.project.activity = data.activity;
    published = true;
    renderActivity();
    if (state.project) {
      renderStats(); renderAttention(); renderEngineeringFlow();
      renderChangeConvergenceMap(); renderRuntimeTopology(); renderEngineeringTimeline();
    }
  } catch (error) {
    if (!controller.signal.aborted && observationCurrent(stamp)) {
      if (published) {
        state.activitySnapshot = previous.snapshot;
        state.activityUpdated = previous.updated;
        if (state.project) state.project.activity = previous.projectActivity;
      }
      state.activityError = true;
      const renders = [renderActivity, ...(state.project ? [renderStats, renderAttention, renderEngineeringFlow, renderChangeConvergenceMap, renderRuntimeTopology, renderEngineeringTimeline] : [])];
      for (const render of renders) {
        try { render(); } catch (renderError) {
          console.warn("wcode: activity error view failed", renderError);
          if (render === renderActivity) {
            const unavailable = `<div class="empty">${esc(localized("Activity telemetry unavailable", "活动遥测不可用"))}</div>`;
            setHtml("activity", els.activity, unavailable);
            setHtml("resourceStatus", els.resourceStatus, unavailable);
          }
        }
      }
    }
  } finally { if (state.activityController === controller) state.activityController = null; }
}
async function refreshSemantics() {
  if (els.refreshSemantic.disabled) return;
  const stamp = observationStamp(), workspace = stamp.workspace;
  els.refreshSemantic.disabled = true;
  setSync("loading", t("Syncing"));
  try {
    const result = await uiJson("/intelligence/semantic-refresh", "POST", {}, { workspace, timeout: 120000 });
    if (!observationCurrent(stamp)) return;
    state.semanticRefreshPending = false; state.revisionKey = null;
    await refreshProject({ reason: "manual", force: true });
    if (!observationCurrent(stamp)) return;
    if (Array.isArray(result.failures) && result.failures.length) {
      setSync("warn", localized(`${result.failures.length} semantic refresh failures; inspect provider details`, `${result.failures.length} 项语义刷新失败，请查看分析器详情`));
      revealSection("qualitySection");
    }
  } catch (error) {
    if (!observationCurrent(stamp)) return;
    console.warn("wcode: semantic refresh failed", error);
    if (error.code === "authorization_required" || error.message.includes("authorization required")) {
      state.semanticRefreshPending = true; setAccessPanel(true); await loadAccess();
      if (observationCurrent(stamp)) setSync("warn", t("Semantic refresh needs approval"));
    } else setSync("error", `${t("Refresh failed")} · ${requestFailureMessage(error)}`);
  } finally { els.refreshSemantic.disabled = false; }
}
function autoEnabled() { return state.autoRefresh && !document.hidden; }
function activityInterval() {
  const activity = state.activitySnapshot?.activity;
  // Keep a stable idle cadence so single-flight reschedule contracts stay
  // predictable; only accelerate while work is visibly in flight.
  return !state.activityError && activity?.available === true &&
    (activity.active > 0 || activity.queued > 0 || pendingCount() > 0) ? 2000 : 8000;
}
function scheduleProject() {
  clearTimeout(state.timer); state.timer = null;
  if (autoEnabled() && !state.projectTickActive) {
    // Architecture/overview stay on the 8s cadence; other surfaces can wait longer.
    if (["architecture", "overview"].includes(state.workspaceTab)) {
      state.timer = setTimeout(refreshTick, 8000);
    } else {
      state.timer = setTimeout(refreshTick, 12000);
    }
  }
}
function scheduleActivity() {
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (autoEnabled() && !state.activityTickActive) state.activityTimer = setTimeout(activityTick, activityInterval());
}
async function refreshTick() {
  clearTimeout(state.timer); state.timer = null;
  if (!autoEnabled() || state.projectTickActive) return;
  state.projectTickActive = true;
  try { await pollRevision(); }
  finally { state.projectTickActive = false; scheduleProject(); }
}
async function activityTick() {
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (!autoEnabled() || state.activityTickActive) return;
  state.activityTickActive = true;
  try {
    await refreshActivity();
    void refreshTunnels();
    if (autoEnabled() && accessPanelOpen() && !state.accessBusy) await loadAccess();
  } finally { state.activityTickActive = false; scheduleActivity(); }
}
const commandActions = () => [
  ["overview", localized("Open Overview", "打开总览"), localized("Health, evidence and engineering signals", "健康度、证据与工程信号"), "chart"],
  ["architecture", localized("Open architecture", "打开架构"), localized("System map and dependency evidence", "系统图谱与依赖证据"), "layers"],
  ["activity", localized("Open task activity", "打开任务活动"), localized("Durable execution and runtime work", "持久化执行与实时工作"), "monitor"],
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
  const rows = commandActions().filter(item => !query || item[1].toLowerCase().includes(query) || item[2].toLowerCase().includes(query));
  state.commandPaletteIndex = Math.min(state.commandPaletteIndex, Math.max(0, rows.length - 1));
  setHtml("commandPaletteList", els.commandPaletteList, rows.length ? rows.map((item, index) => `<button type="button" class="command-palette-item" role="option" aria-selected="${index === state.commandPaletteIndex}" data-command="${esc(item[0])}">${uiIcon(item[3])}<span><strong>${esc(item[1])}</strong><small>${esc(item[2])}</small></span><kbd>${index < 9 ? index + 1 : ""}</kbd></button>`).join("") : `<div class="empty">${esc(localized("No matching actions.", "没有匹配的操作。"))}</div>`, () => {
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
  if (["overview", "architecture", "activity", "proof", "changes", "files", "requirements"].includes(command)) activateWorkspaceTab(command, { scroll: true });
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
function schedule() {
  scheduleProject(); scheduleActivity(); renderLive();
}
els.workspace.addEventListener("change", async () => {
  const refresh = refreshProject({ workspace: els.workspace.value, reason: "manual", preferCached: true });
  void activityTick();
  await refresh; scheduleProject();
  if (accessPanelOpen()) await loadAccess();
});
els.language.addEventListener("click", () => {
  state.language = state.language === "zh-CN" ? "en" : "zh-CN";
  savePreference("wcode.ui.language", state.language); applyLanguage();
});
els.theme.addEventListener("click", () => {
  state.theme = state.theme === "system" ? "dark" : state.theme === "dark" ? "light" : "system";
  savePreference("wcode.ui.theme", state.theme); applyTheme();
});
els.manage.addEventListener("click", async () => { const open = !accessPanelOpen(); setAccessPanel(open); if (open) await loadAccess(); });
els.closeAccess.addEventListener("click", () => setAccessPanel(false));
els.addWorkspace.addEventListener("click", addWorkspaceFromUi);
els.fileSearch?.addEventListener("input", renderProjectStructure);
els.workspacePath.addEventListener("keydown", event => { if (event.key === "Enter") addWorkspaceFromUi(); });
els.addCommand.addEventListener("click", addCommandFromUi);
els.allCommandsToggle?.addEventListener("click", toggleAllCommandsFromUi);
els.commandCandidate.addEventListener("keydown", event => { if (event.key === "Enter") addCommandFromUi(); });
els.authorizeOperation.addEventListener("click", authorizeOperationFromUi);
els.operationArgs.addEventListener("keydown", event => { if (event.key === "Enter") authorizeOperationFromUi(); });
els.refresh.addEventListener("click", async () => {
  if (els.refresh.disabled) return;
  setManualRefreshBusy(true);
  try {
    let revision = null;
    try {
      revision = await uiJson("/intelligence/revision", "GET", undefined, { workspace: state.current });
    } catch {}
    await Promise.all([refreshProject({ reason: "manual", force: true, revision, preferCached: true }), refreshActivity(), refreshTunnels()]);
    schedule();
    if (accessPanelOpen()) await loadAccess();
  } finally {
    setManualRefreshBusy(false);
  }
});
els.refreshSemantic.addEventListener("click", refreshSemantics);
els.auto.addEventListener("click", () => {
  state.autoRefresh = !state.autoRefresh;
  applyAutoRefreshControl();
  schedule();
});
els.search.addEventListener("input", () => { invalidate("requirements", "detail"); renderRequirements(); renderDetail(); });
els.componentSearch.addEventListener("input", () => { renderComponentCards(); renderComponentInspector(); });
els.projectNavigator?.addEventListener("input", () => renderProjectNavigator({ open: true }));
els.projectNavigator?.addEventListener("focus", () => renderProjectNavigator({ open: true }));
els.projectNavigator?.addEventListener("keydown", event => {
  if (event.isComposing || event.keyCode === 229) return;
  if (event.key === "Escape") {
    event.preventDefault(); event.stopPropagation();
    els.projectNavigator.value = ""; renderProjectNavigator(); els.projectNavigator.blur();
    return;
  }
  if (els.navigatorResults?.classList.contains("hidden") || els.projectNavigator.getAttribute("aria-expanded") !== "true") return;
  const results = [...(els.navigatorResults?.querySelectorAll("[data-nav-index]") || [])];
  if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && results.length) {
    event.preventDefault();
    const current = results.findIndex(item => item.getAttribute("aria-selected") === "true"),
      next = event.key === "Home" ? 0 : event.key === "End" ? results.length - 1 : event.key === "ArrowDown" ? (current + 1 + results.length) % results.length : (current - 1 + results.length) % results.length;
    results.forEach((item, index) => item.setAttribute("aria-selected", String(index === next)));
    if (results[next]?.id) els.projectNavigator.setAttribute("aria-activedescendant", results[next].id);
    results[next]?.scrollIntoView({ block: "nearest" });
  } else if (event.key === "Enter" && results.length) {
    event.preventDefault();
    const selected = results.find(item => item.getAttribute("aria-selected") === "true") || results[0];
    selected?.click();
  }
});
document.addEventListener("keydown", event => {
  if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return;
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
  if ((event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey))) {
    event.preventDefault();
    if (!event.repeat) setCommandPalette(!state.commandPaletteOpen);
    return;
  }
  if (event.key === "/" && !state.commandPaletteOpen && !event.metaKey && !event.ctrlKey && !event.altKey && !typing) {
    event.preventDefault(); els.projectNavigator?.focus();
  }
});
document.addEventListener("click", event => {
  if (!event.target.closest(".project-navigator")) {
    els.navigatorResults?.classList.add("hidden");
    els.projectNavigator?.setAttribute("aria-expanded", "false");
    els.projectNavigator?.removeAttribute("aria-activedescendant");
  }
});
document.querySelectorAll(".filter").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".filter").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.filter = button.dataset.filter; invalidate("requirements", "detail"); renderRequirements(); renderDetail();
}));
document.querySelectorAll(".arch-mode").forEach(button => button.addEventListener("click", () => {
  document.querySelectorAll(".arch-mode").forEach(item => { item.classList.toggle("active", item === button); item.setAttribute("aria-pressed", String(item === button)); });
  state.architectureMode = button.dataset.archMode || "overlay"; invalidate("architectureGraph"); renderArchitectureGraph();
}));
document.querySelectorAll("[data-arch-back]").forEach(button => button.addEventListener("click", () => {
  state.architectureView = button.dataset.archBack || "blueprint"; renderArchitecture();
}));
document.querySelectorAll("[data-architecture-view]").forEach(button => button.addEventListener("click", () => {
  state.architectureView = button.dataset.architectureView || "blueprint";
  renderArchitecture();
}));
els.systemMapFit?.addEventListener("click", fitSystemMap);
els.systemMapZoomOut?.addEventListener("click", () => setSystemMapScale(state.systemMapScale - .1));
els.systemMapZoomIn?.addEventListener("click", () => setSystemMapScale(state.systemMapScale + .1));
els.systemMapFull?.addEventListener("click", () => setSystemMapFull(!state.systemMapFull));
document.querySelectorAll("[data-workspace-tab]").forEach(button => {
  button.addEventListener("click", () => activateWorkspaceTab(button.dataset.workspaceTab, { scroll: true }));
  button.addEventListener("keydown", event => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const tabs = [...document.querySelectorAll("[data-workspace-tab]")], index = tabs.indexOf(button);
    const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : event.key === "ArrowRight" ? (index + 1) % tabs.length : (index - 1 + tabs.length) % tabs.length;
    event.preventDefault();
    tabs[nextIndex]?.focus();
    activateWorkspaceTab(tabs[nextIndex]?.dataset.workspaceTab, { scroll: true });
  });
});
document.addEventListener("keydown", event => {
  if (event.key !== "Escape" || event.isComposing || event.keyCode === 229 || event.defaultPrevented) return;
  if (state.commandPaletteOpen) { event.preventDefault(); setCommandPalette(false); return; }
  if (state.codeGraphFull) { event.preventDefault(); setCodeGraphFull(false); return; }
  if (accessPanelOpen()) { event.preventDefault(); setAccessPanel(false); return; }
  if (els.componentInspector?.classList.contains("open")) {
    event.preventDefault();
    if (state.architectureView === "blueprint") setSystemMapFull(true);
    else {
      state.selectedComponent = "";
      els.componentInspector.classList.remove("open");
    }
  }
});
window.addEventListener("resize", () => {
  if (accessPanelOpen()) setAccessPanel(true, false);
  if (state.systemMapFit && state.architectureView === "blueprint") requestAnimationFrame(fitSystemMap);
});
systemThemeQuery.addEventListener?.("change", () => { if (state.theme === "system") applyTheme(); });
document.addEventListener("visibilitychange", () => {
  clearTimeout(state.timer); state.timer = null;
  clearTimeout(state.activityTimer); state.activityTimer = null;
  if (document.hidden) {
    cancelTunnelRefresh();
    const codeGraphController = state.codeGraphController;
    state.codeGraphController = null;
    state.codeGraphLoading = false;
    codeGraphController?.abort();
    if (codeGraphController) renderCodeGraph();
    state.pollController?.abort(); state.activityController?.abort(); state.controller?.abort();
  } else if (state.autoRefresh) {
    void refreshTick(); void activityTick();
  }
  renderLive();
});
function startObservatory() {
  if (state.started) return;
  state.started = true;
  wireCommandPalette();
  if (typeof wireCodeGraph === "function") wireCodeGraph();
  activateWorkspaceTab(state.workspaceTab);
  // Neither the initial render nor later project refreshes own the activity loop.
  void refreshTunnels();
  void activityTick();
  return refreshProject({ workspace: state.current, reason: "initial", preferCached: true }).then(scheduleProject);
}

applyTheme();
applyLanguage();
startObservatory();
