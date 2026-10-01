// Native verification tasks only: transport completion never establishes Acceptance.
function verificationEntry(create = false) {
  let entry = state.verificationTasks.get(state.current);
  if (entry && entry.owner !== token) {
    entry.controller?.abort(); clearTimeout(entry.timer);
    state.verificationTasks.delete(state.current); entry = null;
  }
  if (!entry && create) {
    entry = { workspace: state.current, owner: token, task: null, result: null, busy: false, uncertain: false,
      error: "", sequence: 0, controller: null, timer: null, polls: 0, refreshed: "", expectedRevision: null };
    state.verificationTasks.set(state.current, entry);
    if (state.verificationTasks.size > 8) state.verificationTasks.delete(state.verificationTasks.keys().next().value);
  }
  return entry;
}
function suspendVerificationTasks() {
  for (const entry of state.verificationTasks.values()) {
    if (entry.busy && entry.operation === "launch") { entry.uncertain = true; entry.error = verificationTaskError(null, true); }
    entry.controller?.abort(); entry.controller = null; entry.operation = ""; clearTimeout(entry.timer); entry.timer = null;
    entry.sequence++; entry.busy = false;
  }
}
function verificationLaunchContext() {
  const project = state.project, revision = repositorySourceRevision();
  const snapshot = snapshotRevisionKey(project?.snapshot_revision);
  const digest = value => typeof value === "string" && /^sha256:[0-9a-f]{64}$/i.test(value);
  if (!token || project?.workspace !== state.current || !snapshot || snapshot.length > 16384
    || !digest(revision.code) || (revision.design != null && !digest(revision.design))
    || state.syncError || state.fitnessSnapshotFromCache || project.snapshot_refreshing === true
    || project.snapshot_unconfirmed === true || project.git_observation?.reason === "execution_disabled") return null;
  return { workspace: state.current, snapshot, revision: { ...revision } };
}
function validVerificationReceipt(data, expected = {}) {
  const text = (value, limit = 2000) => typeof value === "string" && value.length <= limit;
  const revision = value => value === null || (value && typeof value === "object"
    && /^sha256:[0-9a-f]{64}$/i.test(value.code) && (value.design == null || /^sha256:[0-9a-f]{64}$/i.test(value.design)));
  const count = value => Number.isSafeInteger(value) && value >= 0;
  const terminal = ["completed", "cancelled", "failed"].includes(data?.status);
  const sameRevision = (a, b) => a?.code === b?.code && (a?.design ?? null) === (b?.design ?? null);
  return data?.schema_version === 1 && data.kind === "verification_task" && data.tool === "verify_project"
    && text(data.task_id, 160) && /^[a-zA-Z0-9:_-]+$/.test(data.task_id)
    && data.workspace === expected.workspace && (!expected.taskId || data.task_id === expected.taskId)
    && ["working", "input_required", "completed", "cancelled", "failed"].includes(data.status)
    && data.terminal === terminal && data.completion_is_not_success === true && data.acceptance_ready === false
    && ["result_available", "authorization_required", "is_error"].every(key => typeof data[key] === "boolean")
    && text(data.status_message) && [data.created_at_ms, data.updated_at_ms, data.ttl_ms, data.poll_interval_ms].every(count)
    && data.updated_at_ms >= data.created_at_ms && data.poll_interval_ms > 0
    && revision(data.requested_revision) && revision(data.current_revision)
    && (!expected.revision || sameRevision(data.requested_revision, expected.revision))
    && ["current", "stale", "unknown"].includes(data.freshness) && ["current", "stale", "unknown"].includes(data.git_freshness)
    && (data.freshness !== "current" || (data.requested_revision !== null && data.current_revision !== null && data.git_freshness === "current" && sameRevision(data.requested_revision, data.current_revision)))
    && ["poll_status", "inspect_result_and_refresh_project", "refresh_project"].includes(data.next_action)
    && (data.error === null || text(data.error)) && (data.report === null || validVerificationTaskReport(data.report, expected.workspace));
}
function validVerificationTaskReport(report, workspace) {
  const count = value => Number.isSafeInteger(value) && value >= 0;
  const text = (value, limit) => value === null || (typeof value === "string" && value.length <= limit);
  return report && report.workspace === workspace && ["quick", "full"].includes(report.level)
    && (report.passed === null || typeof report.passed === "boolean")
    && ["checks_run", "checks_failed", "checks_reused", "elapsed_ms", "checks_total"].every(key => count(report[key]))
    && report.checks_failed <= report.checks_run && report.checks_reused <= report.checks_run
    && text(report.summary, 2000) && typeof report.checks_truncated === "boolean"
    && Array.isArray(report.skipped_checks) && report.skipped_checks.length <= 64 && report.skipped_checks.every(value => text(value, 160))
    && Array.isArray(report.checks) && report.checks.length <= 64 && report.checks_total >= report.checks.length
    && report.checks.every(row => row && text(row.id, 160) && (row.phase === null || count(row.phase))
      && (row.success === null || typeof row.success === "boolean")
      && (row.execution === null || ["executed", "unavailable", "unknown"].includes(row.execution))
      && (row.reused === null || typeof row.reused === "boolean") && (row.exit_code === null || Number.isSafeInteger(row.exit_code))
      && (row.elapsed_ms === null || count(row.elapsed_ms)) && text(row.command, 1024)
      && text(row.stdout_tail, 1024) && text(row.stderr_tail, 1024) && text(row.evidence_id, 160)
      && typeof row.output_truncated === "boolean");
}
function verificationTaskError(error, mutation = false) {
  if (error?.status === 401 || error?.code === "authorization_required") return localized("Local UI authorization is required. Open this page from wcode.", "需要本地 UI 授权，请从 wcode 打开此页面。");
  if (error?.status === 403) return localized("Execution access or exact command authorization is required. Inspect access; no permission is granted automatically.", "需要执行权限或精确命令授权。请检查权限，不会自动授权。");
  if (error?.status === 409) return localized("The bound snapshot changed or is unavailable. Refresh the project before starting another run.", "绑定快照已变化或不可用，请刷新项目后再启动验证。");
  if (error?.status === 404) return localized("Task is unknown to this workspace or local UI session. No task is substituted.", "当前工作区或本地 UI 会话无法找到此任务，不会替换成其他任务。");
  return mutation ? localized("The launch outcome is unknown. Do not submit again: inspect activity, or reconnect using the returned task ID if known.", "启动结果未知。请勿重复提交；检查活动，或在已知任务 ID 时重新连接。")
    : localized("Task observation failed. Retry status for this task; this will not start verification again.", "任务观测失败。重试此任务的状态查询，不会重新启动验证。");
}
function verificationRequest(entry) {
  entry.controller?.abort(); clearTimeout(entry.timer); entry.timer = null;
  const controller = new AbortController(), sequence = ++entry.sequence, stamp = observationStamp(), owner = token;
  entry.controller = controller; entry.operation = "observe"; entry.busy = true; entry.error = ""; renderVerificationTask();
  return { controller, current: () => !controller.signal.aborted && owner === token && observationCurrent(stamp)
    && state.verificationTasks.get(entry.workspace) === entry && entry.sequence === sequence };
}
async function startVerificationTask(level) {
  const context = verificationLaunchContext(), entry = verificationEntry(true);
  if (!context || !["quick", "full"].includes(level) || entry.busy || entry.uncertain || (entry.task && !entry.task.terminal)) return false;
  const request = verificationRequest(entry); entry.operation = "launch"; entry.expectedRevision = context.revision; entry.task = null; entry.result = null; entry.refreshed = "";
  try {
    const data = await uiJson("/intelligence/verification/run", "POST", { level,
      snapshot_revision: context.snapshot, revision: context.revision,
      timeout_seconds: level === "full" ? 1800 : 600, fail_fast: level === "quick" },
      { workspace: context.workspace, signal: request.controller.signal, timeout: 30000 });
    if (!request.current()) return false;
    if (!validVerificationReceipt(data, { workspace: context.workspace, revision: context.revision })) throw new Error("Invalid verification receipt");
    entry.task = data; entry.polls = 0; entry.uncertain = false; return true;
  } catch (error) {
    if (request.current()) {
      entry.uncertain = ![400, 401, 403, 409].includes(error?.status);
      entry.error = verificationTaskError(error, entry.uncertain);
    }
    return false;
  } finally {
    if (request.current()) { entry.controller = null; entry.busy = false; renderVerificationTask(); scheduleVerificationObservation(entry); }
  }
}
function scheduleVerificationObservation(entry) {
  clearTimeout(entry.timer); entry.timer = null;
  if (entry.busy || !entry.task || entry.error || entry.workspace !== state.current || document.hidden || state.workspaceTab !== "proof") return;
  if (entry.task.terminal) {
    if (entry.task.result_available && !entry.result) void observeVerificationTask({ result: true });
    else if (entry.refreshed !== entry.task.task_id) void refreshVerificationAcceptance(entry);
    return;
  }
  if (entry.task.status === "input_required" || entry.polls >= 120) return;
  entry.timer = setTimeout(() => { entry.timer = null; void observeVerificationTask(); }, Math.min(8000, Math.max(2000, entry.task.poll_interval_ms)));
}
async function observeVerificationTask({ result = false, manual = false, taskId } = {}) {
  const entry = verificationEntry(true), id = taskId || entry.task?.task_id;
  if (!id || !/^[a-zA-Z0-9:_-]{1,160}$/.test(id) || entry.busy) return false;
  if (manual) entry.polls = 0;
  const reconnect = Boolean(taskId), request = verificationRequest(entry);
  try {
    const data = await uiJson(`/intelligence/verification/${encodeURIComponent(id)}${result ? "/result" : ""}`, "GET", undefined,
      { workspace: entry.workspace, signal: request.controller.signal, timeout: 15000 });
    if (!request.current()) return false;
    if (!validVerificationReceipt(data, { workspace: entry.workspace, taskId: id,
      revision: reconnect ? null : entry.expectedRevision })) throw new Error("Invalid verification receipt");
    entry.task = data; entry.expectedRevision = data.requested_revision; entry.uncertain = false; entry.polls++;
    if (result) { entry.result = data; entry.refreshed = ""; }
    if (reconnect) { entry.result = null; entry.refreshed = ""; }
    return true;
  } catch (error) { if (request.current()) entry.error = verificationTaskError(error); return false; }
  finally {
    if (request.current()) {
      entry.controller = null; entry.busy = false; renderVerificationTask();
      if (result && entry.result?.terminal) await refreshVerificationAcceptance(entry);
      else scheduleVerificationObservation(entry);
    }
  }
}
async function cancelVerificationTask() {
  const entry = verificationEntry();
  if (!entry?.task || entry.task.terminal || entry.busy) return false;
  const id = entry.task.task_id, request = verificationRequest(entry);
  try {
    const data = await uiJson(`/intelligence/verification/${encodeURIComponent(id)}/cancel`, "POST", {},
      { workspace: entry.workspace, signal: request.controller.signal, timeout: 15000 });
    if (!request.current()) return false;
    if (!validVerificationReceipt(data, { workspace: entry.workspace, taskId: id, revision: entry.expectedRevision })) throw new Error("Invalid cancellation receipt");
    entry.task = data; return true;
  } catch (error) { if (request.current()) entry.error = localized("Cancellation was not confirmed. Retry this task's status before deciding whether it stopped.", "取消尚未确认，请先重试此任务状态，再判断是否已停止。"); return false; }
  finally { if (request.current()) { entry.controller = null; entry.busy = false; renderVerificationTask(); scheduleVerificationObservation(entry); } }
}
async function refreshVerificationAcceptance(entry) {
  if (entry.workspace !== state.current || state.verificationTasks.get(entry.workspace) !== entry || !entry.task?.terminal) return false;
  const stamp = observationStamp(), id = entry.task.task_id, sequence = entry.sequence;
  entry.refreshed = id; renderVerificationTask();
  const refreshed = await refreshProject({ workspace: entry.workspace, reason: "verification", force: true, preferCached: false,
    applyIf: () => observationCurrent(stamp) && state.verificationTasks.get(entry.workspace) === entry && entry.sequence === sequence && entry.task?.task_id === id });
  if (!observationCurrent(stamp) || state.verificationTasks.get(entry.workspace) !== entry || entry.sequence !== sequence) return false;
  if (!refreshed) { entry.error = localized("Verification ended, but current Evidence and Acceptance could not be refreshed. The previous decision stays stale.", "验证已结束，但无法刷新当前证据与验收；先前结论仍为过期。"); entry.refreshed = ""; }
  renderVerificationTask(); return refreshed;
}
function verificationReportHtml(receipt) {
  const report = receipt?.report;
  if (!report) return `<p class="panel-meta">${esc(localized("No typed verification report was returned; task completion does not mean Pass.", "未返回类型化验证报告；任务完成不代表通过。"))}</p>`;
  const rows = report.checks.map(row => `<details class="verification-check"><summary><code>${esc(row.id || localized("Unknown check", "未知检查"))}</code> · ${esc(statusLabel(row.execution || "unknown"))} · ${esc(row.success === null ? localized("Outcome unknown", "结果未知") : row.success ? localized("Reported pass", "报告通过") : localized("Reported failure", "报告失败"))}${row.reused ? " · " + esc(localized("Reused", "已复用")) : ""}</summary><p class="panel-meta">${esc(row.command || localized("Command unavailable", "命令不可用"))} · ${esc(localized("Exit", "退出码"))}: ${esc(row.exit_code ?? "—")} · ${num(row.elapsed_ms || 0)} ms</p>${row.evidence_id ? `<button type="button" class="acceptance-evidence-link" data-acceptance-evidence="${esc(row.evidence_id)}">${esc(row.evidence_id)}</button>` : ""}<label>stdout</label><pre>${esc(row.stdout_tail ?? localized("Output unavailable", "输出不可用"))}</pre><label>stderr</label><pre>${esc(row.stderr_tail ?? localized("Output unavailable", "输出不可用"))}</pre>${row.output_truncated ? `<p class="warn">${esc(localized("Output is bounded and truncated.", "输出有界且已截断。"))}</p>` : ""}</details>`).join("");
  return `<p>${esc(localized("Reported verification", "报告的验证"))}: ${esc(report.level)} · ${esc(report.passed === null ? localized("Unknown", "未知") : report.passed ? localized("Passed", "已通过") : localized("Failed", "失败"))} · ${num(report.checks_run)} ${esc(localized("checks", "项检查"))} · ${num(report.checks_failed)} ${esc(localized("failures", "项失败"))}</p><p class="panel-meta">${esc(localized("This report is distinct from the freshly captured canonical Acceptance decision.", "此报告与重新捕获的统一验收结论分别呈现。"))}</p>${report.skipped_checks.length ? `<p class="warn">${esc(localized("Skipped", "已跳过"))}: ${report.skipped_checks.map(esc).join(" · ")}</p>` : ""}${report.checks_truncated ? `<p class="warn">${report.checks.length} / ${report.checks_total} ${esc(localized("checks returned; remaining diagnostics are not in this bounded response.", "项检查已返回；其余诊断不在此有界响应中。"))}</p>` : ""}<div class="verification-checks">${rows}</div>`;
}
function renderVerificationTask() {
  const host = els.verificationTask; if (!host) return;
  const entry = verificationEntry(), task = entry?.task, acceptance = acceptanceView();
  const plan = acceptance.record?.plan;
  const planHtml = `<p class="panel-meta">${esc(acceptance.status === "stale" ? localized("Historical verification plan", "历史验证计划") : localized("Bound verification plan", "绑定的验证计划"))}: <code>${esc(plan?.id || localized("No plan returned", "未返回计划"))}</code>${plan ? ` · #${num(plan.verification_generation)}` : ""}</p>`;
  const active = Boolean(task && !task.terminal), disabled = !verificationLaunchContext() || entry?.busy || entry?.uncertain || active;
  const gate = !verificationLaunchContext() ? `<p class="warn">${esc(localized("Refresh a current authorized snapshot before running verification. Read-only or cached observations do not authorize execution.", "执行验证前，请刷新当前已授权快照。只读或缓存观测不能授权执行。"))}</p>` : "";
  const taskHtml = task ? `<div class="verification-task-state">${pill(statusLabel(task.status), task.status === "failed" ? "bad" : "info")}<code>${esc(task.task_id)}</code><span>${esc(statusLabel(task.freshness))} · Git: ${esc(statusLabel(task.git_freshness))}</span><p>${esc(task.status_message)}</p><p class="panel-meta">${esc(localized("Task completion is not success or Acceptance approval.", "任务完成不是成功或验收批准。"))}</p>${task.authorization_required || task.status === "input_required" ? `<p class="warn">${esc(localized("An exact operator request needs review. Approval is not applied automatically; inspect access, then query this task's actual status.", "精确操作员请求需要复核。不会自动批准；请检查权限，再查询此任务的真实状态。"))}</p><button type="button" class="quiet-action" data-summary-action="access">${esc(localized("Inspect approval", "检查批准"))}</button>` : ""}${task.freshness !== "current" ? `<p class="warn">${esc(localized("Task revision is stale or unknown; its result cannot establish current Acceptance.", "任务版本过期或未知，其结果不能建立当前验收结论。"))}</p>` : ""}<div class="acceptance-actions"><button type="button" class="quiet-action" data-verification-status ${entry.busy ? "disabled" : ""}>${esc(localized("Refresh task status", "刷新任务状态"))}</button>${active ? `<button type="button" class="quiet-action" data-verification-cancel ${entry.busy ? "disabled" : ""}>${esc(localized("Cancel verification", "取消验证"))}</button>` : ""}${task.terminal ? `<button type="button" class="quiet-action" data-verification-result ${entry.busy ? "disabled" : ""}>${esc(localized("Inspect retained result", "检查保留结果"))}</button><button type="button" class="quiet-action" data-verification-refresh>${esc(localized("Refresh Evidence / Acceptance", "刷新证据 / 验收"))}</button>` : ""}</div>${entry.polls >= 120 || document.hidden ? `<p class="panel-meta">${esc(localized("Automatic observation paused; refresh status to continue. Verification is not restarted.", "自动观测已暂停；刷新状态以继续，不会重新执行验证。"))}</p>` : ""}</div>${entry.result ? verificationReportHtml(entry.result) : ""}` : "";
  const html = `<header><h3>${esc(localized("Run native verification", "运行原生验证"))}</h3><p class="panel-meta">${esc(localized("Quick or full runs the discovered native checks through protected durable tasks. Nothing starts until you click Run.", "Quick 或 Full 通过受保护的持久化任务执行已发现的原生检查。点击运行后才会启动。"))}</p></header>${planHtml}${gate}<div class="acceptance-actions"><button type="button" class="quiet-action" data-verification-run="quick" ${disabled ? "disabled" : ""}>${esc(localized("Run quick", "运行 Quick"))}</button><button type="button" class="quiet-action" data-verification-run="full" ${disabled ? "disabled" : ""}>${esc(localized("Run full", "运行 Full"))}</button></div>${entry?.error ? `<p class="warn" role="alert">${esc(entry.error)}</p>` : ""}${entry?.uncertain || !task ? `<details class="verification-reconnect"><summary>${esc(localized("Reconnect a known task ID", "重连已知任务 ID"))}</summary><p class="panel-meta">${esc(localized("Only this workspace and local UI session's native verification tasks are accessible. If launch returned no ID, its outcome remains unknown.", "仅能访问当前工作区和本地 UI 会话的原生验证任务。若启动未返回 ID，其结果仍然未知。"))}</p><input data-verification-reconnect-id aria-label="${esc(localized("Verification task ID", "验证任务 ID"))}" maxlength="160" autocomplete="off"><button type="button" class="quiet-action" data-verification-reconnect ${entry?.busy ? "disabled" : ""}>${esc(localized("Read task status", "读取任务状态"))}</button></details>` : ""}${verificationRecoveryHtml(entry)}${taskHtml}`;
  setHtml("verificationTask", host, html, () => {
    bindSummaryActions(host); bindVerificationRecovery(host, entry);
    host.querySelectorAll("[data-verification-run]").forEach(button => button.addEventListener("click", () => void startVerificationTask(button.dataset.verificationRun)));
    host.querySelector("[data-verification-status]")?.addEventListener("click", () => void observeVerificationTask({ manual: true }));
    host.querySelector("[data-verification-result]")?.addEventListener("click", () => void observeVerificationTask({ result: true, manual: true }));
    host.querySelector("[data-verification-cancel]")?.addEventListener("click", () => void cancelVerificationTask());
    host.querySelector("[data-verification-refresh]")?.addEventListener("click", () => void refreshVerificationAcceptance(entry));
    host.querySelector("[data-verification-reconnect]")?.addEventListener("click", () => void observeVerificationTask({ manual: true, taskId: String(host.querySelector("[data-verification-reconnect-id]")?.value || "").trim() }));
    host.querySelectorAll("[data-acceptance-evidence]").forEach(button => button.addEventListener("click", () => openAcceptanceEvidence(button.dataset.acceptanceEvidence)));
  });
}
