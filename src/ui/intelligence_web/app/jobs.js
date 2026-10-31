// Retained native jobs only. This surface never constructs or launches commands.
function validRetainedTaskList(data, workspace, verification = false) {
  const count = value => Number.isSafeInteger(value) && value >= 0;
  return data?.schema_version === 1 && data.workspace === workspace
    && data.kind === (verification ? "verification_task_list" : "command_job_list")
    && data.coverage === "retained_bounded" && data.discovery_is_not_execution === true
    && typeof data.truncated === "boolean" && Array.isArray(data.items) && data.items.length <= 32
    && new Set(data.items.map(row => row?.task_id)).size === data.items.length
    && data.items.every(row => row?.workspace === workspace && /^TASK-\d{20}-[0-9a-f]{32}$/.test(row.task_id)
      && row.tool === (verification ? "verify_project" : "run_command")
      && ["working", "input_required", "completed", "failed", "cancelled", "unknown"].includes(row.status)
      && (verification ? row.origin === "ui" : ["ui", "mcp"].includes(row.origin))
      && typeof row.can_cancel === "boolean" && (!row.can_cancel || (row.origin === "ui" && row.status === "working"))
      && row.completion_is_not_success === true && count(row.created_at_ms) && count(row.updated_at_ms)
      && row.updated_at_ms >= row.created_at_ms);
}
function validRuntimeJob(data, workspace, id) {
  const stream = value => value && typeof value.text === "string" && value.text.length <= 32768
    && Number.isSafeInteger(value.total_bytes) && value.total_bytes >= 0
    && typeof value.truncated === "boolean" && typeof value.redacted === "boolean"
    && Array.from(value.text).reduce((n, char) => n + (char.codePointAt(0) < 128 ? 1 : char.codePointAt(0) < 2048 ? 2 : char.codePointAt(0) < 65536 ? 3 : 4), 0) <= 32768;
  return data?.schema_version === 1 && data.kind === "command_job" && data.workspace === workspace
    && data.task_id === id && data.tool === "run_command" && ["ui", "mcp"].includes(data.origin)
    && ["working", "input_required", "completed", "failed", "cancelled", "unknown"].includes(data.status)
    && typeof data.can_cancel === "boolean" && (!data.can_cancel || (data.origin === "ui" && data.status === "working"))
    && (data.success === null || typeof data.success === "boolean")
    && (data.exit_code === null || Number.isSafeInteger(data.exit_code))
    && (data.error === null || (typeof data.error === "string" && data.error.length <= 4000))
    && data.completion_is_not_success === true && data.acceptance_ready === false
    && stream(data.stdout) && stream(data.stderr);
}
function clearRuntimeJobs() {
  state.runtimeJobsController?.abort(); clearTimeout(state.runtimeJobsTimer);
  state.runtimeJobsSequence++; state.runtimeJobsController = null; state.runtimeJobsTimer = null;
  state.runtimeJobs = []; state.runtimeJobsLoaded = false; state.runtimeJobsTruncated = false;
  state.runtimeJobsStatusFilter = "all"; state.runtimeJobsOriginFilter = "all";
  state.runtimeJob = null; state.runtimeJobSelected = ""; state.runtimeJobsBusy = false;
  state.runtimeJobsMessage = ""; state.runtimeJobsUncertain = false; state.runtimeJobsPolls = 0;
  state.runtimeJobsScope = { workspace: state.current, owner: token, epoch: state.workspaceEpoch };
  // Identical labels do not imply identical ownership. Rebind the controls to
  // this scope even when the cached markup has not changed.
  invalidate("runtimeJobForm");
  invalidate("runtimeJobList"); invalidate("runtimeJobDetail");
}
function runtimeJobsScope() {
  const scope = state.runtimeJobsScope;
  if (!scope || scope.workspace !== state.current || scope.owner !== token || scope.epoch !== state.workspaceEpoch) clearRuntimeJobs();
  return state.runtimeJobsScope;
}
function runtimeJobsRequest() {
  const scope = runtimeJobsScope();
  if (!token || state.runtimeJobsBusy) return null;
  clearTimeout(state.runtimeJobsTimer); state.runtimeJobsTimer = null;
  const controller = new AbortController(), sequence = ++state.runtimeJobsSequence, stamp = observationStamp();
  state.runtimeJobsController = controller; state.runtimeJobsBusy = true; state.runtimeJobsMessage = "";
  renderRuntimeJobs();
  return { workspace: scope.workspace, signal: controller.signal,
    current: () => !controller.signal.aborted && token === scope.owner && observationCurrent(stamp)
      && state.runtimeJobsSequence === sequence && state.runtimeJobsScope === scope };
}
async function refreshRuntimeJobs() {
  const request = runtimeJobsRequest(); if (!request) return false;
  try {
    const data = await uiJson("/intelligence/jobs", "GET", undefined, { ...request, timeout: 15000 });
    if (!request.current()) return false;
    if (!validRetainedTaskList(data, request.workspace)) throw new Error("Invalid retained jobs");
    state.runtimeJobs = data.items; state.runtimeJobsLoaded = true; state.runtimeJobsTruncated = data.truncated;
    return true;
  } catch (_) {
    if (request.current()) {
      state.runtimeJobsLoaded = false; state.runtimeJobs = []; state.runtimeJob = null;
      state.runtimeJobsMessage = localized("Job discovery is unavailable. This is not an empty or complete task history; no command was started.", "任务发现不可用，不代表没有任务或历史完整；未启动任何命令。");
    }
    return false;
  } finally { if (request.current()) { state.runtimeJobsBusy = false; renderRuntimeJobs(); } }
}
async function observeRuntimeJob(id = state.runtimeJobSelected, manual = true) {
  runtimeJobsScope();
  if (!/^TASK-\d{20}-[0-9a-f]{32}$/.test(id) || (id !== state.runtimeJobSelected && !state.runtimeJobs.some(row => row.task_id === id))) return false;
  const request = runtimeJobsRequest(); if (!request) return false;
  if (state.runtimeJobSelected !== id) state.runtimeJob = null;
  state.runtimeJobSelected = id; if (manual) state.runtimeJobsPolls = 0;
  renderRuntimeJobs();
  try {
    const data = await uiJson(`/intelligence/jobs/${encodeURIComponent(id)}`, "GET", undefined, { ...request, timeout: 15000 });
    if (!request.current()) return false;
    if (!validRuntimeJob(data, request.workspace, id)) throw new Error("Invalid command job");
    state.runtimeJob = data; state.runtimeJobsUncertain = false; state.runtimeJobsPolls++;
    return true;
  } catch (_) {
    if (request.current()) {
      state.runtimeJob = null;
      state.runtimeJobsMessage = localized("Job status or output is unknown. Retry this exact ID; no command will be rerun.", "任务状态或输出未知。重试此精确 ID，不会重新运行命令。");
    }
    return false;
  } finally { if (request.current()) { state.runtimeJobsBusy = false; renderRuntimeJobs(); scheduleRuntimeJobObservation(); } }
}
async function cancelRuntimeJob() {
  runtimeJobsScope(); const job = state.runtimeJob;
  if (!job || job.origin !== "ui" || !job.can_cancel || state.runtimeJobsUncertain) return false;
  const request = runtimeJobsRequest(); if (!request) return false;
  try {
    const data = await uiJson(`/intelligence/jobs/${encodeURIComponent(job.task_id)}/cancel`, "POST", {}, { ...request, timeout: 15000 });
    if (!request.current()) return false;
    if (!validRuntimeJob(data, request.workspace, job.task_id)) throw new Error("Invalid cancellation receipt");
    state.runtimeJob = data; state.runtimeJobsUncertain = false; return true;
  } catch (_) {
    if (request.current()) {
      state.runtimeJobsUncertain = true;
      state.runtimeJobsMessage = localized("Cancellation is not confirmed. Refresh this task's status before another cancellation attempt.", "取消尚未确认。再次尝试取消前，请刷新此任务状态。");
    }
    return false;
  } finally { if (request.current()) { state.runtimeJobsBusy = false; renderRuntimeJobs(); } }
}
function scheduleRuntimeJobObservation() {
  clearTimeout(state.runtimeJobsTimer); state.runtimeJobsTimer = null;
  if (state.runtimeJobsBusy || state.runtimeJobsMessage || state.runtimeJobsUncertain || document.hidden
    || state.workspaceTab !== "activity" || state.runtimeJob?.status !== "working" || state.runtimeJobsPolls >= 120) return;
  const scope = runtimeJobsScope(), selected = state.runtimeJobSelected, sequence = state.runtimeJobsSequence;
  state.runtimeJobsTimer = setTimeout(() => {
    // clearTimeout cannot withdraw a callback already queued for delivery.
    if (runtimeJobsScope() !== scope || state.runtimeJobSelected !== selected || state.runtimeJobsSequence !== sequence
      || state.runtimeJobsBusy || state.runtimeJobsMessage || state.runtimeJobsUncertain || document.hidden
      || state.workspaceTab !== "activity" || state.runtimeJob?.status !== "working" || state.runtimeJobsPolls >= 120) return;
    void observeRuntimeJob(selected, false);
  }, 2000);
}
const runtimeJobStatuses = ["all", "working", "input_required", "completed", "failed", "cancelled", "unknown"];
const runtimeJobOrigins = ["all", "ui", "mcp"];
function runtimeJobsView() {
  runtimeJobsScope();
  return state.runtimeJobs.filter(row => (state.runtimeJobsStatusFilter === "all" || row.status === state.runtimeJobsStatusFilter)
    && (state.runtimeJobsOriginFilter === "all" || row.origin === state.runtimeJobsOriginFilter));
}
function setRuntimeJobFilters(status, origin, scope = runtimeJobsScope()) {
  if (runtimeJobsScope() !== scope || !runtimeJobStatuses.includes(status) || !runtimeJobOrigins.includes(origin)) return false;
  state.runtimeJobsStatusFilter = status; state.runtimeJobsOriginFilter = origin;
  renderRuntimeJobs(); return true;
}
function runtimeJobOutputScope() {
  const owner = runtimeJobsScope(), job = state.runtimeJob;
  const task = job?.task_id === state.runtimeJobSelected ? job.task_id : "";
  const previous = state.runtimeJobOutput;
  if (!previous || previous.owner !== owner || previous.task !== task) {
    state.runtimeJobOutput = { owner, task, query: "", stream: "all", page: 0, composing: false };
  }
  return state.runtimeJobOutput;
}
function setRuntimeJobOutput(query, stream, page = 0, view = runtimeJobOutputScope()) {
  if (runtimeJobOutputScope() !== view || typeof query !== "string" || query.length > 256
    || /[\r\n\0]/.test(query) || !["all", "stdout", "stderr"].includes(stream)
    || !Number.isSafeInteger(page) || page < 0) return false;
  const changed = view.query !== query || view.stream !== stream;
  view.query = query; view.stream = stream; view.page = changed ? 0 : page;
  renderRuntimeJobs(); return true;
}
function runtimeJobOutputView() {
  const view = runtimeJobOutputScope(), job = state.runtimeJob;
  const available = !!view.task && validRuntimeJob(job, state.current, view.task);
  const names = view.stream === "all" ? ["stdout", "stderr"] : [view.stream];
  const rows = []; let totalLines = 0, truncated = false, redacted = false;
  if (available) for (const name of names) {
    const stream = job[name], lines = stream.text === "" ? [] : stream.text.split(/\r\n|\r|\n/);
    totalLines += lines.length; truncated ||= stream.truncated; redacted ||= stream.redacted;
    if (view.query) lines.forEach((text, index) => {
      if (text.includes(view.query)) rows.push({ stream: name, line: index + 1, text });
    });
  }
  const pages = Math.max(1, Math.ceil(rows.length / 25));
  view.page = Math.min(view.page, pages - 1);
  return { available, query: view.query, stream: view.stream, page: view.page, pages,
    total_lines: totalLines, total_matches: rows.length, rows: rows.slice(view.page * 25, view.page * 25 + 25),
    limited: truncated || redacted, truncated, redacted };
}
function runtimeJobOutputCoverage(view) {
  return '<p class="panel-meta" data-job-output-coverage>'
    + esc(localized("Search covers retained output only. Line numbers are relative to each retained fragment; streams have no shared time order.",
      "仅搜索已保留输出。行号相对于各自保留片段；两个输出流没有共同时间顺序。"))
    + (view.truncated ? " " + esc(localized("Output is truncated; omitted text is not searched.", "输出已截断，省略的内容不参与搜索。")) : "")
    + (view.redacted ? " " + esc(localized("Output is redacted; hidden text is not searched.", "输出已脱敏，隐藏的内容不参与搜索。")) : "") + "</p>";
}
function runtimeJobOutputHtml(view) {
  if (!view.available) return "";
  const options = ["all", "stdout", "stderr"].map(name => '<option value="' + name + '"'
    + (view.stream === name ? " selected" : "") + ">" + esc(name === "all" ? localized("Both streams", "两个输出流") : name) + "</option>").join("");
  const matches = view.rows.map(row => {
    const at = row.text.indexOf(view.query);
    const text = esc(row.text.slice(0, at)) + "<mark>" + esc(row.text.slice(at, at + view.query.length))
      + "</mark>" + esc(row.text.slice(at + view.query.length));
    return '<li data-job-output-match><strong>' + esc(row.stream) + " · "
      + esc(localized("retained line ", "保留片段第 ")) + row.line + esc(localized("", " 行"))
      + "</strong><pre>" + text + "</pre></li>";
  }).join("");
  const summary = view.query
    ? localized(view.total_matches + " matching retained lines of " + view.total_lines + "; showing " + view.rows.length + ".",
      view.total_lines + " 行保留文本中有 " + view.total_matches + " 行匹配，当前显示 " + view.rows.length + " 行。")
    : localized("Enter literal text to locate matching retained lines.", "输入字面文本以定位匹配的保留行。");
  const pages = view.total_matches > 25 ? '<div class="runtime-job-output-pages">'
    + '<button type="button" class="quiet-action" data-job-output-previous data-job-output-page="' + (view.page - 1) + '"' + (!view.page ? " disabled" : "") + ">"
    + esc(localized("Previous", "上一页")) + "</button><span>" + esc(localized("Page ", "第 ")) + (view.page + 1) + " / " + view.pages
    + '</span><button type="button" class="quiet-action" data-job-output-next data-job-output-page="' + (view.page + 1) + '"'
    + (view.page + 1 >= view.pages ? " disabled" : "") + ">" + esc(localized("Next", "下一页")) + "</button></div>" : "";
  return '<section class="runtime-job-output" aria-label="' + esc(localized("Search retained output", "搜索保留输出")) + '">'
    + '<div class="runtime-job-output-controls"><label>' + esc(localized("Find in retained output", "在保留输出中查找"))
    + '<input type="search" data-job-output-query maxlength="256" spellcheck="false" autocomplete="off" value="' + esc(view.query)
    + '" aria-describedby="runtimeJobOutputHint"></label><label>' + esc(localized("Output stream", "输出流"))
    + '<select data-job-output-stream>' + options + "</select></label></div>"
    + '<p id="runtimeJobOutputHint" class="panel-meta">' + esc(localized("Literal, case-sensitive text; up to 256 characters.",
      "按字面区分大小写匹配，最多 256 个字符。")) + "</p>" + runtimeJobOutputCoverage(view)
    + '<p class="panel-meta" role="status" data-job-output-note>' + esc(summary) + "</p>"
    + (view.query && !view.total_matches ? '<p data-job-output-empty>' + esc(localized("No matches in the selected retained output.",
      "所选保留输出中没有匹配。")) + "</p>" : "")
    + '<ol class="runtime-job-output-matches">' + matches + "</ol>" + pages + "</section>";
}
function bindRuntimeJobOutput(host, view) {
  const input = host?.querySelector("[data-job-output-query]");
  input?.addEventListener("compositionstart", () => { if (runtimeJobOutputScope() === view) view.composing = true; });
  input?.addEventListener("compositionend", () => {
    if (runtimeJobOutputScope() !== view) return;
    view.composing = false; setRuntimeJobOutput(input.value, view.stream, 0, view);
  });
  input?.addEventListener("input", event => {
    if (!event.isComposing && !view.composing) setRuntimeJobOutput(input.value, view.stream, 0, view);
  });
  const stream = host?.querySelector("[data-job-output-stream]");
  stream?.addEventListener("change", () => setRuntimeJobOutput(view.query, stream.value, 0, view));
  host?.querySelectorAll("[data-job-output-page]").forEach(button => button.addEventListener("click",
    () => setRuntimeJobOutput(view.query, view.stream, Number(button.dataset.jobOutputPage), view)));
}
function setRuntimeJobDetailHtml(html, bind, view) {
  const host = document.getElementById("runtimeJobDetail");
  if (!host) return;
  const same = host._runtimeJobOutputView === view;
  if (same && view.composing) {
    // Preserve the composing node while withdrawing any old mutation control.
    host.querySelectorAll("[data-job-cancel]").forEach(button => { button.disabled = true; });
    const note = host.querySelector("[data-job-output-note]");
    if (note) note.textContent = localized("Finish typing to refresh the output view.", "完成输入后刷新输出视图。");
    invalidate("runtimeJobDetail"); return;
  }
  const active = document.activeElement;
  const focusSelectors = ["[data-job-output-query]", "[data-job-output-stream]", "[data-job-output-previous]", "[data-job-output-next]"];
  const selector = same && host.contains?.(active) ? focusSelectors.find(value => active.matches?.(value)) : null;
  const selection = selector === "[data-job-output-query]" ? [active.selectionStart, active.selectionEnd, active.selectionDirection] : null;
  const raw = same ? Array.from(host.querySelectorAll("[data-job-output-raw]")).map(node => [node.dataset.jobOutputRaw, node.open]) : [];
  if (!same) invalidate("runtimeJobDetail");
  setHtml("runtimeJobDetail", host, html, bind);
  host._runtimeJobOutputView = view;
  for (const [name, open] of raw) { const node = host.querySelector('[data-job-output-raw="' + name + '"]'); if (node) node.open = open; }
  let restored = selector && host.querySelector(selector);
  if (selector === "[data-job-output-previous]" || selector === "[data-job-output-next]") {
    if (!restored || restored.disabled) restored = host.querySelector(selector === "[data-job-output-next]" ? "[data-job-output-previous]" : "[data-job-output-next]");
    if (!restored || restored.disabled) restored = host.querySelector("[data-job-output-query]");
  }
  restored?.focus({ preventScroll: true });
  if (restored && selection && Number.isInteger(selection[0])) restored.setSelectionRange?.(...selection);
}

function renderRuntimeJobs() {
  const scope = runtimeJobsScope(), outputScope = runtimeJobOutputScope();
  const host = id => document.getElementById(id), job = state.runtimeJob, busy = state.runtimeJobsBusy;
  const selected = state.runtimeJobSelected;
  const currentScope = () => runtimeJobsScope() === scope && state.workspaceTab === "activity";
  const currentSelection = () => currentScope() && state.runtimeJobSelected === selected;
  const filtered = runtimeJobsView();
  const options = (values, selected, labels) => values.map(value => `<option value="${value}" ${selected === value ? "selected" : ""}>${esc(value === "all" ? localized("All", "全部") : labels(value))}</option>`).join("");
  setHtml("runtimeJobForm", host("runtimeJobForm"), `<button type="button" class="quiet-action" data-jobs-refresh ${busy || !token ? "disabled" : ""}>${esc(localized("Refresh retained jobs", "刷新保留任务"))}</button>
    <label class="panel-meta">${esc(localized("Task state", "任务状态"))} <select data-jobs-state>${options(runtimeJobStatuses, state.runtimeJobsStatusFilter, statusLabel)}</select></label>
    <label class="panel-meta">${esc(localized("Task origin", "任务来源"))} <select data-jobs-origin>${options(runtimeJobOrigins, state.runtimeJobsOriginFilter, value => value === "ui" ? "UI" : "MCP")}</select></label>`, () => {
    host("runtimeJobForm")?.querySelector("[data-jobs-refresh]")?.addEventListener("click", () => { if (currentScope()) void refreshRuntimeJobs(); });
    for (const selector of ["[data-jobs-state]", "[data-jobs-origin]"]) {
      host("runtimeJobForm")?.querySelector(selector)?.addEventListener("change", () => {
        if (runtimeJobsScope() !== scope) return;
        const form = host("runtimeJobForm"), status = form?.querySelector("[data-jobs-state]")?.value, origin = form?.querySelector("[data-jobs-origin]")?.value;
        if (setRuntimeJobFilters(status, origin, scope)) host("runtimeJobForm")?.querySelector(selector)?.focus();
      });
    }
  });
  setHtml("runtimeJobMessage", host("runtimeJobMessage"), `<p class="${state.runtimeJobsMessage ? "warn" : "panel-meta"}">${esc(state.runtimeJobsMessage || localized("Bounded retained history, not a complete process list. Task completion is not successful verification or Acceptance.", "有界保留历史，不是完整进程列表。任务完成不等于验证通过或验收批准。"))}</p>${state.runtimeJobsTruncated ? `<p class="warn">${esc(localized("Discovery was truncated or some records were unavailable.", "发现结果已截断或部分记录不可用。"))}</p>` : ""}`);
  const filterSummary = state.runtimeJobsLoaded ? `<p role="status" class="panel-meta">${esc(localized(`Showing ${filtered.length} of ${state.runtimeJobs.length} retained records. Filtering does not fetch additional history.`, `显示 ${state.runtimeJobs.length} 条保留记录中的 ${filtered.length} 条；筛选不会读取更多历史。`))}</p>` : "";
  const rows = filtered.map(row => `<button type="button" class="quiet-action" data-job-id="${esc(row.task_id)}" ${busy ? "disabled" : ""}><code>${esc(row.task_id)}</code> · ${esc(row.origin)} · ${esc(statusLabel(row.status))}</button>`).join("");
  setHtml("runtimeJobList", host("runtimeJobList"), filterSummary + (rows || `<p class="panel-meta">${esc(state.runtimeJobsLoaded ? localized("No retained records match these filters in this bounded response.", "此有界响应中没有符合筛选条件的保留记录。") : localized("Refresh to discover real retained task IDs.", "刷新以发现真实的保留任务 ID。"))}</p>`), () => host("runtimeJobList")?.querySelectorAll("[data-job-id]").forEach(button => button.addEventListener("click", () => { if (currentScope()) void observeRuntimeJob(button.dataset.jobId); })));
  const controls = state.runtimeJobSelected ? `<button type="button" class="quiet-action" data-job-status ${busy ? "disabled" : ""}>${esc(localized("Refresh exact task", "刷新精确任务"))}</button>${job?.can_cancel && job.origin === "ui" ? `<button type="button" class="quiet-action" data-job-cancel ${busy || state.runtimeJobsUncertain ? "disabled" : ""}>${esc(localized("Cancel this UI-owned job", "取消此 UI 所属任务"))}</button>` : ""}` : "";
  const streams = job ? ["stdout", "stderr"].map(name => `<details open data-job-output-raw="${name}"><summary>${name} · ${num(job[name].total_bytes)} bytes ${job[name].truncated ? esc(localized("· truncated", "· 已截断")) : ""} ${job[name].redacted ? esc(localized("· redacted", "· 已脱敏")) : ""}</summary><pre>${esc(job[name].text)}</pre></details>`).join("") : "";
  setRuntimeJobDetailHtml(`${controls}${job ? `<p><code>${esc(job.task_id)}</code> · ${esc(job.origin)} · ${esc(statusLabel(job.status))}</p><p class="panel-meta">${esc(localized("Command outcome", "命令结果"))}: ${esc(job.success === null ? localized("Unknown", "未知") : job.success ? localized("Reported success", "报告成功") : localized("Reported failure", "报告失败"))} · exit ${esc(job.exit_code ?? "—")}</p>${job.error ? `<p class="warn">${esc(job.error)}</p>` : ""}${runtimeJobOutputHtml(runtimeJobOutputView())}${streams}` : ""}${state.runtimeJobsPolls >= 120 ? `<p>${esc(localized("Automatic observation paused; refresh the exact task to continue.", "自动观测已暂停；刷新精确任务以继续。"))}</p>` : ""}`, () => {
    host("runtimeJobDetail")?.querySelector("[data-job-status]")?.addEventListener("click", () => { if (currentSelection()) void observeRuntimeJob(selected); });
    host("runtimeJobDetail")?.querySelector("[data-job-cancel]")?.addEventListener("click", () => { if (currentSelection()) void cancelRuntimeJob(); });
    bindRuntimeJobOutput(host("runtimeJobDetail"), outputScope);
  }, outputScope);
}
