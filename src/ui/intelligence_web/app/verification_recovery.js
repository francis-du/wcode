// Lost launch replies are recovered from server-owned records, never guessed or rerun.
async function discoverVerificationTasks() {
  const entry = verificationEntry(true);
  if (!token || entry.busy) return false;
  const request = verificationRequest(entry);
  try {
    const data = await uiJson("/intelligence/verification/tasks", "GET", undefined,
      { workspace: entry.workspace, signal: request.controller.signal, timeout: 15000 });
    if (!request.current()) return false;
    if (!validRetainedTaskList(data, entry.workspace, true)) throw new Error("Invalid task discovery");
    entry.discovered = data.items; entry.discoveryTruncated = data.truncated; entry.discoveryLoaded = true;
    // Discovery alone does not resolve a lost POST, select a task, or authorize a retry.
    return true;
  } catch (error) {
    if (request.current()) {
      entry.discovered = []; entry.discoveryLoaded = false;
      entry.error = verificationTaskError(error);
    }
    return false;
  } finally { if (request.current()) { entry.controller = null; entry.busy = false; renderVerificationTask(); } }
}
function verificationRecoveryHtml(entry) {
  const rows = (entry?.discovered || []).map(row => `<button type="button" class="quiet-action" data-verification-discovered="${esc(row.task_id)}" ${entry.busy ? "disabled" : ""}><code>${esc(row.task_id)}</code> · ${esc(statusLabel(row.status))}</button>`).join("");
  return `<section class="verification-recovery"><button type="button" class="quiet-action" data-verification-discover ${!token || entry?.busy ? "disabled" : ""}>${esc(localized("Find retained verification tasks", "发现保留的验证任务"))}</button><p class="panel-meta">${esc(localized("Discover real IDs owned by this workspace and UI session. Select a record to read it; discovery never starts verification.", "发现此工作区和 UI 会话所属的真实 ID。选择记录后读取；发现操作不会启动验证。"))}</p>${entry?.discoveryLoaded && !rows ? `<p>${esc(localized("No matching records in this bounded response. A lost launch remains unknown; do not repeat it automatically.", "此有界响应中没有匹配记录。丢失回执的启动仍然未知，不会自动重试。"))}</p>` : ""}${entry?.discoveryTruncated ? `<p class="warn">${esc(localized("Discovery is partial; some retained records are not shown.", "发现结果不完整，部分保留记录未显示。"))}</p>` : ""}${rows}</section>`;
}
function bindVerificationRecovery(host, entry) {
  host.querySelector("[data-verification-discover]")?.addEventListener("click", () => void discoverVerificationTasks());
  host.querySelectorAll("[data-verification-discovered]").forEach(button => button.addEventListener("click", () => {
    const id = button.dataset.verificationDiscovered;
    if (entry?.discovered?.some(row => row.task_id === id)) void observeVerificationTask({ taskId: id, manual: true });
  }));
}
