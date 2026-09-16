function requirementMatches(r) {
  const query = els.search.value.trim().toLowerCase();
  const hay = [
    r.id,
    r.title,
    r.intent,
    ...(r.components || []).flatMap(
      (c) => [c.id, c.name, ...(c.responsibilities || [])],
    ),
  ].join(" ").toLowerCase();
  if (query && !hay.includes(query)) return false;
  if (state.filter === "changed" && !r.changed) return false;
  if (state.filter === "drift" && r.convergence !== "needs_convergence") {
    return false;
  }
  if (state.filter === "incomplete" && r.convergence !== "incomplete") {
    return false;
  }
  return true;
}
function renderRequirements() {
  const all = state.project?.requirements || [],
    items = all.filter(requirementMatches);
  els.reqCount.textContent = `${items.length} / ${all.length}`;
  if (!state.selected || !items.some((r) => r.id === state.selected)) {
    state.selected = (items.find((r) => r.changed) || items[0] || {}).id || "";
  }
  const html = items.map((r) => {
    const dependencies = r.dependency_alignment || [],
      blocking = dependencies.filter((d) => d.blocking).length,
      advisory = dependencies.filter((d) =>
        !d.blocking && d.status !== "aligned"
      ).length + (r.drift || []).length;
    const selected = r.id === state.selected;
    return `<button type="button" class="req ${selected ? "selected" : ""}" data-id="${esc(r.id)}" aria-pressed="${selected}"><div class="req-top"><span class="req-id">${
      esc(r.id)
    }</span><span class="dot ${
      statusClass(r.convergence)
    }"></span></div><div class="req-name">${
      esc(r.title)
    }</div><div class="req-meta"><span>${
      esc(statusLabel(r.priority))
    }</span><span>·</span><span class="${statusClass(r.convergence)}">${
      esc(statusLabel(r.convergence))
    }</span><span>·</span><span>${
      esc(unit(r.components.length, "component", "components", "个组件"))
    }</span><span>·</span><span>${
      esc(unit(r.implementation_lines, "line", "lines", "行"))
    }</span>${
      blocking
        ? `<span class="bad">· ${blocking} ${
          blocking === 1 ? t("blocker") : t("blockers")
        }</span>`
        : ""
    }${
      advisory ? `<span class="info">· ${advisory} ${t("advisory")}</span>` : ""
    }</div></button>`;
  }).join("") ||
    `<div class="empty">${esc(t("No requirements match this filter."))}</div>`;
  setHtml(
    "requirements",
    els.requirements,
    html,
    () => {
      const buttons = [...els.requirements.querySelectorAll(".req")];
      const activate = (button, { focus = false } = {}) => {
        if (!button) return;
        state.selected = button.dataset.id;
        invalidate("requirements", "detail");
        renderRequirements();
        renderDetail();
        if (focus) requestAnimationFrame(() => [...els.requirements.querySelectorAll(".req")].find(item => item.dataset.id === state.selected)?.focus());
        if (window.matchMedia("(max-width: 720px)").matches) {
          requestAnimationFrame(() =>
            els.detail.scrollIntoView({ behavior: "smooth", block: "start" })
          );
        }
      };
      buttons.forEach((button, index) => {
        button.addEventListener("click", () => activate(button));
        button.addEventListener("keydown", event => {
          if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : event.key === "ArrowDown" ? Math.min(buttons.length - 1, index + 1) : Math.max(0, index - 1);
          activate(buttons[nextIndex], { focus: true });
        });
      });
    },
  );
}
function implementationRows(items) {
  if (!items.length) {
    return `<div class="empty">${
      esc(t("No implementation reference declared."))
    }</div>`;
  }
  return `<div class="impl-list">${
    items.map((item) =>
      `<div class="impl ${item.changed ? "changed" : ""} ${
        item.resolved ? "" : "unresolved"
      }"><div class="impl-path">${
        esc(item.target)
      }</div><div class="impl-meta">${
        esc(statusLabel(item.resolved ? "resolved" : "unresolved"))
      } · ${esc(item.provider)} / ${esc(statusLabel(item.precision))}${
        item.changed ? ` · ${esc(localized("changed now", "当前已变更"))}` : ""
      }</div></div>`
    ).join("")
  }</div>`;
}
function designComponents(r) {
  return r.components.map((component) =>
    `<div class="component-card ${
      component.changed ? "changed" : ""
    }"><div class="req-top"><div><b>${
      esc(component.name)
    }</b><div class="component-id">${esc(component.id)}</div></div>${
      pill(unit(component.implementation_lines, "line", "lines", "行"))
    }</div>${
      component.responsibilities.length
        ? `<ul class="responsibilities">${
          component.responsibilities.map((item) => `<li>${esc(item)}</li>`)
            .join("")
        }</ul>`
        : `<div class="empty">${esc(t("No responsibilities declared."))}</div>`
    }${
      component.depends_on.length
        ? `<div class="pills">${
          component.depends_on.map((dependency) =>
            pill(localized(`depends on ${dependency}`, `依赖 ${dependency}`))
          ).join("")
        }</div>`
        : ""
    }</div>`
  ).join("") ||
    `<div class="empty">${
      esc(t("No implementation component declared."))
    }</div>`;
}
function acceptanceNodes(r) {
  return r.acceptance.map((a) =>
    `<div class="accept-card"><b>${esc(a.title)}</b><div class="component-id">${
      esc(a.id)
    }</div><small>${esc(a.statement)}</small></div>`
  ).join("") ||
    `<div class="empty">${esc(t("No acceptance criterion declared."))}</div>`;
}
function actualComponents(r) {
  return r.components.map((component) =>
    `<div class="component-card ${
      component.changed ? "changed" : ""
    }"><div class="req-top"><div><b>${
      esc(component.name)
    }</b><div class="component-id">${
      esc(component.id)
    }</div></div><div class="pills">${
      pill(unit(component.implementation_lines, "line", "lines", "行"))
    }${
      component.changed
        ? pill(statusLabel("changed"), "warn")
        : pill(statusLabel("current"), "good")
    }</div></div>${implementationRows(component.implementation)}</div>`
  ).join("") ||
    `<div class="empty">${esc(t("No current implementation mapping."))}</div>`;
}
function dependencyRows(r) {
  const deps = r.dependency_alignment || [];
  if (!deps.length) {
    return `<div class="empty">${
      esc(
        t("No cross-component dependency is declared or detected for this feature."),
      )
    }</div>`;
  }
  return `<div class="deps">${
    deps.map((dep) => {
      const tone = dep.blocking
          ? "warn"
          : dep.status === "aligned"
          ? "good"
          : "info",
        precision = dep.precision === "not_observed"
          ? t("not observed")
          : statusLabel(dep.precision);
      return `<div class="dep"><div><b>${
        esc(dep.from_name)
      }</b><div class="component-id">${
        esc(dep.from)
      }</div></div><div class="arrow" aria-hidden="true"></div><div><b>${
        esc(dep.to_name)
      }</b><div class="component-id">${
        esc(dep.to)
      }</div></div><div class="dep-state">${
        pill(statusLabel(dep.status), tone)
      }<div class="panel-meta">${esc(precision)} · ${
        dep.blocking
          ? t("blocker")
          : dep.status === "aligned"
          ? t("evidence")
          : t("advisory")
      }</div></div></div>`;
    }).join("")
  }</div>`;
}
function verificationBlock(r) {
  if (!r.acceptance.length) {
    return `<div class="empty">${esc(t("No acceptance criteria."))}</div>`;
  }
  return `<div class="verification">${
    r.acceptance.map((a) =>
      `<div class="verification-item"><div class="req-top"><b>${
        esc(a.title)
      }</b><code>${esc(a.id)}</code></div><div class="statement">${
        esc(a.statement)
      }</div>${implementationRows(a.verification)}</div>`
    ).join("")
  }</div>`;
}
function featureFlow(r) {
  const impl = (r.components || []).flatMap((c) => c.implementation || []),
    resolved = impl.filter((i) => i.resolved).length,
    ver = (r.acceptance || []).flatMap((a) => a.verification || []),
    verified = ver.filter((i) => i.resolved).length,
    blockers = r.convergence_blockers || [];
  const steps = [[
    t("Desired State"),
    statusLabel(r.status === "complete" ? "declared" : "incomplete"),
    localized(
      `${r.components.length} components · ${r.acceptance.length} acceptance criteria`,
      `${r.components.length} 个组件 · ${r.acceptance.length} 个验收条件`,
    ),
    r.status === "complete" ? "good" : "warn",
  ], [
    t("Actual State"),
    localized(
      `${resolved}/${impl.length} references resolved`,
      `${resolved}/${impl.length} 个引用已解析`,
    ),
    localized(
      `${
        num(r.implementation_lines)
      } lines · ${r.implementation_symbols} symbols`,
      `${num(r.implementation_lines)} 行 · ${r.implementation_symbols} 个符号`,
    ),
    resolved === impl.length ? "good" : "warn",
  ], [
    t("Change"),
    r.changed
      ? localized("changing", "变更中")
      : localized("no mapped change", "无映射变更"),
    r.changed
      ? `+${r.change_additions}/-${r.change_deletions}`
      : localized("working tree stable", "工作树稳定"),
    r.changed ? "warn" : "good",
  ], [
    t("Proof"),
    `${t("Mapped")} ${verified}/${ver.length}`,
    ver.length && verified === ver.length
      ? localized(
        "References resolved; execution, result and freshness are separate.",
        "引用已解析；执行、结果和新鲜度分别统计。",
      )
      : localized("verification mapping incomplete", "验证映射不完整"),
    ver.length && verified === ver.length ? "good" : "warn",
  ], [
    t("Convergence"),
    statusLabel(r.convergence),
    blockers.length
      ? `${blockers.length} ${
        blockers.length === 1 ? t("blocker") : t("blockers")
      }`
      : localized("design and actual state aligned", "设计与实际状态已对齐"),
    statusClass(r.convergence),
  ]];
  return `<div class="convergence-flow">${
    steps.map(([label, value, detail, tone], index) =>
      `<div class="flow-step ${tone}"><div class="flow-step-head"><span class="flow-index">0${index + 1}</span><div class="flow-label">${
        esc(label)
      }</div></div><div class="flow-value">${
        esc(value)
      }</div><div class="flow-detail">${esc(detail)}</div></div>`
    ).join("")
  }</div>`;
}
function requirementChanges(r) {
  return (state.project.changes || []).filter((change) =>
    (change.affected_requirements || []).includes(r.id)
  );
}
function renderDetail() {
  const r = (state.project?.requirements || []).find((item) =>
    item.id === state.selected
  );
  if (!r) {
    setHtml(
      "detail",
      els.detail,
      `<div class="section empty">${esc(t("Select a requirement."))}</div>`,
    );
    return;
  }
  const deps = r.dependency_alignment || [],
    blockingDeps = deps.filter((dep) => dep.blocking),
    advisories = deps.filter((dep) =>
      !dep.blocking && dep.status !== "aligned"
    ),
    changes = requirementChanges(r);
  const alignmentSignals = [];
  if (blockingDeps.length) {
    alignmentSignals.push(
      pill(
        localized(
          `${blockingDeps.length} dependency ${
            blockingDeps.length === 1 ? t("blocker") : t("blockers")
          }`,
          `${blockingDeps.length} 个依赖${
            blockingDeps.length === 1 ? t("blocker") : t("blockers")
          }`,
        ),
        "warn",
      ),
    );
  }
  if (advisories.length) {
    alignmentSignals.push(
      pill(
        localized(
          `${advisories.length} dependency ${t("advisory")}`,
          `${advisories.length} 个依赖${t("advisory")}`,
        ),
        "info",
      ),
    );
  }
  if (!alignmentSignals.length) {
    alignmentSignals.push(
      pill(localized("dependencies aligned", "依赖已对齐"), "good"),
    );
  }
  const html = `<div class="feature-head"><div class="eyebrow"><span>${
    esc(r.id)
  }</span><span>·</span><span>${
    esc(statusLabel(r.priority))
  }</span><span>·</span><span>${esc(statusLabel(r.status))}</span></div><h2>${
    esc(r.title)
  }</h2><div class="intent">${esc(r.intent)}</div><div class="pills">${
    pill(statusLabel(r.convergence), statusClass(r.convergence))
  }${
    r.changed
      ? pill(
        localized(
          `changed +${r.change_additions}/-${r.change_deletions}`,
          `已变更 +${r.change_additions}/-${r.change_deletions}`,
        ),
        "warn",
      )
      : pill(
        localized("no mapped implementation change", "无映射实现变更"),
        "good",
      )
  }${pill(unit(r.implementation_files, "file", "files", "个文件"))}${
    pill(unit(r.implementation_symbols, "symbol", "symbols", "个符号"))
  }${pill(unit(r.implementation_lines, "line", "lines", "行"))}${
    pill(
      unit(
        r.acceptance.length,
        "acceptance criterion",
        "acceptance criteria",
        "个验收条件",
      ),
    )
  }</div></div>${featureFlow(r)}<section class="section"><h3>${
    esc(t("Feature architecture · desired vs actual"))
  }</h3><div class="alignment-banner"><div><strong class="${
    statusClass(r.convergence)
  }">${esc(statusLabel(r.convergence))}</strong><div class="panel-meta">${
    esc(t("positive evidence note"))
  }${
    r.convergence_blockers?.length
      ? ` ${esc(r.convergence_blockers.slice(0, 3).join(" · "))}`
      : ""
  }</div></div><div class="pills">${
    alignmentSignals.join("")
  }</div></div><div class="architecture-lane"><div class="lane-label">${
    esc(t("Design architecture"))
  }</div><div class="arch-flow"><div class="node arch-root-node"><span class="arch-node-kind">${esc(localized("Requirement", "需求"))}</span><b>${
    esc(r.title)
  }</b><small>${esc(r.id)}</small></div><div class="connector" aria-hidden="true"></div><div class="node-stack arch-node-group"><span class="arch-node-group-label">${esc(localized("Owned components", "所属组件"))}</span>${
    designComponents(r)
  }</div><div class="connector" aria-hidden="true"></div><div class="node-stack arch-node-group"><span class="arch-node-group-label">${esc(localized("Acceptance proof", "验收证明"))}</span>${
    acceptanceNodes(r)
  }</div></div></div><div class="architecture-lane"><div class="lane-label">${
    esc(t("Actual code architecture · generated from current implementation"))
  }</div><div class="actual-grid">${
    actualComponents(r)
  }</div><div class="lane-spacer"></div>${
    dependencyRows(r)
  }</div></section><section class="section"><div class="two"><div><h3>${
    esc(t("Acceptance & verification"))
  }</h3>${verificationBlock(r)}</div><div><h3>${
    esc(t("Constraints, decisions & drift"))
  }</h3>${
    r.constraints.length
      ? r.constraints.map((c) =>
        `<div class="info-card"><h4>${
          esc(c.title)
        }</h4><div class="component-id">${
          esc(c.id)
        }</div><div class="panel-meta card-copy">${
          esc(c.statement)
        }</div></div>`
      ).join("")
      : `<div class="empty">${
        esc(t("No requirement-specific constraints."))
      }</div>`
  }${
    r.decisions?.length
      ? `<div class="card-gap"></div>${
        r.decisions.map((d) =>
          `<div class="info-card"><div class="req-top"><h4>${
            esc(d.title)
          }</h4>${
            pill(statusLabel(d.status), "accent")
          }</div><div class="component-id">${
            esc(d.id)
          }</div><div class="card-copy">${esc(d.decision)}</div>${
            d.rationale
              ? `<div class="panel-meta card-copy">${esc(d.rationale)}</div>`
              : ""
          }</div>`
        ).join("")
      }`
      : ""
  }${
    r.drift.length
      ? `<div class="card-gap"></div><div class="risk-list">${
        r.drift.map((item) => `<div class="risk drift">${esc(item)}</div>`)
          .join("")
      }</div>`
      : ""
  }</div></div></section><section class="section"><h3>${
    esc(t("Current changes touching this feature"))
  }</h3>${
    changes.length
      ? changeTable(changes, true)
      : `<div class="empty">${
        esc(t("No current working-tree file is mapped to this requirement."))
      }</div>`
  }</section>`;
  setHtml("detail", els.detail, html);
}

function changeTable(items, compact = false) {
  return `<table class="table change-table"><thead><tr><th>${esc(t("Path"))}</th><th>${
    esc(t("Status"))
  }</th><th>${esc(t("Scope"))}</th><th>${esc(t("Diff"))}</th>${
    compact ? "" : `<th>${esc(t("Requirements"))}</th>`
  }</tr></thead><tbody>${
    items.slice(0, 120).map((item) => {
      const tone = statusClass(item.untracked ? "untracked" : item.status);
      return `<tr class="change-row" data-tone="${esc(tone)}"><td data-label="${esc(t("Path"))}"><div class="change-path"><code>${
        esc(item.path)
      }</code></div></td><td data-label="${esc(t("Status"))}"><span class="change-status">${pill(statusLabel(item.status), tone)}${
        item.untracked ? pill(t("untracked"), "info") : ""
      }</span></td><td data-label="${esc(t("Scope"))}"><span class="change-scope">${esc(item.scope || "—")}</span></td><td data-label="${esc(t("Diff"))}">${changeNums(item)}</td>${
        compact
          ? ""
          : `<td data-label="${esc(t("Requirements"))}">${
            (item.affected_requirements || []).slice(0, 6).map((id) =>
              `<button type="button" class="click-req" data-req="${esc(id)}">${esc(id)}</button>`
            ).join(" ") || "—"
          }</td>`
      }</tr>`;
    }).join("")
  }</tbody></table>`;
}
function verificationImpactKind(kind) {
  switch (kind) {
    case "direct_change": return localized("direct change", "直接变更");
    case "contract_bridge": return localized("contract bridge", "契约桥接");
    case "manifest_dependency": return localized("manifest dependent", "清单下游");
    case "broad_fallback": return localized("broad fallback", "广域回退");
    default: return kind || "—";
  }
}
function renderVerificationImpact() {
  const impact = state.project?.verification_impact;
  if (!impact) {
    setHtml(
      "verificationImpact",
      els.verificationImpact,
      `<div class="empty">${esc(localized("No current change selection impact.", "当前没有需要解释的变更验证影响。"))}</div>`,
    );
    return;
  }
  const affected = impact.affected_islands || [], reasons = impact.reasons || [];
  const summary = `<div class="pills">${
    pill(localized(`${affected.length} affected islands`, `${affected.length} 个受影响项目岛`))
  }${pill(`${impact.provider || "—"} / ${impact.precision || "—"}`)}${
    impact.selective
      ? pill(localized("selective", "选择性验证"), "good")
      : pill(localized("broad fallback", "广域回退"), "warn")
  }${impact.truncated ? pill(t("truncated"), "warn") : ""}</div>`;
  const rows = reasons.slice(0, 12).map((reason) =>
    `<div class="dep"><div><code>${esc(reason.source || "workspace")}</code><div class="component-id">${
      esc(reason.evidence || "—")
    }</div></div><div class="arrow" aria-hidden="true"></div><div><b>${esc(reason.island || "*")}</b><div class="component-id">${
      esc(reason.relationship || "—")
    }</div></div><div class="dep-state">${
      pill(verificationImpactKind(reason.kind), reason.kind === "broad_fallback" ? "warn" : "info")
    }<div class="panel-meta">${esc(reason.provider || "—")} / ${esc(reason.precision || "—")}</div></div></div>`
  ).join("");
  setHtml(
    "verificationImpact",
    els.verificationImpact,
    `${summary}<div class="deps">${rows || `<div class="empty">${esc(localized("No impact reasons recorded.", "没有记录影响原因。"))}</div>`}</div>`,
  );
}
function adaptiveModeLabel(mode) {
  return {
    static: localized("static plan", "静态计划"),
    focused_test: localized("focused test", "聚焦测试"),
    cost_sentinel: localized("cost sentinel", "成本哨兵"),
    combined: localized("combined", "组合优化"),
  }[mode] || statusLabel(mode || "static");
}
function adaptiveFallbackLabel(reason) {
  return {
    no_current_changes: localized("No current changes; no adaptive quick plan is needed.", "当前没有变更，无需生成自适应 quick 计划。"),
    review_unavailable: localized("Git review is unavailable; keep the canonical static plan.", "Git Review 不可用，保持标准静态计划。"),
    quick_verification_gap: localized("An affected island lacks quick verification coverage; resolve the gap before optimizing order.", "受影响项目岛缺少 quick 验证覆盖，先补验证缺口再优化顺序。"),
    quick_plan_exceeds_bound: localized("The quick plan exceeds the bounded planner capacity; narrow the workspace before adapting it.", "quick 计划超过有界规划容量，请先缩小工作区再做自适应。"),
    no_strong_adaptive_evidence: localized("No strong adaptive evidence; the canonical quick plan remains unchanged.", "没有足够强的自适应证据，继续使用标准 quick 计划。"),
    cost_backtest_outcome_mismatch: localized("The cost model produced a temporal replay outcome mismatch and was blocked fail-closed.", "成本模型在时间回放中出现结果不一致，已按 fail-closed 阻止启用。"),
    cost_backtest_non_positive_net_savings: localized("Temporal replay found no positive net time savings; adaptive cost reordering was disabled.", "时间回放没有证明正向净耗时收益，已禁用自适应成本重排。"),
  }[reason] || reason || localized("Canonical quick plan remains unchanged.", "继续使用标准 quick 计划。");
}
function renderAdaptiveVerification() {
  const adaptive = state.project?.adaptive_verification;
  if (!adaptive) {
    setHtml(
      "adaptiveVerification",
      els.adaptiveVerification,
      `<div class="empty">${esc(localized("Adaptive verification preview is unavailable.", "自适应验证预览当前不可用。"))}</div>`,
    );
    return;
  }
  const identity = `<div class="pills">${
    pill(adaptiveModeLabel(adaptive.mode), adaptive.mode === "static" ? "info" : "good")
  }${pill(`${adaptive.provider || "verification-planner"} / ${adaptive.precision || "—"}`)}${
    pill(localized(`${adaptive.base_quick_checks || 0} → ${adaptive.planned_quick_checks || 0} quick checks`, `quick 检查 ${adaptive.base_quick_checks || 0} → ${adaptive.planned_quick_checks || 0}`))
  }${adaptive.full_coverage_unchanged ? pill(localized("full coverage unchanged", "full 覆盖不变"), "good") : ""}</div>`;
  const previewNote = `<div class="panel-meta card-gap">${esc(localized(
    "Planning preview only — Engineering Observatory does not execute these checks.",
    "这里只做计划预览——工程观测台不会执行这些检查。",
  ))}</div>`;
  const focused = adaptive.focused_test
    ? `<div class="info-card"><b>${esc(localized("Focused test", "聚焦测试"))}</b><div><code>${esc(adaptive.focused_test.command)}</code></div><div class="panel-meta">${esc(adaptive.focused_test.island || "workspace")} · phase ${esc(adaptive.focused_test.phase)} · ${esc(adaptive.focused_test.provider || "—")} / ${esc(adaptive.focused_test.precision || "—")}</div><div class="panel-meta">${esc(adaptive.focused_test.reason || "—")}</div></div>`
    : "";
  const frontierRows = (adaptive.cost_sentinel?.frontier || []).map((entry) =>
    `<div class="frontier-row"><div><b>#${esc(entry.order)}</b> <code>${esc(entry.command || entry.check_id || "—")}</code><div class="component-id">${esc(entry.island || "workspace")}</div></div><div class="dep-state">${Number(entry.failure_rate_percent || 0).toFixed(1)}% ${esc(localized("overall failures", "总体失败"))}<div class="panel-meta">${esc(entry.marginal_failures)}/${esc(entry.marginal_samples)} ${esc(localized("marginal failures", "边际失败"))} · ${Number(entry.marginal_failure_rate_percent || 0).toFixed(1)}% · +${esc(entry.estimated_incremental_savings_ms)} ms ${esc(localized("estimated savings", "预计节省"))}</div></div></div>`
  ).join("");
  const sentinel = adaptive.cost_sentinel
    ? `<div class="info-card"><b>${esc(localized("Fail-fast frontier", "Fail-fast 前沿"))}</b><div class="panel-meta">${esc(adaptive.cost_sentinel.frontier?.length || 1)} ${esc(localized("bounded stages", "个有界阶段"))} · ${esc(adaptive.cost_sentinel.estimated_total_savings_ms || adaptive.cost_sentinel.estimated_savings_ms)} ms ${esc(localized("total estimated savings", "总预计节省"))}</div>${frontierRows || `<div><code>${esc(adaptive.cost_sentinel.command || adaptive.cost_sentinel.check_id || "—")}</code></div>`}<div class="panel-meta">${esc(adaptive.cost_sentinel.model || "—")} · ${esc(adaptive.cost_sentinel.provider || "—")} / ${esc(adaptive.cost_sentinel.precision || "—")}</div></div>`
    : "";
  const costEvaluation = adaptive.cost_evaluation;
  const activationTone = costEvaluation?.activation_state === "active" ? "good" : costEvaluation?.activation_state === "blocked" ? "warn" : "info";
  const activationLabel = {
    active: localized("backtest active", "回放验证已启用"),
    exploring: localized("cold-start exploration", "冷启动探索"),
    blocked: localized("backtest blocked", "回放阻断"),
  }[costEvaluation?.activation_state] || costEvaluation?.activation_state || "—";
  const evaluation = costEvaluation
    ? `<div class="info-card"><b>${esc(localized("Cost-model replay", "成本模型回放"))}</b><div class="pills">${pill(activationLabel, activationTone)}${pill(`${costEvaluation.candidate_model || "—"} vs ${costEvaluation.baseline_model || "—"}`)}</div><div class="metric">${Number(costEvaluation.net_savings_percent || 0).toFixed(1)}%</div><div class="panel-meta">${esc(localized(`${costEvaluation.net_savings_ms || 0} ms net · ${costEvaluation.wins || 0} wins / ${costEvaluation.ties || 0} ties / ${costEvaluation.losses || 0} losses`, `净收益 ${costEvaluation.net_savings_ms || 0} ms · ${costEvaluation.wins || 0} 胜 / ${costEvaluation.ties || 0} 平 / ${costEvaluation.losses || 0} 负`))}<br>${esc(localized(`${costEvaluation.evaluable_revisions || 0} evaluable revisions · minimum ${costEvaluation.minimum_evaluable_revisions || 0} · ${costEvaluation.activation_reason || "—"}`, `${costEvaluation.evaluable_revisions || 0} 个可评估 revision · 最少 ${costEvaluation.minimum_evaluable_revisions || 0} 个 · ${costEvaluation.activation_reason || "—"}`))}</div></div>`
    : "";
  const fallback = adaptive.fallback_reason
    ? `<div class="empty">${esc(adaptiveFallbackLabel(adaptive.fallback_reason))}</div>`
    : "";
  setHtml(
    "adaptiveVerification",
    els.adaptiveVerification,
    `${identity}<div class="adaptive-cards">${focused}${sentinel}${evaluation}</div>${fallback}${previewNote}`,
  );
}
function renderVerifiedLearning() {
  const learning = state.project?.verified_learning;
  if (!learning?.available) {
    setHtml(
      "verifiedLearning",
      els.verifiedLearning,
      `<div class="empty">${esc(localized("Verified learning evaluation is unavailable.", "验证学习评估当前不可用。"))}</div>`,
    );
    return;
  }
  const identity = `<div class="pills">${
    pill(`${learning.provider || "verified-change-history"} / ${learning.retrieval_precision || "heuristic"}`)
  }${pill(`${learning.retrieval_model || "verified-context-cochange-v3"} ↔ ${learning.baseline_model || "raw-count-v1"}`)}${
    pill(learning.evaluation_method || "global-temporal-ab-v2")
  }${pill(localized(`top-${learning.top_k || 0}`, `Top-${learning.top_k || 0}`))}${
    pill(localized("prompt / chain-of-thought free", "不保存提示词 / 思维链"), "good")
  }</div>`;
  if (!learning.records) {
    setHtml(
      "verifiedLearning",
      els.verifiedLearning,
      `${identity}<div class="empty">${esc(localized("Cold start: no verified context/change history has been learned yet.", "冷启动：目前还没有学到已验证的上下文/变更历史。"))}</div>`,
    );
    return;
  }
  const pct = (value) => `${Number(value || 0).toFixed(1)}%`;
  const pp = (value) => {
    const number = Number(value || 0);
    return `${number >= 0 ? "+" : ""}${number.toFixed(1)} pp`;
  };
  const cards = [
    [localized("Coverage", "覆盖率"), learning.coverage_percent, learning.baseline_coverage_percent, learning.coverage_delta_percent_points, localized(`${learning.prediction_cases}/${learning.evaluation_cases} candidate cases vs ${learning.baseline_prediction_cases}/${learning.evaluation_cases} baseline`, `候选模型 ${learning.prediction_cases}/${learning.evaluation_cases}，基线 ${learning.baseline_prediction_cases}/${learning.evaluation_cases}`)],
    [localized("Hit rate", "命中率"), learning.hit_rate_percent, learning.baseline_hit_rate_percent, learning.hit_rate_delta_percent_points, localized(`${learning.hit_cases}/${learning.prediction_cases} candidate hits vs ${learning.baseline_hit_cases}/${learning.baseline_prediction_cases} baseline`, `候选模型 ${learning.hit_cases}/${learning.prediction_cases} 命中，基线 ${learning.baseline_hit_cases}/${learning.baseline_prediction_cases}`)],
    [`Precision@${learning.top_k}`, learning.precision_at_k_percent, learning.baseline_precision_at_k_percent, learning.precision_at_k_delta_percent_points, localized(`${learning.true_positives}/${learning.predictions} candidate paths vs ${learning.baseline_true_positives}/${learning.baseline_predictions} baseline`, `候选模型 ${learning.true_positives}/${learning.predictions}，基线 ${learning.baseline_true_positives}/${learning.baseline_predictions}`)],
    [`Recall@${learning.top_k}`, learning.recall_at_k_percent, learning.baseline_recall_at_k_percent, learning.recall_at_k_delta_percent_points, localized(`${learning.true_positives}/${learning.expected_targets} candidate targets vs ${learning.baseline_true_positives}/${learning.expected_targets} baseline`, `候选模型找回 ${learning.true_positives}/${learning.expected_targets}，基线 ${learning.baseline_true_positives}/${learning.expected_targets}`)],
  ].map(([label, value, baseline, delta, detail]) =>
    `<div class="info-card"><b>${esc(label)}</b><div class="metric">${esc(pct(value))}</div><div class="panel-meta">${esc(localized(`baseline ${pct(baseline)} · Δ ${pp(delta)}`, `基线 ${pct(baseline)} · Δ ${pp(delta)}`))}<br>${esc(detail)}</div></div>`
  ).join("");
  const history = localized(
    `${learning.records} verified records · ${learning.full_records} full / ${learning.quick_records} quick · ${learning.live_paths}/${learning.unique_paths} paths live · ${learning.stale_path_references} stale references ignored`,
    `${learning.records} 条已验证记录 · ${learning.full_records} 条 full / ${learning.quick_records} 条 quick · ${learning.live_paths}/${learning.unique_paths} 个路径仍有效 · 已忽略 ${learning.stale_path_references} 个过期引用`,
  );
  setHtml(
    "verifiedLearning",
    els.verifiedLearning,
    `${identity}<div class="learning-cards">${cards}</div><div class="panel-meta card-gap">${esc(history)}</div>`,
  );
}
function renderChanges() {
  const items = state.project.changes || [],
    html = items.length
      ? changeTable(items, false)
      : `<div class="section empty">${
        esc(state.project.git_review?.available === true ? localized("Working tree is clean.", "工作树没有未提交变更。") : localized("Git review is unavailable. This is not proof of a clean working tree.", "Git 检查不可用，不能据此判断工作树没有变更。"))
      }</div>`;
  setHtml(
    "changes",
    els.changes,
    html,
    () =>
      els.changes.querySelectorAll("[data-req]").forEach((link) =>
        link.addEventListener("click", () => {
          state.filter = "all"; els.search.value = "";
          document.querySelectorAll(".filter").forEach(button => { button.classList.toggle("active", button.dataset.filter === "all"); button.setAttribute("aria-pressed", String(button.dataset.filter === "all")); });
          state.selected = link.dataset.req;
          revealSection("requirementsSection");
          invalidate("requirements", "detail");
          renderRequirements();
          renderDetail();
          els.detail.scrollIntoView({ behavior: "smooth", block: "start" });
        })
      ),
  );
}
