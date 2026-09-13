function graphPrecision() {
  return state.project?.graph_precision || { primary: "unknown", providers: [], semantic_edges: 0, runtime_edges: 0, syntax_edges: 0 };
}
function semanticAvailable() {
  return (state.project?.language_quality?.languages || []).some(language => Number(language.detected_files || 0) > 0 && language.semantic_available);
}
function renderLive() {
  const p = state.project;
  if (!p) return;
  const precision = graphPrecision(), primary = precision.primary || "unknown";
  els.precisionBadge.className = `precision-badge ${["semantic", "runtime", "syntax"].includes(primary) ? primary : ""}`;
  els.precisionBadge.textContent = statusLabel(primary);
  if (els.precisionProviders) {
    const providers = (precision.providers || []).filter(item => item !== "design-state"),
      sourceItems = [...providers, "wcode-design"].filter((item, index, all) => item && all.indexOf(item) === index),
      evidence = [
        `${num(precision.semantic_edges || 0)} ${localized("semantic", "语义")}`,
        `${num(precision.runtime_edges || 0)} ${localized("runtime", "运行时")}`,
        `${num(precision.syntax_edges || 0)} ${localized("syntax", "语法")}`,
      ].join(" · "),
      visibleSources = sourceItems.length <= 2
        ? sourceItems
        : [sourceItems[0], sourceItems[sourceItems.length - 1]].filter((item, index, all) => item && all.indexOf(item) === index),
      hiddenSources = Math.max(0, sourceItems.length - visibleSources.length),
      chips = visibleSources.map(item => `<span class="context-chip">${esc(item)}</span>`),
      overflowChip = hiddenSources ? `<span class="context-chip context-more">+${hiddenSources}</span>` : "",
      sourceHtml = [...chips, overflowChip].filter(Boolean).join('<span class="context-sep" aria-hidden="true"></span>');
    setHtml("precisionProviders", els.precisionProviders, sourceHtml || `<span class="context-chip">${esc(localized("No provider reported", "暂无数据源"))}</span>`);
    els.precisionProviders.title = `${statusLabel(primary)}${sourceItems.length ? ` · ${sourceItems.join(" · ")}` : ""} · ${evidence}`;
  }
  const age = state.lastUpdated ? Math.max(0, Math.floor((Date.now() - state.lastUpdated) / 1000)) : null;
  const updated = state.lastUpdated ? time(state.lastUpdated) : "—";
  els.lastUpdated.textContent = localized(
    `Last updated  ${updated}   ·   Snapshot  ${age === null ? "—" : `${age}s ago`}`,
    `更新于  ${updated}   ·   快照  ${age === null ? "—" : `${age} 秒前`}`,
  );
  els.lastUpdated.title = `${localized("Activity refresh 2–8s · project refresh 8s", "活动刷新 2–8 秒 · 项目刷新 8 秒")} · ${state.syncError ? localized("snapshot stale", "快照已过期") : !state.autoRefresh || document.hidden ? localized("live refresh paused", "实时刷新已暂停") : localized("live refresh active", "实时刷新中")}`;
}
function stat(label, value, detail, tone = "", target = "") {
  const accessible = `${label}: ${value}. ${detail}`;
  return `<button type="button" class="stat ${tone}" data-summary-action="${esc(target)}" aria-label="${esc(accessible)}" title="${esc(detail)}"><span class="k">${esc(label)}</span><strong class="v">${esc(value)}</strong></button>`;
}
function pendingCount() {
  return state.pendingValue ?? state.project?.pending_authorizations ?? state.activitySnapshot?.pending_authorizations ?? null;
}
function effectiveProof() {
  const proof = state.project?.proof || {}, effective = proof.effective;
  return effective ? { ...proof, current_evidence: effective.total,
    current_failed: effective.failed, current_passed: effective.passed,
    current_inconclusive: effective.inconclusive, current_disagreed: effective.disagreed } : proof;
}
function bindSummaryActions(node) {
  node.querySelectorAll("[data-summary-action]").forEach(button => button.addEventListener("click", async () => {
    const target = button.dataset.summaryAction;
    if (target === "access") { setAccessPanel(true); await loadAccess(); }
    else if (target) revealSection(target);
  }));
}
function renderStats() {
  const p = state.project;
  if (!p) return;
  const activity = state.activitySnapshot?.activity || p.activity, proof = effectiveProof(), pending = pendingCount();
  const knownActivity = activity?.available === true && !state.activityError;
  const reviewKnown = p.git_review?.available === true;
  const items = [
    stat(localized("Executing now", "正在执行"), knownActivity ? num(activity.active) : "—", knownActivity ? localized(`${num(activity.queued)} queued · ${num(activity.orchestration)} coordinators`, `${num(activity.queued)} 项排队 · ${num(activity.orchestration)} 项编排`) : localized("Activity unavailable, not idle", "活动数据不可用，不代表空闲"), "", "activitySection"),
    stat(localized("Needs your approval", "等待你批准"), pending === null ? "—" : num(pending), localized("Exact requests for this project", "仅当前项目的精确授权请求"), pending ? "warn" : "", "access"),
    stat(localized("Working-tree changes", "工作区变更"), reviewKnown ? num(p.code?.changed_files) : localized("Unknown", "未知"), reviewKnown ? localized("Files changed, not completed features", "文件变更数量，不代表已完成功能") : localized("Git review did not complete", "Git 检查未完成"), reviewKnown ? "" : "warn", "changesSection"),
    stat(localized("Current-version evidence", "当前版本证据"), proof.current_evidence ? num(proof.current_evidence) : localized("Not verified", "尚未验证"), proof.current_evidence ? localized(`${num(proof.current_failed)} effective failures · ${num(proof.current_inconclusive)} inconclusive`, `${num(proof.current_failed)} 项有效失败 · ${num(proof.current_inconclusive)} 项未定结论`) : localized("Mapping is not a passing test", "已映射 ≠ 测试通过"), proof.current_failed ? "bad" : "", "proofSection"),
  ];
  setHtml("stats", els.stats, items.join(""), () => bindSummaryActions(els.stats));
}
function attentionSignals() {
  const p = state.project, items = [];
  if (!p) return items;
  const proof = effectiveProof(), conv = p.convergence || {}, pending = pendingCount();
  const add = (tone, title, detail, target) => items.push({ tone, title, detail, target });
  if (state.syncError) add("bad", localized("Snapshot is stale", "快照已过期"), localized("The last refresh failed. Do not judge current state from these numbers.", "最近刷新失败，请勿把下方旧数据当作当前状态。"), "");
  if (p.design_valid === false) add("bad", localized("Design needs attention", "设计状态需要处理"), localized("Missing or invalid design; inspect requirements and diagnostics.", "设计未初始化或校验失败，请查看需求与诊断。"), "requirementsSection");
  const oversized = Number(p.structure?.oversized_files || 0), lineLimit = Number(p.structure?.line_limit || 1000);
  if (oversized) add("bad", localized(`${num(oversized)} source files violate the hard line limit`, `${num(oversized)} 个源文件违反硬性行数限制`), localized(`Core policy blocks verification and further growth above ${num(lineLimit)} lines until those modules are decomposed by responsibility.`, `核心策略会阻断验证，并禁止超过 ${num(lineLimit)} 行的模块继续增长；请先按职责拆分。`), "filesSection");
  if (proof.current_failed) add("bad", localized("Failure evidence recorded", "存在失败证据"), localized(`${num(proof.current_failed)} records for this version. Inspect the latest verification, not the count alone.`, `当前版本有 ${num(proof.current_failed)} 条失败记录，请核对最新验证结果。`), "proofSection");
  if (p.architecture?.blocking_drift_edges) add("bad", localized("Confirmed architecture drift", "已确认架构偏离"), localized(`${num(p.architecture.blocking_drift_edges)} dependencies have strong drift evidence.`, `${num(p.architecture.blocking_drift_edges)} 条依赖具有强证据偏离。`), "architectureSection");
  if (pending) add("warn", localized(`${num(pending)} requests need approval`, `${num(pending)} 项请求等待批准`), localized("Review the exact operation before allowing it.", "检查具体操作后再决定批准或拒绝。"), "access");
  if (proof.current_verification_blocked) add("warn", localized("Verification plans are blocked", "验证计划尚未就绪"), localized(`${num(proof.current_verification_blocked)} current plans still have gates to satisfy.`, `${num(proof.current_verification_blocked)} 个当前计划仍有门禁未满足。`), "proofSection");
  if (!proof.current_evidence) add("info", localized("This version is not verified yet", "当前版本尚未验证"), localized("No current-version evidence was returned. Historical passes and mapped tests are not proof.", "没有返回当前版本证据。历史通过和测试映射都不能代替本次验证。"), "proofSection");
  else if (proof.current_disagreed || proof.current_inconclusive) add("warn", localized("Evidence has unresolved conclusions", "证据仍有未定结论"), localized("Resolve disagreement or incomplete checks before claiming completion.", "先处理分歧与不完整检查，再判断是否完成。"), "proofSection");
  if (p.git_review?.available !== true) add("info", localized("Working-tree status is unknown", "工作树状态未知"), gitReviewReason(p.git_review?.reason), "changesSection");
  const risks = (p.risk?.risks || []).filter(item => ["critical", "high"].includes(item.level));
  if (risks.length) add(risks.some(item => item.level === "critical") ? "bad" : "warn", localized(`${risks.length} high-priority risk signals`, `${risks.length} 项高优先级风险信号`), risks[0].summary || localized("Inspect the recorded risk reasons.", "查看已记录的风险原因。"), "diagnosticsSection");
  const needs = Number(conv.needs_convergence_requirements || 0) + Number(conv.incomplete_requirements || 0);
  if (needs) add("warn", localized(`${needs} requirements need work`, `${needs} 项需求需要处理`), localized("Inspect incomplete implementation and dependency evidence.", "查看不完整实现和依赖证据。"), "requirementsSection");
  if (p.code?.graph_truncated || proof.evidence_scan_truncated) add("info", localized("Partial snapshot", "快照不完整"), localized("A bounded scan omitted data; an absent item is not proof of absence.", "有界扫描省略了部分数据；未展示不代表不存在。"), "diagnosticsSection");
  if (!items.length) add("info", localized("No known blockers in this snapshot", "此快照中未发现已知阻塞"), localized("This is an observation, not a release approval. Review evidence and changes below.", "这是观测结果，不是发布批准。请结合证据和变更判断。"), "proofSection");
  if (state.activityError) add("warn", localized("Activity telemetry is stale", "活动遥测已过期"), localized("Task status could not be refreshed; it is not an idle signal.", "任务状态刷新失败，不代表系统空闲。"), "activitySection");
  const priority = { bad: 0, warn: 1, info: 2, good: 3 };
  return items.sort((left, right) => priority[left.tone] - priority[right.tone]);
}
function gitReviewReason(reason) {
  return ({ execution_disabled: localized("Command execution is disabled.", "命令执行已禁用。"), not_a_repository: localized("No Git repository was found at this root.", "当前根目录不是 Git 仓库。"), partial_review: localized("Some Git probes failed; the review is partial.", "部分 Git 检查失败，结果不完整。"), review_failed: localized("Git review failed; retry or inspect permissions.", "Git 检查失败，请重试或检查权限。") })[reason] || localized("Review data was not returned.", "没有返回检查数据。");
}
function attentionItem(item) {
  return `<button type="button" class="attention-item ${item.tone}" data-summary-action="${esc(item.target)}"><span class="signal-mark signal-${esc(item.tone)}" aria-hidden="true"></span><span><strong>${esc(item.title)}</strong><span>${esc(item.detail)}</span></span>${item.target ? '<span class="attention-arrow" aria-hidden="true"></span>' : '<span></span>'}</button>`;
}
function renderAttention() {
  if (!state.project) return;
  const items = attentionSignals(), first = items[0];
  const urgent = items.filter(item => ["bad", "warn"].includes(item.tone)).length;
  setHtml("statusSummary", els.statusSummary, `<div><span class="eyebrow">${esc(localized("PROJECT PULSE", "项目状态"))}</span><h2>${esc(first.title)}</h2><p>${esc(first.detail)}</p></div><span class="summary-count ${urgent ? "warn" : "info"}">${esc(urgent ? localized(`${urgent} to review`, `${urgent} 项待处理`) : localized("Read the evidence", "请结合证据判断"))}</span>`);
  const rest = items.slice(1);
  const html = rest.length ? `<details class="more-signals"><summary>${esc(localized(`${rest.length} more signals`, `另有 ${rest.length} 项信号`))}</summary>${rest.map(attentionItem).join("")}</details>` : "";
  setHtml("attention", els.attention, html, () => bindSummaryActions(els.attention));
}
function renderEffectiveEvidence(rows) {
  const heading = `<h3>${esc(localized("Effective checks · this revision", "当前版本 · 有效检查"))}</h3>`;
  const explanation = `<p class="panel-meta">${esc(localized(
    "Latest result per target, producer and policy. Earlier attempts remain in historical counts above.",
    "按目标、来源与策略分别取最新结果；先前执行仍保留在上方历史计数中。",
  ))}</p>`;
  if (!rows.length) return heading + explanation + `<div class="empty">${esc(localized(
    "No effective verification evidence for this revision.", "此版本没有有效验证证据。",
  ))}</div>`;
  const body = rows.map(item => {
    const diagnostic = item.summary
      ? `<details><summary>${esc(localized("Diagnostic summary", "诊断摘要"))}</summary><pre class="evidence-diagnostic">${esc(item.summary)}</pre></details>`
      : "";
    return `<tr><td>${pill(statusLabel(item.result), statusClass(item.result))}</td>` +
      `<td><code>${esc(item.subject)}</code><div>${esc(item.producer)}</div>${diagnostic}</td>` +
      `<td>${esc(item.policy || "—")}<div>${esc(time(item.timestamp_ms))}</div></td></tr>`;
  }).join("");
  const headers = [t("Status"), localized("Check / producer", "检查 / 来源"), localized("Policy / time", "策略 / 时间")]
    .map(label => `<th scope="col">${esc(label)}</th>`).join("");
  return heading + explanation + `<div class="table-wrap"><table class="table proof-table"><thead><tr>${headers}</tr></thead><tbody>${body}</tbody></table></div>`;
}
function renderProofSummary() {
  if (!state.project) return;
  const proof = state.project.proof || {}, acceptance = proof.acceptance || {}, effective = proof.effective || {},
    rows = Array.isArray(effective.items) ? effective.items.slice(0, 32) : [],
    evidenceKey = (item, index) => String(item.artifact_digest || item.id || `${item.subject || "evidence"}:${item.producer || "unknown"}:${item.timestamp_ms || index}`),
    keyed = rows.map((item, index) => ({ item, key: evidenceKey(item, index), index }));
  if (!keyed.some(entry => entry.key === state.selectedEvidenceKey)) state.selectedEvidenceKey = keyed[0]?.key || "";
  const selectedEntry = keyed.find(entry => entry.key === state.selectedEvidenceKey) || keyed[0], selected = selectedEntry?.item,
    selectedIndex = selectedEntry?.index ?? -1,
    coverage = Number(acceptance.total || 0) > 0 ? Math.round((Number(acceptance.fresh || 0) / Number(acceptance.total || 1)) * 100) : 0,
    drift = Number(state.project.architecture?.blocking_drift_edges || 0),
    metricCards = [
      [localized("Plans", "计划"), num(proof.current_verification_plans || 0), localized(`${num(proof.current_verification_ready || 0)} ready now`, `${num(proof.current_verification_ready || 0)} 个已就绪`), "accent", "document"],
      [localized("Passing evidence", "通过证据"), num(proof.current_passed || effective.passed || 0), localized(`${num(effective.total || rows.length)} effective records`, `${num(effective.total || rows.length)} 条有效记录`), "good", "check"],
      [localized("Evidence coverage", "证据覆盖"), acceptance.total ? `${coverage}%` : "—", localized(`${num(acceptance.fresh || 0)} / ${num(acceptance.total || 0)} fresh`, `${num(acceptance.fresh || 0)} / ${num(acceptance.total || 0)} 新鲜`), "accent", "target"],
      [localized("Drift risk", "偏离风险"), drift ? localized("Attention", "需处理") : localized("Low", "低"), drift ? localized(`${drift} confirmed drift edges`, `${drift} 条确认偏离`) : localized("No confirmed architecture drift", "没有确认的架构偏离"), drift ? "warn" : "good", "warning"],
    ].map(([label, value, detail, tone, icon]) => `<div class="proof-metric-card ${tone}"><span class="proof-metric-icon">${uiIcon(icon)}</span><div><strong>${esc(value)}</strong><span>${esc(label)}</span><small>${esc(detail)}</small></div></div>`).join("");
  const ledgerRows = keyed.length ? keyed.map(({ item, key, index }) => {
    const result = item.result || "unknown", active = key === state.selectedEvidenceKey,
      id = item.id || item.artifact_digest || key,
      plan = item.plan_id || item.policy || "—",
      revision = String(item.revision || proof.revision_code || "—"),
      proofType = item.stage || item.kind || localized("Verification", "验证");
    return `<button type="button" class="evidence-ledger-row${active ? " selected" : ""}" data-evidence-key="${esc(key)}" role="row" aria-pressed="${active}"><span role="cell">${pill(statusLabel(result), statusClass(result))}</span><span role="cell"><code>${esc(String(id).slice(0, 16))}</code></span><span role="cell"><strong>${esc(item.subject || localized("Evidence", "证据"))}</strong><small>${esc(item.producer || "—")}</small></span><span role="cell"><code>${esc(String(plan).slice(0, 14))}</code></span><span role="cell"><code>${esc(revision.slice(0, 10))}</code></span><span role="cell">${esc(time(item.timestamp_ms))}</span><span role="cell"><strong>${esc(proofType)}</strong><small>#${index + 1}</small></span></button>`;
  }).join("") : `<div class="empty">${esc(localized("No effective verification evidence for this revision.", "此版本没有有效验证证据。"))}</div>`;
  const inspectorControls = keyed.length ? `<div class="evidence-inspector-controls"><button type="button" data-evidence-prev aria-label="${esc(localized("Previous evidence", "上一条证据"))}" ${selectedIndex <= 0 ? "disabled" : ""}>${uiIcon("chevron-left")}</button><button type="button" data-evidence-next aria-label="${esc(localized("Next evidence", "下一条证据"))}" ${selectedIndex >= keyed.length - 1 ? "disabled" : ""}>${uiIcon("chevron-right")}</button><button type="button" data-evidence-close aria-label="${esc(localized("Close inspector", "关闭检查器"))}">${uiIcon("close")}</button></div>` : "";
  const inspector = selected ? `<div class="evidence-inspector-head"><div><span class="proof-metric-icon">${uiIcon("cube")}</span><strong>${esc(localized("Evidence inspector", "证据检查器"))}</strong></div>${inspectorControls}</div><div class="evidence-inspector-identity"><div class="evidence-inspector-state">${pill(statusLabel(selected.result || "unknown"), statusClass(selected.result || "unknown"))}<code>${esc(String(selected.id || selected.artifact_digest || state.selectedEvidenceKey).slice(0, 18))}</code></div><h3>${esc(selected.subject || localized("Verification evidence", "验证证据"))}</h3><span>${esc(selected.producer || "—")} · ${esc(selected.policy || "—")} · ${esc(time(selected.timestamp_ms))}</span></div><section class="evidence-inspector-section"><h4>${esc(localized("Exact revision", "精确版本"))}</h4><code>${esc(proof.revision_code || selected.revision || "—")}</code><small>${esc(proof.revision_design || "")}</small></section><section class="evidence-inspector-section"><h4>${esc(localized("Effective checks", "有效检查"))}</h4><div class="inspector-chip-list"><span class="inspector-chip">${esc(selected.stage || selected.kind || "deterministic")}</span><span class="inspector-chip">${esc(selected.policy || "—")}</span><span class="inspector-chip good">${esc(statusLabel(selected.result || "unknown"))}</span></div></section><section class="evidence-inspector-section"><h4>${esc(localized("Adaptive verification", "自适应验证"))}</h4><div class="inspector-chip-list"><span class="inspector-chip">${esc(state.project.adaptive_verification?.mode || "static")}</span><span class="inspector-chip">${esc(selected.producer || "—")}</span></div></section><section class="evidence-inspector-section"><h4>${esc(localized("Diagnostic", "诊断"))}</h4>${selected.summary ? `<pre>${esc(selected.summary)}</pre>` : `<p>${esc(localized("No diagnostic summary was retained for this effective record.", "该有效记录没有保留诊断摘要。"))}</p>`}</section>` : `<div class="empty">${esc(localized("Select evidence to inspect its exact revision and producer.", "选择一条证据以查看精确版本和来源。"))}</div>`;
  const truncated = effective.truncated ? `<p class="proof-truncated warn">${esc(localized("Details truncated; counts include all retained effective records.", "详情已截断；计数包含全部已保留的有效记录。"))}</p>` : "";
  const html = `<div class="proof-metric-strip">${metricCards}</div><div class="proof-main-grid${state.evidenceInspectorOpen ? "" : " inspector-closed"}"><section class="evidence-ledger-card"><header class="proof-card-head"><div><strong>${esc(localized("Evidence ledger", "证据账本"))}</strong><span>${esc(localized("Revision-bound verification evidence generated from effective checks.", "由有效检查生成、绑定版本的验证证据。"))}</span></div><span>${num(effective.total || rows.length)} ${esc(localized("items", "条"))}</span></header><div class="evidence-ledger-head" role="row"><span>${esc(localized("Status", "状态"))}</span><span>ID</span><span>${esc(localized("Subject", "目标"))}</span><span>${esc(localized("Plan", "计划"))}</span><span>${esc(localized("Revision", "版本"))}</span><span>${esc(localized("Timestamp", "时间"))}</span><span>${esc(localized("Proof type", "证据类型"))}</span></div><div class="evidence-ledger-body">${ledgerRows}</div></section>${state.evidenceInspectorOpen ? `<aside class="evidence-inspector-card">${inspector}</aside>` : ""}</div>${truncated}`;
  setHtml("proofSummary", els.proofSummary, html, () => {
    const focusSelected = () => requestAnimationFrame(() => [...els.proofSummary.querySelectorAll("[data-evidence-key]")].find(button => button.dataset.evidenceKey === state.selectedEvidenceKey)?.focus());
    els.proofSummary.querySelectorAll("[data-evidence-key]").forEach((button, index) => {
      button.addEventListener("click", () => {
        state.selectedEvidenceKey = button.dataset.evidenceKey; state.evidenceInspectorOpen = true; invalidate("proofSummary"); renderProofSummary();
      });
      button.addEventListener("keydown", event => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? keyed.length - 1 : event.key === "ArrowDown" ? Math.min(keyed.length - 1, index + 1) : Math.max(0, index - 1);
        state.selectedEvidenceKey = keyed[nextIndex]?.key || state.selectedEvidenceKey;
        state.evidenceInspectorOpen = true; invalidate("proofSummary"); renderProofSummary(); focusSelected();
      });
    });
    els.proofSummary.querySelector("[data-evidence-prev]")?.addEventListener("click", () => { if (selectedIndex > 0) { state.selectedEvidenceKey = keyed[selectedIndex - 1].key; invalidate("proofSummary"); renderProofSummary(); focusSelected(); } });
    els.proofSummary.querySelector("[data-evidence-next]")?.addEventListener("click", () => { if (selectedIndex >= 0 && selectedIndex < keyed.length - 1) { state.selectedEvidenceKey = keyed[selectedIndex + 1].key; invalidate("proofSummary"); renderProofSummary(); focusSelected(); } });
    els.proofSummary.querySelector("[data-evidence-close]")?.addEventListener("click", () => { state.evidenceInspectorOpen = false; invalidate("proofSummary"); renderProofSummary(); });
  });
}
function renderActivity() {
  const snapshot = state.activitySnapshot, activity = snapshot?.activity || state.project?.activity;
  if (activity?.available !== true || state.activityError) {
    setHtml("activity", els.activity, `<div class="empty">${esc(localized("Activity unavailable. This does not mean no tasks are running.", "活动数据暂不可用，不代表没有任务在运行。"))}</div>`);
    setHtml("resourceStatus", els.resourceStatus, `<div class="empty">${esc(localized("Resource telemetry unavailable", "资源遥测不可用"))}</div>`);
    return;
  }
  const rows = activity.recent || [];
  const duration = ms => ms == null ? "—" : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`;
  const label = status => ({queued: localized("Queued", "排队中"), running: localized("Running", "执行中"), completed: localized("Completed", "已完成"), failed: localized("Failed / interrupted", "失败 / 中断")})[status] || status;
  const html = `<header class="activity-ledger-head"><div><strong>${esc(localized("Recent task activity", "最近任务活动"))}</strong><span>${esc(localized("Current project · retained rows are bounded", "当前项目 · 最近记录有数量上限"))}</span></div><span class="activity-live-count">${num(activity.active || 0)} ${esc(localized("active", "执行中"))} · ${num(activity.queued || 0)} ${esc(localized("queued", "排队"))}</span></header><div class="activity-list">${rows.length ? rows.map(task => `<div class="activity-row"><span class="pill ${task.status === "failed" ? "bad" : task.status === "running" ? "info" : ""}">${esc(label(task.status))}</span><div><strong>${esc(task.tool)}</strong><small>#${num(task.id)} · ${esc(task.slot_counted ? localized("tool slot", "工具槽位") : localized("coordination only", "仅编排，不占槽位"))}</small></div><div class="activity-duration"><span>${esc(localized("wait ", "等待 "))}${duration(task.wait_ms)}</span><span>${esc(localized("run ", "执行 "))}${duration(task.run_ms)}</span></div></div>`).join("") : `<div class="empty">${esc(localized("No retained task records for this project.", "此项目暂无保留的任务记录。"))}</div>`}</div><p class="activity-history-note">${esc(localized(`${num(activity.completed)} completed · ${num(activity.failed)} failed/interrupted (historical totals, not current blockers)`, `累计 ${num(activity.completed)} 项完成 · ${num(activity.failed)} 项失败/中断（历史总数，不等于当前阻塞）`))}${activity.recent_truncated ? ` · ${esc(t("truncated"))}` : ""}</p>`;
  setHtml("activity", els.activity, html);
  const limits = snapshot?.resources?.limits;
  const queue = (value, name) => value ? `<div><strong>${num(value.active)} / ${num(value.limit)}</strong><span>${esc(name)} · ${num(value.waiting)} ${esc(localized("waiting", "排队"))}</span></div>` : "";
  const resources = limits ? `<header class="resource-telemetry-head"><strong>${esc(localized("Runtime capacity", "运行时容量"))}</strong><span>${esc(localized("Shared by all workspaces · reserved permits are not CPU-running tasks", "全部项目共享 · 占用额度不等于正在使用 CPU"))}</span></header><div class="proof-counts resource-counts">${queue(limits.child_queue, localized("Heavy processes", "重型进程"))}${queue(limits.probe_queue, localized("Git probes", "Git 检查"))}<div><strong>${typeof limits.resident_memory_bytes === "number" && Number.isFinite(limits.resident_memory_bytes) && limits.resident_memory_bytes >= 0 ? `${Math.round(limits.resident_memory_bytes / 1048576)} MiB` : "—"}</strong><span>${esc(localized("Resident memory", "进程驻留内存"))}</span></div></div>` : `<div class="empty">${esc(localized("Resource telemetry requires the updated runtime.", "资源遥测需要更新后的运行时。"))}</div>`;
  let bottleneck = "";
  if (limits?.memory_pressure === "critical" || limits?.memory_pressure === "over_limit") bottleneck = localized("Memory pressure is delaying new work.", "内存压力正在延迟新任务进入。");
  else if (limits?.child_queue?.waiting) bottleneck = localized("Commands are waiting for heavy-process capacity.", "命令正在等待重型进程名额。");
  else if (limits?.probe_queue?.waiting) bottleneck = localized("Git inspections are waiting for probe capacity.", "Git 查询正在等待检查名额。");
  else if (activity.queued) bottleneck = localized("Tasks are queued; the exact waiting reason is not tracked here.", "有任务排队；此处尚未追踪每项任务的具体等待原因。");
  setHtml("resourceStatus", els.resourceStatus, resources + (bottleneck ? `<p class="risk medium">${esc(bottleneck)}</p>` : ""));
}
