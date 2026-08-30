function renderAccess(force = false) {
  const access = state.access || {},
    allowed = access.allowed_commands || [],
    workspaceOptions = state.workspaceAccess?.workspace_options || [];
  if (force) {
    invalidate("workspaceAccess", "commandAccess", "authorizationAccess");
  }
  setHtml(
    "workspaceAccess",
    els.workspaceList,
    workspaceOptions.length
      ? workspaceOptions.map((item) =>
        `<span class="workspace-chip"><code>${
          esc(item.id)
        }</code><span class="panel-meta">${esc(item.root || "")}</span></span>`
      ).join("")
      : `<span class="panel-meta">${esc(t("No authorized projects"))}</span>`,
  );
  setHtml(
    "commandAccess",
    els.commandList,
    allowed.length
      ? allowed.map((program) =>
        `<span class="command-chip"><code>${
          esc(program)
        }</code><button type="button" data-revoke-command="${
          esc(program)
        }" title="${esc(t("Revoke"))}">×</button></span>`
      ).join("")
      : `<span class="panel-meta">${esc(t("No commands authorized"))}</span>`,
    () =>
      els.commandList.querySelectorAll("[data-revoke-command]").forEach(
        (button) =>
          button.addEventListener(
            "click",
            () => revokeCommandFromUi(button.dataset.revokeCommand),
          ),
      ),
  );
  setHtml(
    "authorizationAccess",
    els.authorizationList,
    state.authorizations.length
      ? state.authorizations.map((request) =>
        `<div class="authorization-item"><div class="authorization-head"><code>${
          esc(request.id)
        }</code>${
          pill(authorizationKind(request.kind), "warn")
        }</div><div class="authorization-summary">${
          esc(request.summary)
        }</div><div class="authorization-meta">${esc(request.workspace)}${
          request.program ? ` · ${esc(request.program)}` : ""
        }</div><div class="authorization-actions"><button class="approve" type="button" data-approve-authorization="${
          esc(request.id)
        }">${
          esc(t("Approve"))
        }</button><button class="deny" type="button" data-deny-authorization="${
          esc(request.id)
        }">${esc(t("Deny"))}</button></div></div>`
      ).join("")
      : `<span class="panel-meta">${
        esc(t("No pending authorizations"))
      }</span>`,
    () => {
      els.authorizationList.querySelectorAll("[data-approve-authorization]")
        .forEach((button) =>
          button.addEventListener(
            "click",
            () =>
              decideAuthorization(button.dataset.approveAuthorization, true),
          )
        );
      els.authorizationList.querySelectorAll("[data-deny-authorization]")
        .forEach((button) =>
          button.addEventListener(
            "click",
            () => decideAuthorization(button.dataset.denyAuthorization, false),
          )
        );
    },
  );
  if (!els.commandMessage.dataset.result) {
    els.commandMessage.textContent = t("command safety note");
  }
  if (!els.authorizationMessage.dataset.result) {
    els.authorizationMessage.textContent = t("authorization safety note");
  }
}
async function loadAccess() {
  try {
    const [workspaceAccess, commands, authorizations] = await Promise.all([
      uiJson("/intelligence/workspaces"),
      uiJson("/intelligence/commands"),
      uiJson("/intelligence/authorizations"),
    ]);
    state.workspaceAccess = workspaceAccess;
    state.access = commands;
    state.authorizations = authorizations.pending || [];
    state.accessLoaded = true;
    if (state.project) {
      state.project.pending_authorizations = state.authorizations.length;
    }
    els.commandMessage.dataset.result = "";
    els.authorizationMessage.dataset.result = "";
    renderAccess();
    renderAttention();
  } catch (error) {
    els.commandMessage.dataset.result = "1";
    els.commandMessage.textContent = `${
      t("Unable to update access")
    }: ${error.message}`;
  }
}
async function addWorkspaceFromUi() {
  const root = els.workspacePath.value.trim();
  if (!root) return;
  els.addWorkspace.disabled = true;
  try {
    const data = await uiJson("/intelligence/workspaces", "POST", { root });
    state.current = data.workspace.id;
    state.selected = "";
    els.workspacePath.value = "";
    els.workspaceMessage.textContent = `${
      t("Workspace added")
    }: ${data.workspace.id}`;
    await refreshProject({
      workspace: state.current,
      reason: "manual",
      force: true,
    });
    await loadAccess();
  } catch (error) {
    els.workspaceMessage.textContent = `${
      t("Unable to update access")
    }: ${error.message}`;
  } finally {
    els.addWorkspace.disabled = false;
  }
}
async function addCommandFromUi() {
  const program = els.commandCandidate.value.trim();
  if (!program) return;
  els.addCommand.disabled = true;
  try {
    state.access = await uiJson("/intelligence/commands", "POST", { program });
    els.commandCandidate.value = "";
    els.commandMessage.dataset.result = "1";
    els.commandMessage.textContent = `${t("Command authorized")}: ${program}`;
    renderAccess();
  } catch (error) {
    els.commandMessage.dataset.result = "1";
    els.commandMessage.textContent = `${
      t("Unable to update access")
    }: ${error.message}`;
  } finally {
    els.addCommand.disabled = false;
  }
}
async function revokeCommandFromUi(program) {
  try {
    state.access = await uiJson("/intelligence/commands", "DELETE", {
      program,
    });
    els.commandMessage.dataset.result = "1";
    els.commandMessage.textContent = `${t("Command revoked")}: ${program}`;
    renderAccess();
  } catch (error) {
    els.commandMessage.dataset.result = "1";
    els.commandMessage.textContent = `${
      t("Unable to update access")
    }: ${error.message}`;
  }
}
async function authorizeOperationFromUi() {
  const program = els.operationProgram.value.trim(),
    cwd = els.operationCwd.value.trim() || ".";
  if (!program) return;
  const args = els.operationArgs.value.trim()
    ? els.operationArgs.value.trim().split(/\s+/)
    : [];
  els.authorizeOperation.disabled = true;
  try {
    const data = await uiJson("/intelligence/command-operations", "POST", {
      program,
      args,
      cwd,
    });
    state.access = data.workspace || state.access;
    state.authorizations = data.pending || state.authorizations;
    els.operationMessage.dataset.result = "1";
    els.operationMessage.textContent = `${
      t("Operation authorized")
    }: ${program}${args.length ? " " + args.join(" ") : ""} · ${cwd}`;
    renderAccess();
    renderAttention();
  } catch (error) {
    els.operationMessage.dataset.result = "1";
    els.operationMessage.textContent = `${
      t("Unable to update access")
    }: ${error.message}`;
  } finally {
    els.authorizeOperation.disabled = false;
  }
}
async function decideAuthorization(id, approve) {
  try {
    const data = await uiJson(
      "/intelligence/authorizations",
      approve ? "POST" : "DELETE",
      { id },
    );
    state.authorizations = data.pending || [];
    state.accessLoaded = true;
    if (state.project) {
      state.project.pending_authorizations = state.authorizations.length;
    }
    state.access = await uiJson("/intelligence/commands");
    els.authorizationMessage.dataset.result = "1";
    const status = data.request?.status
      ? ` · ${statusLabel(data.request.status)}`
      : "";
    els.authorizationMessage.textContent = `${
      t(approve ? "Authorization approved" : "Authorization denied")
    }: ${id}${status}`;
    renderAccess();
    renderAttention();
    if (approve && state.semanticRefreshPending) {
      queueMicrotask(refreshSemantics);
    }
  } catch (error) {
    els.authorizationMessage.dataset.result = "1";
    els.authorizationMessage.textContent = `${
      t("Unable to update authorizations")
    }: ${error.message}`;
  }
}
