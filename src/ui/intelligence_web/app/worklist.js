// Read-only projection of the existing Worklist. Filters never mutate task state.
const worklistFilters = ["open", "all", "pending", "in_progress", "blocked", "done", "unknown"];
function worklistViewScope() {
  let scope = state.worklistViewScope;
  if (!scope || scope.workspace !== state.current || scope.owner !== token || scope.epoch !== state.workspaceEpoch) {
    scope = { workspace: state.current, owner: token, epoch: state.workspaceEpoch, filter: "open", query: "", page: 0, revision: null };
    state.worklistViewScope = scope;
    invalidate("executionStatus");
  }
  return scope;
}
function setWorklistView(filter, query = "", page = 0, scope = worklistViewScope()) {
  if (worklistViewScope() !== scope || !worklistFilters.includes(filter) || typeof query !== "string"
    || query.length > 256 || !Number.isSafeInteger(page) || page < 0 || page > 8) return false;
  scope.filter = filter; scope.query = query; scope.page = page;
  renderExecutionStatus(); return true;
}
function worklistSnapshotStale() {
  return Boolean(state.snapshotStale || state.syncError || state.fitnessSnapshotFromCache
    || state.project?.snapshot_unconfirmed || state.project?.snapshot_refreshing);
}
function worklistRenderDeferred() {
  if (!worklistViewScope().composing) return false;
  // A successful newer observation may also revoke a runnable lane.
  // Keep the composing input, but never retain positive labels while paint waits.
  els.executionStatus?.querySelectorAll("[data-worklist-runnable]").forEach(element => element.remove());
  invalidate("executionStatus");
  return true;
}
function setExecutionStatusHtml(html) {
  const host = els.executionStatus, scope = worklistViewScope(), active = document.activeElement;
  const selector = host?.contains?.(active) && ["[data-worklist-search]", "[data-worklist-filter]", "[data-worklist-previous]", "[data-worklist-next]"]
    .find(value => active?.matches?.(value));
  const start = active?.selectionStart, end = active?.selectionEnd;
  setHtml("executionStatus", host, html, bindWorklistControls);
  if (selector && worklistViewScope() === scope) {
    const replacement = host.querySelector(selector);
    if (replacement && replacement !== active) {
      replacement.focus();
      if (typeof start === "number" && typeof replacement.setSelectionRange === "function") replacement.setSelectionRange(start, end ?? start);
    }
  }
}
function bindWorklistControls() {
  const host = els.executionStatus, scope = worklistViewScope();
  const bind = (selector, event, update) => host?.querySelector(selector)?.addEventListener(event, eventObject => {
    if (worklistViewScope() !== scope || eventObject?.isComposing) return;
    const old = host.querySelector(selector), position = old?.selectionStart;
    update(old);
    const next = host.querySelector(selector); next?.focus();
    if (typeof position === "number" && typeof next?.setSelectionRange === "function") next.setSelectionRange(position, position);
  });
  bind("[data-worklist-filter]", "change", element => setWorklistView(element.value, scope.query, 0, scope));
  bind("[data-worklist-search]", "input", element => setWorklistView(scope.filter, element.value, 0, scope));
  host?.querySelector("[data-worklist-search]")?.addEventListener("compositionstart", () => {
    if (worklistViewScope() === scope) scope.composing = true;
  });
  bind("[data-worklist-search]", "compositionend", element => {
    scope.composing = false; setWorklistView(scope.filter, element.value, 0, scope);
  });
  bind("[data-worklist-previous]", "click", () => setWorklistView(scope.filter, scope.query, Math.max(0, scope.page - 1), scope));
  bind("[data-worklist-next]", "click", () => setWorklistView(scope.filter, scope.query, scope.page + 1, scope));
}
function worklistTaskState(item) {
  return ["pending", "in_progress", "blocked", "done"].includes(item.status) ? item.status : "unknown";
}
function worklistStateLabel(value) {
  return { open: localized("Open tasks", "未完成任务"), all: localized("All tasks", "全部任务"),
    pending: localized("Pending", "待处理"), in_progress: localized("In progress", "进行中"),
    blocked: localized("Blocked", "已阻塞"), done: localized("Marked done", "标记完成"),
    unknown: localized("Unknown task state", "任务状态未知") }[value];
}
function worklistObservation(worklist) {
  const historical = worklist?.available === false && worklist.last_known?.available === true;
  const observed = historical ? worklist.last_known : worklist;
  if (!observed || observed.available === false || typeof observed.exists !== "boolean"
    || historical && !observed.exists || observed.items_included === false) return { known: false };
  if (!observed.exists) return { known: true, exists: false };
  const items = observed.items;
  if (!Array.isArray(items) || items.length > 64 || items.some(item => !item || typeof item !== "object"
    || Array.isArray(item) || typeof item.id !== "string" || !item.id || item.id.length > 160)
    || new Set(items.map(item => item.id)).size !== items.length) return { known: false };
  const counts = observed.counts, states = ["pending", "in_progress", "blocked", "done"];
  // Native Worklist counts omit zero-count states.
  const values = states.map(key => counts && Object.prototype.hasOwnProperty.call(counts, key) ? counts[key] : 0);
  const countMismatch = counts !== undefined && (!counts || typeof counts !== "object" || Array.isArray(counts)
    || Object.keys(counts).some(key => !states.includes(key))
    || values.some((value, index) => !Number.isSafeInteger(value) || value < 0
      || value !== items.filter(item => item.status === states[index]).length)
    || values.reduce((sum, value) => sum + value, 0) !== items.length);
  return { known: true, exists: true, observed, historical, items, byId: new Map(items.map(item => [item.id, item])),
    partial: observed.truncated === true || countMismatch };
}
function worklistDependencies(item, observation) {
  const dependencies = item.depends_on === undefined ? [] : item.depends_on;
  if (!Array.isArray(dependencies) || dependencies.length > 16
    || dependencies.some(id => typeof id !== "string" || !id || id.length > 160 || id === item.id)
    || new Set(dependencies).size !== dependencies.length) {
    return { satisfied: false, html: esc(localized("Dependency state unknown", "依赖状态未知")) };
  }
  if (!dependencies.length) return { satisfied: true, html: esc(localized("No dependencies", "无前置依赖")) };
  const satisfied = dependencies.every(id => worklistTaskState(observation.byId.get(id) || {}) === "done");
  const labels = dependencies.slice(0, 4).map(id => {
    const dependency = observation.byId.get(id);
    const label = dependency ? worklistStateLabel(worklistTaskState(dependency)) : observation.partial
      ? localized("Dependency not observed", "依赖未观测") : localized("Missing dependency", "缺失依赖");
    return esc(id) + " · " + esc(label);
  });
  return { satisfied, html: (satisfied ? esc(localized("Dependencies satisfied", "依赖已满足")) + " · " : "")
    + labels.join(" / ") + (dependencies.length > 4 ? " · +" + num(dependencies.length - 4) : "") };
}
function worklistQueueView(observation) {
  const scope = worklistViewScope(), revision = String(observation.historical) + ":" + observation.observed.revision;
  if (scope.revision !== revision) { scope.revision = revision; scope.page = 0; }
  const search = scope.query.trim().toLowerCase(), items = observation.items;
  const filtered = items.filter(item => {
    const status = worklistTaskState(item), fields = [item.id, item.title, item.note, item.claim?.actor, item.result?.summary,
      ...(Array.isArray(item.write_paths) ? item.write_paths : [])];
    return (scope.filter === "all" || scope.filter === "open" && status !== "done" || scope.filter === status)
      && (!search || fields.some(value => typeof value === "string" && value.toLowerCase().includes(search)));
  });
  const rank = { blocked: 0, in_progress: 1, pending: 2, unknown: 3, done: 4 };
  filtered.sort((a, b) => rank[worklistTaskState(a)] - rank[worklistTaskState(b)]);
  scope.page = Math.min(scope.page, Math.max(0, Math.ceil(filtered.length / 8) - 1));
  const stale = worklistSnapshotStale();
  return { scope, filtered, visible: filtered.slice(scope.page * 8, scope.page * 8 + 8), stale,
    current: !stale && !observation.historical && !observation.partial };
}
function worklistControlsHtml(view) {
  const { scope, filtered } = view;
  const options = worklistFilters.map(value => '<option value="' + value + '"' + (scope.filter === value ? " selected" : "")
    + ">" + esc(worklistStateLabel(value)) + "</option>").join("");
  return '<div class="worklist-toolbar"><label>' + esc(localized("Task state", "任务状态"))
    + '<select data-worklist-filter aria-label="' + esc(localized("Filter engineering tasks", "筛选工程任务")) + '">' + options + "</select></label><label>"
    + esc(localized("Find task", "查找任务")) + '<input type="search" data-worklist-search maxlength="256" value="' + esc(scope.query)
    + '" aria-label="' + esc(localized("Search task title, note, actor or path", "搜索任务标题、备注、执行者或路径")) + '"></label></div>'
    + '<div class="worklist-pagination"><button type="button" class="quiet-action" data-worklist-previous ' + (scope.page === 0 ? "disabled" : "") + ">"
    + esc(localized("Previous", "上一页")) + '</button><span role="status" class="panel-meta">'
    + esc(localized("Page", "页")) + " " + num(scope.page + 1) + " / " + num(Math.max(1, Math.ceil(filtered.length / 8)))
    + " · " + num(filtered.length) + " " + esc(localized("matching observed tasks", "条匹配的已观测任务"))
    + '</span><button type="button" class="quiet-action" data-worklist-next ' + ((scope.page + 1) * 8 >= filtered.length ? "disabled" : "") + ">"
    + esc(localized("Next", "下一页")) + "</button></div>";
}
function worklistTaskDetailsHtml(item, observation, view) {
  const status = worklistTaskState(item), dependencies = worklistDependencies(item, observation), claim = item.claim;
  const free = !claim || claim.expired === true || Number.isFinite(claim.expires_at_ms) && claim.expires_at_ms > 0 && claim.expires_at_ms <= Date.now();
  const runnable = view.current && free && ["pending", "in_progress"].includes(status) && dependencies.satisfied
    && Array.isArray(observation.observed.runnable) && observation.observed.runnable.includes(item.id);
  const note = typeof item.note === "string" ? item.note.slice(0, 1000) + (item.note.length > 1000 ? "…" : "") : "";
  return '<div class="worklist-task-state">' + pill(worklistStateLabel(status), status === "blocked" || status === "unknown" ? "warn" : "info")
    + "<code>" + esc(item.id) + "</code>" + (runnable ? '<span data-worklist-runnable>' + pill(localized("Reported runnable", "报告可运行"), "info") + "</span>" : "") + "</div>"
    + '<p class="worklist-dependencies">' + dependencies.html + "</p>"
    + (note ? '<p class="worklist-task-note">' + esc(note) + "</p>" : "");
}
function worklistOwnershipHtml(worklist) {
  const observation = worklistObservation(worklist);
  const heading = esc(localized("Engineering work queue", "工程任务队列"));
  const unknown = '<section class="execution-workers execution-empty warn"><strong>' + heading + '</strong><span>'
    + esc(localized("Ownership state is unknown; refresh the observed Worklist.", "任务所有权未知；请刷新观测到的 Worklist。")) + "</span></section>";
  if (!observation.known) return unknown;
  if (!observation.exists) return '<section class="execution-workers execution-empty"><strong>' + heading + '</strong><span>'
    + esc(localized("No durable Worklist has been created.", "尚未创建持久化 Worklist。")) + "</span></section>";
  const { historical, items } = observation, view = worklistQueueView(observation);
  worklist = observation.observed;
  const text = (value, fallback, limit = 300) => esc(typeof value === "string" ? value.slice(0, limit) : fallback);
  const rows = view.visible.map(item => {
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
    return `<article class="execution-worker" data-worklist-task="${esc(item.id)}"><div class="execution-worker-head"><strong>${text(item.title, item.id || localized("Untitled task", "未命名任务"))}</strong>${pill(ownership, historical || (claim && (!validClaim || expired)) ? "warn" : "info")}</div>
      ${worklistTaskDetailsHtml(item, observation, view)}
      <div class="execution-worker-facts"><span>${esc(localized("Actor", "执行者"))}<b>${claim ? text(claim.actor, "—", 128) : "—"}</b></span><span>${esc(localized("Lease expires", "租约到期"))}<b>${esc(lease)}</b></span><span>${esc(localized("Write scopes", "写入范围"))}<b>${scope}${paths.length > 4 ? ` · +${num(paths.length - 4)}` : ""}</b></span></div>
      ${result ? `<div class="execution-worker-result">${pill(outcome, "warn")}<span>${text(result.summary, localized("No task summary", "没有任务摘要"), 500)}</span><small>${esc(localized("Reported by", "报告者"))} ${text(result.actor, "—", 128)} · ${evidenceCount == null ? esc(localized("Evidence references unknown", "Evidence 引用未知")) : `${num(evidenceCount)} ${esc(localized("Evidence references", "Evidence 引用"))}`}</small></div>` : ""}</article>`;
  }).join("");
  const bounded = observation.partial || items.length > 8;
  return `<section class="execution-workers execution-blockers"><div class="execution-worker-head"><span class="execution-label">${heading}</span><span class="panel-meta">${historical ? esc(localized("Last observed", "最后观测")) + " " : ""}Worklist #${num(worklist.revision || 0)}</span></div>
    <p class="panel-meta">${esc(localized("Worker reports require independent verification; they do not certify proof.", "Worker 报告需要独立验证；不会认证证明。"))}</p>
    ${historical ? `<p class="warning-inline">${esc(localized("Ownership state is unknown. These are historical observations; refresh before acting on claims.", "当前任务所有权未知。以下为历史观测；请刷新后再依赖领取信息。"))}</p>` : ""}
    ${view.stale && !historical ? `<p class="warning-inline">${esc(localized("Snapshot stale; refresh task ownership.", "快照已过期；请刷新任务所有权。"))}</p>` : ""}
    ${worklistControlsHtml(view)}
    ${observation.partial ? `<p class="warning-inline">${esc(localized("Partial Worklist response; missing tasks and dependencies remain unknown.", "Worklist 响应不完整；缺少的任务和依赖仍为未知。"))}</p>` : ""}
    <div class="execution-worker-list">${rows || `<span class="panel-meta">${esc(observation.partial ? localized("Partial Worklist observation; no matching tasks are visible.", "Worklist 观测不完整；当前没有可见的匹配任务。") : items.length ? localized("No observed tasks match these filters.", "没有符合筛选条件的已观测任务。") : localized("No items in the observed Worklist.", "观测到的 Worklist 没有任务。"))}</span>`}</div>
    ${bounded ? `<p class="panel-meta">${esc(localized("Bounded ownership view", "有界任务所有权视图"))} · ${num(view.visible.length)} / ${num(items.length)} ${esc(localized("observed items", "已观测任务"))}</p>` : ""}</section>`;
}
