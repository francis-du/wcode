// Correlate a live protected activity row with its exact retained command job.
// Cached project activity is not a current source of job navigation authority.
function currentActivityJobSource() {
  const source = state.activitySnapshot, activity = source?.activity;
  if (!token || document.hidden || state.workspaceTab !== "activity" || state.activityError
    || source?.workspace !== state.current || activity?.workspace !== state.current
    || activity.available !== true || !Array.isArray(activity.recent) || activity.recent.length > 12) return null;
  return source;
}
function beginActivityJobRender(activity) {
  const source = currentActivityJobSource(), scope = runtimeJobsScope(), previous = state.activityJobsRender;
  const focused = previous?.scope === scope ? previous.buttons?.get(document.activeElement) : null;
  const render = { scope, source: source?.activity === activity ? source : null,
    activity, rows: activity?.recent, buttons: new Map(),
    focus: focused ? { taskId: focused.taskId, jobId: focused.jobId, element: document.activeElement } : null };
  state.activityJobsRender = render;
  return render;
}
function currentActivityJobSelection(selection, id) {
  if (!selection || typeof id !== "string" || !/^TASK-\d{20}-[0-9a-f]{32}$/.test(id)) return false;
  const render = selection.render, task = selection.task;
  return !!render && render === state.activityJobsRender && render.scope === runtimeJobsScope()
    && render.source !== null && render.source === currentActivityJobSource()
    && render.source.activity === render.activity && render.activity.recent === render.rows
    && task?.tool === "run_command" && Number.isSafeInteger(selection.taskId) && selection.taskId > 0
    && task.id === selection.taskId && task.command_job_id === id && selection.jobId === id
    && ["queued", "running", "completed", "failed"].includes(task.status)
    && render.rows.filter(row => row?.id === selection.taskId).length === 1
    && render.rows.includes(task);
}
function activityJobSelection(task, render) {
  const selection = { render, task, taskId: task?.id, jobId: task?.command_job_id };
  return currentActivityJobSelection(selection, selection.jobId) ? selection : null;
}
function activityJobLinkHtml(task, render) {
  if (task?.tool !== "run_command") return "";
  const selection = activityJobSelection(task, render);
  if (!selection) return `<small class="activity-job-unavailable">${esc(localized("Retained output unavailable", "保留输出暂不可用"))}</small>`;
  return `<button type="button" class="quiet-action activity-job-link" data-activity-job="${esc(selection.jobId)}" data-activity-task="${selection.taskId}" aria-label="${esc(localized(`Inspect retained output for task #${selection.taskId}`, `查看任务 #${selection.taskId} 的保留输出`))}">${esc(localized("Inspect output", "查看输出"))}</button>`;
}
function bindActivityJobLinks(host, render) {
  host?.querySelectorAll("[data-activity-job]").forEach(button => {
    const id = button.dataset.activityJob, taskId = button.dataset.activityTask;
    const task = render.rows?.find(row => String(row?.id) === taskId && row?.command_job_id === id);
    const selection = activityJobSelection(task, render);
    // Rebind even if setHtml retained identical markup for a new observation.
    // A saved callback still belongs to its original render and cannot retarget.
    button.onclick = () => { if (selection) void observeActivityJob(id, selection); };
    if (selection) render.buttons.set(button, selection);
    const focus = render.focus;
    if (selection && focus?.taskId === selection.taskId && focus.jobId === id
      && document.activeElement !== button
      && (document.activeElement === focus.element || document.activeElement === document.body)) {
      button.focus({ preventScroll: true });
    }
  });
  render.focus = null;
}
async function observeActivityJob(id, selection) {
  if (!currentActivityJobSelection(selection, id) || state.runtimeJobsBusy) return false;
  const pending = observeRuntimeJob(id, true, selection);
  if (state.runtimeJobSelected === id) {
    const detail = document.getElementById("runtimeJobDetail");
    detail?.scrollIntoView?.({ block: "start" });
    detail?.focus?.({ preventScroll: true });
  }
  return pending;
}
