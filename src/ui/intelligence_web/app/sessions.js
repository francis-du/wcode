function validOAuthSessions(data) {
  const optionalString = value => value === null || typeof value === "string";
  const optionalCount = value => value === null || (Number.isInteger(value) && value >= 0);
  return Boolean(
    data && typeof data === "object" && !Array.isArray(data) &&
    data.authority === "local_operator" &&
    Array.isArray(data.sessions) &&
    data.sessions.every(session =>
      session && typeof session === "object" && !Array.isArray(session) &&
      typeof session.id === "string" && /^[0-9a-f]{64}$/.test(session.id) &&
      typeof session.client_id === "string" &&
      optionalString(session.resource) &&
      Number.isInteger(session.access_count) && session.access_count >= 0 &&
      Number.isInteger(session.refresh_count) && session.refresh_count >= 0 &&
      optionalCount(session.access_expires_at_ms) &&
      optionalCount(session.refresh_expires_at_ms)
    )
  );
}
function validSessionRevocation(data) {
  return Boolean(
    data && typeof data === "object" && !Array.isArray(data) &&
    data.revoked === true &&
    Number.isInteger(data.removed_access) && data.removed_access >= 0 &&
    Number.isInteger(data.removed_refresh) && data.removed_refresh >= 0
  );
}
function sessionLabel(session) {
  const id = session.id.length > 16 ? `${session.id.slice(0, 12)}…` : session.id;
  return `${id} · ${session.client_id || "—"}`;
}
function sessionExpiry(session) {
  const access = session.access_expires_at_ms ? time(session.access_expires_at_ms) : "—";
  const refresh = session.refresh_expires_at_ms ? time(session.refresh_expires_at_ms) : "—";
  return `${t("access")} ${access} · ${t("refresh")} ${refresh}`;
}
// The timestamps say when a token ends; the status says what the session can
// still do right now. A dead refresh token ends the session, an expired access
// token only means the next refresh has to happen first.
function sessionStatus(session) {
  const now = Date.now();
  const access = session.access_expires_at_ms, refresh = session.refresh_expires_at_ms;
  if (typeof refresh === "number" && refresh <= now) {
    return { label: localized("Expired", "已过期"), tone: "warn" };
  }
  if (typeof access === "number" && access <= now) {
    return { label: localized("Access expired", "访问已过期"), tone: "info" };
  }
  if (typeof access === "number" || typeof refresh === "number") {
    return { label: localized("Active", "有效"), tone: "good" };
  }
  return null;
}
function renderSessions() {
  const unknown = localized("Session state unavailable; refresh before deciding.", "会话状态尚未取得，请刷新后再操作。");
  const sessions = state.sessions || [];
  const html = !state.sessionsLoaded
    ? `<span class="panel-meta">${esc(unknown)}</span>`
    : sessions.length
    ? sessions.map(session => {
        const confirming = state.sessionConfirm === session.id;
        const actions = confirming
          ? `<button class="danger" type="button" data-confirm-revoke="${esc(session.id)}">${esc(t("Confirm revoke"))}</button><button type="button" data-cancel-revoke="1">${esc(t("Cancel"))}</button>`
          : `<button class="danger-quiet" type="button" data-revoke-session="${esc(session.id)}" aria-label="${esc(`${t("Revoke session")} ${session.id.slice(0, 12)}`)}">${esc(t("Revoke"))}</button>`;
        const status = sessionStatus(session);
        return `<div class="session-item${confirming ? " confirming" : ""}"><div class="session-head"><code>${esc(sessionLabel(session))}</code>${pill(t("local operator"), "info")}${status ? pill(status.label, status.tone) : ""}</div><div class="session-meta">${esc(t("Client"))}: ${esc(session.client_id || "—")} · ${esc(t("Access tokens"))}: ${num(session.access_count)} · ${esc(t("Refresh tokens"))}: ${num(session.refresh_count)}</div><div class="session-meta">${esc(sessionExpiry(session))}</div><div class="session-actions">${actions}</div></div>`;
      }).join("")
    : `<span class="panel-meta">${esc(t("No active sessions"))}</span>`;
  setHtml("sessionList", els.sessionList, html, () => {
    els.sessionList.querySelectorAll("[data-revoke-session]").forEach(button =>
      button.addEventListener("click", () => armSessionRevoke(button.dataset.revokeSession)));
    els.sessionList.querySelectorAll("[data-confirm-revoke]").forEach(button =>
      button.addEventListener("click", () => revokeSessionFromUi(button.dataset.confirmRevoke)));
    els.sessionList.querySelectorAll("[data-cancel-revoke]").forEach(button =>
      button.addEventListener("click", cancelSessionRevoke));
  });
  if (!els.sessionMessage.dataset.result) els.sessionMessage.textContent = t("session safety note");
  if (els.sessionRefresh) els.sessionRefresh.disabled = state.sessionsBusy;
}
function sessionMessage(node, text) {
  node.dataset.result = "1";
  node.textContent = text;
}
function armSessionRevoke(sessionId) {
  if (state.sessionsBusy || typeof sessionId !== "string") return;
  state.sessionConfirm = sessionId;
  renderSessions();
}
function cancelSessionRevoke() {
  if (state.sessionsBusy) return;
  state.sessionConfirm = null;
  renderSessions();
}
async function loadSessions() {
  if (state.sessionsBusy) return false;
  state.sessionsBusy = true;
  renderSessions();
  try {
    const data = await uiJson("/oauth/sessions", "GET");
    if (!validOAuthSessions(data)) throw new Error("Invalid session response");
    state.sessions = data.sessions;
    state.sessionsLoaded = true;
    renderSessions();
    return true;
  } catch (error) {
    console.warn("wcode: session refresh failed", error);
    sessionMessage(els.sessionMessage, `${t("Unable to update sessions")}: ${requestFailureMessage(error)}`);
    renderSessions();
    return false;
  } finally {
    state.sessionsBusy = false;
    renderSessions();
  }
}
async function revokeSessionFromUi(sessionId) {
  // Revocation is explicit: the first click only arms the confirmation.
  if (state.sessionsBusy || state.sessionConfirm !== sessionId) return;
  await submitSessionRevoke(sessionId);
}
async function submitSessionRevoke(sessionId) {
  if (state.sessionsBusy) return null;
  state.sessionsBusy = true;
  state.sessionConfirm = null;
  renderSessions();
  let result = null;
  try {
    const data = await uiJson("/oauth/sessions/revoke", "POST", { session_id: sessionId });
    if (!validSessionRevocation(data)) throw new Error("Invalid session revocation response");
    result = data;
    if (!data.removed_access && !data.removed_refresh) {
      sessionMessage(els.sessionMessage, t("No matching session; it may already be revoked"));
    } else {
      sessionMessage(els.sessionMessage, `${t("Session revoked")} · ${t("Access tokens")}: ${num(data.removed_access)} · ${t("Refresh tokens")}: ${num(data.removed_refresh)} · ${t("persisted")}`);
    }
  } catch (error) {
    console.warn("wcode: session revocation failed", error);
    const detail = String(error?.message || "");
    if (detail.includes("revocation_not_persisted")) {
      sessionMessage(els.sessionMessage, t("Live credentials removed, but durable revocation was not confirmed; restart may restore this session"));
    } else if (detail.includes("invalid_session_id")) {
      sessionMessage(els.sessionMessage, `${t("Revocation failed")}: ${t("invalid session id")}`);
    } else {
      sessionMessage(els.sessionMessage, `${t("Revocation failed")}: ${requestFailureMessage(error)}`);
    }
    result = null;
  } finally {
    state.sessionsBusy = false;
    renderSessions();
  }
  // Refresh the list only after the mutation is settled so the busy guard
  // cannot suppress the read that proves what actually changed.
  await loadSessions();
  return result;
}
