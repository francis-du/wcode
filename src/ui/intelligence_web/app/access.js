function renderAccess(force = false) {
  const allowed = state.access?.allowed_commands || [];
  const workspaceOptions = state.workspaceAccess?.workspace_options || [];
  const unknown = localized("Access state unavailable; refresh before deciding.", "授权状态尚未取得，请刷新后再操作。");
  if (force) invalidate("workspaceAccess", "commandAccess", "authorizationAccess");
  setHtml("workspaceAccess", els.workspaceList, workspaceOptions.length
    ? workspaceOptions.map(item => `<span class="workspace-chip"><code>${esc(item.id)}</code><span class="panel-meta">${esc(item.root || "")}</span></span>`).join("")
    : `<span class="panel-meta">${esc(state.workspaceAccess ? t("No authorized projects") : unknown)}</span>`);
  setHtml("commandAccess", els.commandList, allowed.length
    ? allowed.map(program => `<span class="command-chip"><code>${esc(program)}</code><button type="button" data-revoke-command="${esc(program)}" aria-label="${esc(`${t("Revoke")} ${program}`)}">${uiIcon("close")}</button></span>`).join("")
    : `<span class="panel-meta">${esc(state.access ? t("No commands authorized") : unknown)}</span>`, () => {
      els.commandList.querySelectorAll("[data-revoke-command]").forEach(button =>
        button.addEventListener("click", () => revokeCommandFromUi(button.dataset.revokeCommand)));
    });
  setHtml("authorizationAccess", els.authorizationList, !state.accessLoaded
    ? `<span class="panel-meta">${esc(unknown)}</span>`
    : state.authorizations.length
    ? state.authorizations.map(request => `<div class="authorization-item"><div class="authorization-head"><code>${esc(request.id)}</code>${pill(authorizationKind(request.kind), "warn")}</div><div class="authorization-summary">${esc(request.summary)}</div><div class="authorization-meta">${esc(request.workspace)}${request.program ? ` · ${esc(request.program)}` : ""}</div><div class="authorization-actions"><button class="approve" type="button" data-approve-authorization="${esc(request.id)}">${esc(t("Approve"))}</button><button class="deny" type="button" data-deny-authorization="${esc(request.id)}">${esc(t("Deny"))}</button></div></div>`).join("")
    : `<span class="panel-meta">${esc(t("No pending authorizations"))}</span>`, () => {
      els.authorizationList.querySelectorAll("[data-approve-authorization]").forEach(button =>
        button.addEventListener("click", () => decideAuthorization(button.dataset.approveAuthorization, true)));
      els.authorizationList.querySelectorAll("[data-deny-authorization]").forEach(button =>
        button.addEventListener("click", () => decideAuthorization(button.dataset.denyAuthorization, false)));
    });
  const allCommands = state.access?.all_commands_authorized === true;
  if (els.allCommandsStatus) els.allCommandsStatus.textContent = t(allCommands ? "All commands authorized for this session" : "All commands require per-request approval");
  if (els.allCommandsToggle) {
    els.allCommandsToggle.textContent = t(allCommands ? "Disable all command authorization" : "Authorize all commands");
    els.allCommandsToggle.classList.toggle("danger-toggle", allCommands);
    els.allCommandsToggle.setAttribute("aria-pressed", allCommands ? "true" : "false");
  }
  if (!els.commandMessage.dataset.result) els.commandMessage.textContent = t("command safety note");
  if (!els.authorizationMessage.dataset.result) els.authorizationMessage.textContent = t("authorization safety note");
  setAccessBusy(state.accessBusy);
}
function setAccessBusy(busy) {
  for (const button of [els.addWorkspace, els.addCommand, els.authorizeOperation, els.allCommandsToggle, els.workspace].filter(Boolean)) button.disabled = busy;
  els.commandList.querySelectorAll("button").forEach(button => { button.disabled = busy; });
  els.authorizationList.querySelectorAll("button").forEach(button => { button.disabled = busy || !state.accessLoaded; });
}
function accessMessage(node, text) {
  node.dataset.result = "1";
  node.textContent = text;
}
function accessOperationCurrent(op) {
  return observationCurrent(op.stamp) && op.stamp.mutation === state.accessMutationEpoch;
}
// One owner for all access mutations. The workspace generation also rejects
// delayed A→B→A responses; a matching workspace string alone is insufficient.
async function mutateAccess(message, work) {
  if (state.accessBusy) return null;
  state.accessBusy = true;
  state.accessEpoch++;
  state.accessMutationEpoch++;
  const op = { stamp: observationStamp() };
  state.accessOperation = op;
  setAccessBusy(true);
  try {
    return await work(op);
  } catch (error) {
    if (accessOperationCurrent(op)) {
      if (error.uncertain) {
        state.accessLoaded = false;
        state.access = null;
        state.authorizations = [];
        renderAccess(true);
      }
      console.warn("wcode: access mutation failed", error);
      accessMessage(message, `${t("Unable to update access")}: ${requestFailureMessage(error)}`);
    }
    return null;
  } finally {
    if (state.accessOperation === op) {
      state.accessBusy = false;
      state.accessOperation = null;
      setAccessBusy(false);
    }
  }
}
async function loadAccess() {
  if (state.accessBusy) return false;
  const workspace = state.current, view = state.workspaceEpoch;
  if (state.accessRead?.workspace === workspace && state.accessRead.view === view &&
      state.accessRead.epoch === state.accessEpoch) return state.accessRead.promise;
  const read = { workspace, view, epoch: ++state.accessEpoch, stamp: observationStamp() };
  state.accessRead = read;
  read.promise = (async () => {
    try {
      const options = { workspace };
      const [workspaceAccess, commands, authorizations] = await Promise.all([
        uiJson("/intelligence/workspaces", "GET", undefined, options),
        uiJson("/intelligence/commands", "GET", undefined, options),
        uiJson("/intelligence/authorizations", "GET", undefined, options),
      ]);
      if (!observationCurrent(read.stamp) || read.epoch !== state.accessEpoch) return false;
      if (!Array.isArray(authorizations.pending)) throw new Error("Invalid authorization response");
      state.workspaceAccess = workspaceAccess;
      state.access = commands;
      state.authorizations = authorizations.pending;
      state.accessLoaded = true;
      observePending(state.authorizations.length, read.stamp);
      renderAccess(); renderStats(); renderAttention();
      return true;
    } catch (error) {
      if (!observationCurrent(read.stamp) || read.epoch !== state.accessEpoch) return false;
      state.accessLoaded = false;
      state.authorizations = [];
      state.access = null;
      state.workspaceAccess = null;
      renderAccess(true);
      console.warn("wcode: access refresh failed", error);
      accessMessage(els.authorizationMessage, `${t("Unable to update access")}: ${requestFailureMessage(error)}`);
      return false;
    } finally {
      if (state.accessRead === read) state.accessRead = null;
    }
  })();
  return read.promise;
}
async function addWorkspaceFromUi() {
  const root = els.workspacePath.value.trim();
  if (!root || state.accessBusy) return;
  const result = await mutateAccess(els.workspaceMessage, async op => {
    const data = await uiJson("/intelligence/workspaces", "POST", { root }, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return null;
    if (typeof data.workspace?.id !== "string") throw new Error("Invalid workspace response");
    els.workspacePath.value = "";
    accessMessage(els.workspaceMessage, `${t("Workspace added")}: ${data.workspace.id}`);
    return { workspace: data.workspace.id, stamp: op.stamp };
  });
  if (result && observationCurrent(result.stamp)) {
    await refreshProject({ workspace: result.workspace, reason: "manual", force: true });
    await loadAccess();
  }
}
async function addCommandFromUi() {
  const program = els.commandCandidate.value.trim();
  if (!program || state.accessBusy) return;
  await mutateAccess(els.commandMessage, async op => {
    const data = await uiJson("/intelligence/commands", "POST", { program }, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return;
    state.access = data;
    els.commandCandidate.value = "";
    accessMessage(els.commandMessage, `${t("Command authorized")}: ${program}`);
    renderAccess();
  });
}
async function revokeCommandFromUi(program) {
  if (state.accessBusy) return;
  await mutateAccess(els.commandMessage, async op => {
    const data = await uiJson("/intelligence/commands", "DELETE", { program }, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return;
    state.access = data;
    accessMessage(els.commandMessage, `${t("Command revoked")}: ${program}`);
    renderAccess();
  });
}
async function toggleAllCommandsFromUi() {
  if (state.accessBusy || !state.access) return;
  const enable = state.access.all_commands_authorized !== true;
  await mutateAccess(els.commandMessage, async op => {
    const data = await uiJson("/intelligence/command-trust", enable ? "POST" : "DELETE", undefined, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return;
    state.access = data;
    if (enable) {
      state.authorizations = state.authorizations.filter(request => request.kind === "destructive_delete");
      observePending(state.authorizations.length, op.stamp);
    }
    accessMessage(els.commandMessage, t(enable ? "All command authorization enabled" : "All command authorization disabled"));
    renderAccess(); renderStats(); renderAttention();
  });
}
function parseOperationArgs(value) {
  const input = value.trim();
  if (!input) return [];
  if (input.startsWith("[")) {
    const args = JSON.parse(input);
    if (!Array.isArray(args) || !args.every(arg => typeof arg === "string")) throw new Error(localized("Arguments must be a JSON array of strings.", "参数必须为字符串组成的 JSON 数组。"));
    return args;
  }
  if (/["'\\]/.test(input)) throw new Error(localized("Use a JSON array to preserve spaces, quotes and empty arguments.", "带空格、引号或空参数时，请使用 JSON 数组以保持原意。"));
  return input.split(/\s+/);
}
async function authorizeOperationFromUi() {
  const program = els.operationProgram.value.trim(), cwd = els.operationCwd.value.trim() || ".";
  if (!program || state.accessBusy) return;
  await mutateAccess(els.operationMessage, async op => {
    const args = parseOperationArgs(els.operationArgs.value);
    const data = await uiJson("/intelligence/command-operations", "POST", { program, args, cwd }, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return;
    state.access = data.workspace || state.access;
    if (Array.isArray(data.pending)) {
      state.authorizations = data.pending;
      state.accessLoaded = true;
      observePending(data.pending.length, op.stamp);
    }
    accessMessage(els.operationMessage, `${t("Operation authorized")}: ${program} ${JSON.stringify(args)} · ${cwd}`);
    renderAccess(); renderStats(); renderAttention();
  });
}
async function decideAuthorization(id, approve) {
  if (state.accessBusy || !state.accessLoaded) return;
  let resumeSemantic = false;
  await mutateAccess(els.authorizationMessage, async op => {
    const data = await uiJson("/intelligence/authorizations", approve ? "POST" : "DELETE", { id }, { workspace: op.stamp.workspace });
    if (!accessOperationCurrent(op)) return;
    if (!Array.isArray(data.pending)) throw new Error("Invalid authorization response");
    state.authorizations = data.pending;
    state.accessLoaded = true;
    observePending(data.pending.length, op.stamp);
    const status = data.request?.status ? ` · ${statusLabel(data.request.status)}` : "";
    accessMessage(els.authorizationMessage, `${t(approve ? "Authorization approved" : "Authorization denied")}: ${id}${status}`);
    // The mutation is already acknowledged. Read failure must not erase that
    // fact or automatically replay the approval.
    try {
      const commands = await uiJson("/intelligence/commands", "GET", undefined, { workspace: op.stamp.workspace });
      if (accessOperationCurrent(op)) state.access = commands;
    } catch { if (accessOperationCurrent(op)) state.access = null; }
    if (!accessOperationCurrent(op)) return;
    renderAccess(); renderStats(); renderAttention();
    resumeSemantic = approve && state.semanticRefreshPending;
  });
  if (resumeSemantic) queueMicrotask(refreshSemantics);
}
