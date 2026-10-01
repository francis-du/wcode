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
  state.runtimeJob = null; state.runtimeJobSelected = ""; state.runtimeJobsBusy = false;
  state.runtimeJobsMessage = ""; state.runtimeJobsUncertain = false; state.runtimeJobsPolls = 0;
  state.runtimeJobsScope = { workspace: state.current, owner: token, epoch: state.workspaceEpoch };
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
  state.runtimeJobSelected = id; if (manual) state.runtimeJobsPolls = 0;
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
  state.runtimeJobsTimer = setTimeout(() => void observeRuntimeJob(state.runtimeJobSelected, false), 2000);
}
function renderRuntimeJobs() {
  runtimeJobsScope();
  const host = id => document.getElementById(id), job = state.runtimeJob, busy = state.runtimeJobsBusy;
  setHtml("runtimeJobForm", host("runtimeJobForm"), `<button type="button" class="quiet-action" data-jobs-refresh ${busy || !token ? "disabled" : ""}>${esc(localized("Refresh retained jobs", "刷新保留任务"))}</button>`, () => host("runtimeJobForm")?.querySelector("[data-jobs-refresh]")?.addEventListener("click", () => void refreshRuntimeJobs()));
  setHtml("runtimeJobMessage", host("runtimeJobMessage"), `<p class="${state.runtimeJobsMessage ? "warn" : "panel-meta"}">${esc(state.runtimeJobsMessage || localized("Bounded retained history, not a complete process list. Task completion is not successful verification or Acceptance.", "有界保留历史，不是完整进程列表。任务完成不等于验证通过或验收批准。"))}</p>${state.runtimeJobsTruncated ? `<p class="warn">${esc(localized("Discovery was truncated or some records were unavailable.", "发现结果已截断或部分记录不可用。"))}</p>` : ""}`);
  const rows = state.runtimeJobs.map(row => `<button type="button" class="quiet-action" data-job-id="${esc(row.task_id)}" ${busy ? "disabled" : ""}><code>${esc(row.task_id)}</code> · ${esc(row.origin)} · ${esc(statusLabel(row.status))}</button>`).join("");
  setHtml("runtimeJobList", host("runtimeJobList"), rows || `<p class="panel-meta">${esc(state.runtimeJobsLoaded ? localized("No matching retained records in this bounded response.", "此有界响应中没有匹配的保留记录。") : localized("Refresh to discover real retained task IDs.", "刷新以发现真实的保留任务 ID。"))}</p>`, () => host("runtimeJobList")?.querySelectorAll("[data-job-id]").forEach(button => button.addEventListener("click", () => void observeRuntimeJob(button.dataset.jobId))));
  const controls = state.runtimeJobSelected ? `<button type="button" class="quiet-action" data-job-status ${busy ? "disabled" : ""}>${esc(localized("Refresh exact task", "刷新精确任务"))}</button>${job?.can_cancel && job.origin === "ui" ? `<button type="button" class="quiet-action" data-job-cancel ${busy || state.runtimeJobsUncertain ? "disabled" : ""}>${esc(localized("Cancel this UI-owned job", "取消此 UI 所属任务"))}</button>` : ""}` : "";
  const streams = job ? ["stdout", "stderr"].map(name => `<details open><summary>${name} · ${num(job[name].total_bytes)} bytes ${job[name].truncated ? esc(localized("· truncated", "· 已截断")) : ""} ${job[name].redacted ? esc(localized("· redacted", "· 已脱敏")) : ""}</summary><pre>${esc(job[name].text)}</pre></details>`).join("") : "";
  setHtml("runtimeJobDetail", host("runtimeJobDetail"), `${controls}${job ? `<p><code>${esc(job.task_id)}</code> · ${esc(job.origin)} · ${esc(statusLabel(job.status))}</p><p class="panel-meta">${esc(localized("Command outcome", "命令结果"))}: ${esc(job.success === null ? localized("Unknown", "未知") : job.success ? localized("Reported success", "报告成功") : localized("Reported failure", "报告失败"))} · exit ${esc(job.exit_code ?? "—")}</p>${job.error ? `<p class="warn">${esc(job.error)}</p>` : ""}${streams}` : ""}${state.runtimeJobsPolls >= 120 ? `<p>${esc(localized("Automatic observation paused; refresh the exact task to continue.", "自动观测已暂停；刷新精确任务以继续。"))}</p>` : ""}`, () => {
    host("runtimeJobDetail")?.querySelector("[data-job-status]")?.addEventListener("click", () => void observeRuntimeJob());
    host("runtimeJobDetail")?.querySelector("[data-job-cancel]")?.addEventListener("click", () => void cancelRuntimeJob());
  });
}
