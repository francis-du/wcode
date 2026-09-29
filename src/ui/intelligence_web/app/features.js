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
      return `<tr class="change-row" data-tone="${esc(tone)}"><td data-label="${esc(t("Path"))}"><div class="change-path">${compact ? `<code>${esc(item.path)}</code>` : `<button type="button" class="change-open" data-change-path="${esc(item.path)}">${esc(item.path)}</button>`}</div></td><td data-label="${esc(t("Status"))}"><span class="change-status">${pill(statusLabel(item.status), tone)}${
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
    `${html}<div id="changeInspector"></div>`,
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
  els.changes.querySelectorAll("[data-change-path]").forEach(button => {
    button.onclick = () => { void openChangeInspection(button.dataset.changePath); };
  });
  renderChangeInspector();
}

function clearChangeInspection() {
  state.changeInspection?.controller?.abort();
  state.changeInspection?.impactController?.abort();
  state.changeInspection = null;
  state.changeRequestSequence = (state.changeRequestSequence || 0) + 1;
  state.changeImpactSequence = (state.changeImpactSequence || 0) + 1;
  const host = q("#changeInspector");
  if (host) host.innerHTML = "";
}
function invalidateChangeInspection() {
  const current = state.changeInspection;
  if (!current) return;
  current.controller?.abort(); current.impactController?.abort();
  state.changeRequestSequence = (state.changeRequestSequence || 0) + 1;
  state.changeImpactSequence = (state.changeImpactSequence || 0) + 1;
  current.data = null; current.symbolImpact = null; current.relationImpact = null; current.loading = false;
  current.error = localized("Project state changed or could not be refreshed. Reload this file.", "项目状态已更新或刷新失败，请重新读取此文件。");
  renderChangeInspector();
}
function changeLayerLabel(layer) {
  return ({working: localized("HEAD → working tree", "HEAD → 工作树"),
    staged: localized("HEAD → index (staged)", "HEAD → 暂存区"),
    unstaged: localized("Index → working tree", "暂存区 → 工作树")})[layer] || layer;
}
function validChangeRanges(view) {
  const valid = ranges => Array.isArray(ranges) && ranges.length <= 256 && ranges.every((range, index) => {
    const start = range?.start_line, end = range?.end_line;
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 1 || end < start) return false;
    return index === 0 || ranges[index - 1]?.end_line < start;
  });
  return valid(view?.before_changed_ranges) && valid(view?.after_changed_ranges)
    && typeof view?.changed_ranges_truncated === "boolean"
    && typeof view?.after_source_matches_worktree === "boolean";
}
function validChangeSymbolImpact(impact, view) {
  if (impact == null) return view?.after_source_matches_worktree !== false;
  const validSymbols = (symbols, sourceRanges) => Array.isArray(symbols) && Array.isArray(sourceRanges) && symbols.length <= 128 && symbols.every(symbol => {
    if (typeof symbol?.node_id !== "string" || !symbol.node_id.startsWith("symbol:ts:") || typeof symbol.name !== "string"
      || typeof symbol.qualified_name !== "string" || typeof symbol.kind !== "string"
      || !Number.isInteger(symbol.start_line) || symbol.start_line < 1
      || !Number.isInteger(symbol.end_line) || symbol.end_line < symbol.start_line || typeof symbol.counterpart_only !== "boolean"
      || (symbol.definition_change != null && !["modified", "added", "removed", "unknown"].includes(symbol.definition_change))
      || !Array.isArray(symbol.changed_ranges) || symbol.changed_ranges.length > 256) return false;
    const expected = sourceRanges.flatMap(range => {
      const start = Math.max(symbol.start_line, range.start_line), end = Math.min(symbol.end_line, range.end_line);
      return start <= end ? [{start_line:start,end_line:end}] : [];
    });
    const actualValid = symbol.changed_ranges.every((range, index) => Number.isInteger(range?.start_line) && Number.isInteger(range?.end_line)
      && range.start_line >= symbol.start_line && range.end_line <= symbol.end_line && range.end_line >= range.start_line
      && (index === 0 || symbol.changed_ranges[index - 1].end_line < range.start_line));
    if (!actualValid) return false;
    if (symbol.counterpart_only) return expected.length === 0 && symbol.changed_ranges.length === 0;
    return expected.length > 0 && symbol.changed_ranges.length === expected.length
      && symbol.changed_ranges.every((range, index) => range.start_line === expected[index].start_line && range.end_line === expected[index].end_line);
  });
  if (typeof impact !== "object" || impact.path !== view.path || impact.snapshot_id !== view.snapshot_id
    || impact.precision !== "syntax" || impact.provider !== "tree-sitter"
    || !["after_line_overlap", "before_after_line_overlap"].includes(impact.mapping)
    || typeof impact.before_symbols_available !== "boolean"
    || typeof impact.partial !== "boolean" || !validSymbols(impact.after_symbols, view.after_changed_ranges)) return false;
  if (impact.source_state === "worktree") {
    if (impact.source_sha256 !== view.worktree_sha256 || view.after_source_matches_worktree === false) return false;
  } else if (impact.source_state === "index") {
    if (view.layer !== "staged" || !/^[a-f0-9]{64}$/i.test(impact.source_sha256 || "")) return false;
  } else if (impact.source_state !== null || typeof impact.unavailable_reason !== "string") {
    return false;
  }
  if (impact.mapping === "before_after_line_overlap") {
    if (!["head", "index"].includes(impact.before_source_state)
      || !/^[a-f0-9]{64}$/i.test(impact.before_source_sha256 || "")
      || !validSymbols(impact.before_symbols, view.before_changed_ranges)) return false;
    if (impact.before_symbols_available) {
      if (impact.before_unavailable_reason !== null) return false;
    } else if (typeof impact.before_unavailable_reason !== "string") return false;
    if (impact.definition_change_basis != null && (impact.definition_change_basis !== "qualified_name_kind_complete_syntax_outlines" || ![...(impact.before_symbols || []), ...(impact.after_symbols || [])].every(symbol => ["modified", "added", "removed", "unknown"].includes(symbol.definition_change)))) return false;
  } else if (impact.before_symbols_available !== false) return false;
  return true;
}
function directChangedSymbolLine(symbol) {
  if (symbol?.counterpart_only !== false || !Array.isArray(symbol.changed_ranges) || !symbol.changed_ranges.length) return null;
  for (const range of symbol.changed_ranges) {
    const start = range?.start_line, end = range?.end_line;
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 1 || end < start) return null;
  }
  return symbol.changed_ranges[0].start_line;
}
function changeSourceAction(symbol, side) {
  const line = directChangedSymbolLine(symbol);
  if (!line || !["before", "after"].includes(side)) return "";
  return `<button type="button" class="change-symbol-link" data-change-source-side="${side}" data-change-source-node="${esc(symbol.node_id)}" data-change-source-line="${line}">${esc(localized("Reveal changed line", "定位变更行"))}</button>`;
}
function changeSymbolImpactPanel(impact, repositoryRevision) {
  if (!impact) return "";
  const note = impact.mapping === "before_after_line_overlap"
    ? impact.counterpart_basis === "qualified_name_kind_syntax"
      ? localized(
        "Changed definitions come from exact before/after syntax overlap; unique same qualified-name + kind counterparts are shown only to keep addition/deletion-only body edits paired. This is not semantic impact or rename proof.",
        "变化定义来自精确改前/改后快照的语法范围重叠；仅在限定名与类型唯一一致时补出另一侧对应定义，用于配对纯新增/纯删除的函数体修改。这不是语义影响或重命名证明。",
      )
      : localized(
        "Syntax range overlap on exact before/after snapshots; this shows changed definitions, not semantic impact or rename proof.",
        "这里只表示精确改前/改后快照上的语法范围重叠；可定位发生变化的定义，但不是语义影响或重命名证明。",
      )
    : localized(
      "Syntax range overlap on the exact after-state snapshot only; before-state mapping is unavailable and this is not semantic impact proof.",
      "这里只表示精确改后快照的语法范围重叠；改前映射不可用，也不是语义影响证明。",
    );
  const title = impact.mapping === "before_after_line_overlap"
    ? localized("Syntax-overlapping changed definitions", "变更定义的语法范围重叠")
    : impact.source_state === "index"
      ? localized("Syntax-overlapping staged symbols", "暂存快照语法重叠符号")
      : localized("Syntax-overlapping current symbols", "当前语法重叠符号");
  if (impact.unavailable_reason && impact.mapping !== "before_after_line_overlap") {
    const unavailable = impact.unavailable_reason === "exact_after_source_unavailable"
      ? localized("The exact staged after-state could not be read completely, so no symbol identity is inferred from newer working-tree bytes.", "无法完整读取精确暂存后的源码，因此不会从更新的工作树字节推断符号身份。")
      : localized("Current symbol mapping is unavailable for this source state.", "当前源码状态无法生成符号映射。");
    return `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(note)}</p><p>${esc(unavailable)}</p></section>`;
  }
  const sourceNote = impact.source_state === "index"
    ? localized("After-state mapped from the exact staged index snapshot, not from the current worktree.", "改后状态映射来自精确暂存区快照，而不是当前工作树。")
    : impact.source_state === "worktree"
      ? localized("After-state mapped from the current working-tree snapshot.", "改后状态映射来自当前工作树快照。")
      : localized("After-state source is unavailable; the before snapshot remains independently mapped.", "改后源码不可用；改前快照仍保持独立映射。");
  const definitionChangeNote = symbol => impact.definition_change_basis === "qualified_name_kind_complete_syntax_outlines" && ({modified:localized("Modified definition · syntax only", "定义已修改 · 仅语法"),added:localized("Added definition · syntax only", "新增定义 · 仅语法"),removed:localized("Removed definition · syntax only", "移除定义 · 仅语法")})[symbol.definition_change] ? `<span class="panel-meta">${esc(({modified:localized("Modified definition · syntax only", "定义已修改 · 仅语法"),added:localized("Added definition · syntax only", "新增定义 · 仅语法"),removed:localized("Removed definition · syntax only", "移除定义 · 仅语法")})[symbol.definition_change])}</span>` : "";
  const beforeItems = impact.before_symbols_available ? (impact.before_symbols || []).map(symbol => `<div class="change-symbol-before"><strong>${esc(symbol.qualified_name || symbol.name)}</strong><span>${esc(symbol.kind)} · ${symbol.start_line}–${symbol.end_line}</span>${definitionChangeNote(symbol)}${symbol.counterpart_only ? `<span class="panel-meta">${esc(localized("Paired counterpart · no changed-line overlap", "配对对应定义 · 无变更行重叠"))}</span>` : ""}${changeSourceAction(symbol, "before")}</div>`).join("") : "";
  const afterNavigable = impact.source_state === "worktree" && validChangeRepositoryRevision(repositoryRevision);
  const signatureNote = symbol => { const peers = impact.before_symbols_available ? (impact.before_symbols || []).filter(before => before.qualified_name === symbol.qualified_name && before.kind === symbol.kind) : []; if (peers.length !== 1 || typeof symbol.signature !== "string" || typeof peers[0]?.signature !== "string") return ""; if (symbol.signature_redacted !== false || peers[0].signature_redacted !== false) return `<span class="panel-meta">${esc(localized("Signature comparison unavailable · syntax signature only", "签名比较不可用 · 仅语法签名"))}</span>`; return `<span class="panel-meta">${esc(symbol.signature !== peers[0].signature ? localized("Signature changed · syntax signature only", "签名已变化 · 仅语法签名") : localized("Signature unchanged · syntax signature only", "签名未变化 · 仅语法签名"))}</span>`; };
  const afterItems = impact.after_symbols.map(symbol => afterNavigable && symbol.counterpart_only === false
    ? `<div class="change-symbol-actions"><button type="button" class="change-symbol-link" data-change-symbol="${esc(symbol.node_id)}"><strong>${esc(symbol.qualified_name || symbol.name)}</strong><span>${esc(symbol.kind)} · ${symbol.start_line}–${symbol.end_line}</span>${definitionChangeNote(symbol)}${signatureNote(symbol)}</button><button type="button" class="change-symbol-link" data-change-impact="${esc(symbol.node_id)}">${esc(localized("Explain impact", "解释影响"))}</button>${changeSourceAction(symbol, "after")}</div>`
    : `<div class="change-symbol-before"><strong>${esc(symbol.qualified_name || symbol.name)}</strong><span>${esc(symbol.kind)} · ${symbol.start_line}–${symbol.end_line}</span>${definitionChangeNote(symbol)}${symbol.counterpart_only ? `<span class="panel-meta">${esc(localized("Paired counterpart · no changed-line overlap", "配对对应定义 · 无变更行重叠"))}</span>` : ""}${signatureNote(symbol)}${changeSourceAction(symbol, "after")}</div>`).join("");
  const navigationNote = impact.source_state === "index" && impact.after_symbols.length
    ? localized(
      "Exact staged-snapshot symbols are not linked to the current graph because that graph represents different working-tree bytes.",
      "精确暂存快照里的符号不会链接到当前代码图，因为当前代码图代表的是不同的工作树字节。",
    )
    : "";
  const paired = impact.mapping === "before_after_line_overlap"
    ? `<div class="change-symbol-pair"><div><b>${esc(localized("Before", "改前"))}</b>${beforeItems || `<p>${esc(localized("No before-state definition overlaps the removed lines.", "没有改前定义范围与删除行重叠。"))}</p>`}</div><div><b>${esc(localized("After", "改后"))}</b>${afterItems || `<p>${esc(localized("No after-state definition overlaps the added lines.", "没有改后定义范围与新增行重叠。"))}</p>`}</div></div>`
    : (afterItems || `<p>${esc(localized("No definition range overlaps the returned after-change lines.", "没有定义范围与已返回的改后变更行重叠。"))}</p>`);
  const partial = impact.partial ? `<p class="warn">${esc(localized("Symbol overlap is partial because the returned change or outline was bounded.", "由于返回的变更或符号轮廓有界，符号重叠结果并不完整。"))}</p>` : "";
  return `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(note)} ${esc(sourceNote)} ${esc(navigationNote)}</p>${partial}${paired}</section>`;
}

function validChangeRepositoryRevision(revision) {
  return revision && typeof revision.code === "string" && revision.code.length > 0 && revision.code.length <= 256
    && (revision.design == null || (typeof revision.design === "string" && revision.design.length > 0 && revision.design.length <= 256));
}
function validProofCount(value) { return Number.isInteger(value) && value >= 0; }
function validChangeProofPayload(proof) {
  const effective = proof?.effective, acceptance = proof?.acceptance;
  const counts = [proof?.current_evidence,proof?.current_passed,proof?.current_failed,proof?.current_inconclusive,proof?.current_disagreed,
    proof?.current_verification_plans,proof?.current_verification_ready,proof?.current_verification_blocked,effective?.total,effective?.passed,effective?.failed,effective?.inconclusive,effective?.disagreed,
    acceptance?.total,acceptance?.mapped,acceptance?.executed,acceptance?.passed,acceptance?.fresh];
  if (!effective || !acceptance || !Array.isArray(effective.items) || counts.some(value => !validProofCount(value))
    || typeof effective.truncated !== "boolean" || typeof proof.evidence_scan_truncated !== "boolean") return false;
  if (effective.passed + effective.failed + effective.inconclusive + effective.disagreed !== effective.total
    || proof.current_passed + proof.current_failed + proof.current_inconclusive + proof.current_disagreed !== proof.current_evidence
    || proof.current_verification_ready + proof.current_verification_blocked !== proof.current_verification_plans
    || acceptance.mapped > acceptance.total || acceptance.executed > acceptance.total
    || acceptance.passed > acceptance.executed || acceptance.fresh > acceptance.executed
    || effective.items.length !== Math.min(effective.total, 24) || effective.truncated !== (effective.total > 24)) return false;
  return effective.items.every(item => item && typeof item.subject === "string" && typeof item.producer === "string"
    && ["pass","fail","inconclusive","disagree"].includes(item.result) && Number.isInteger(item.timestamp_ms) && item.timestamp_ms >= 0);
}
function changeRiskPanel(risk, revision) {
  const title = localized("Matched project risk", "匹配项目风险");
  if (!validChangeRepositoryRevision(revision) || !risk || risk.workspace !== state.current
    || !validChangeRepositoryRevision(risk.revision)
    || risk.revision.code !== revision.code || (risk.revision.design ?? null) !== (revision.design ?? null)
    || !["low","medium","high","critical"].includes(risk.level) || !Array.isArray(risk.risks))
    return `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(localized("Matching project risk status is unavailable.", "匹配的项目风险状态不可用。"))}</p></section>`;
  const rows = risk.risks.slice(0,8).map(item => item && typeof item.summary === "string" ? `<span class="change-symbol-before"><code>${esc(item.level || "risk")}</code><span>${esc(item.category || "risk")}${item.subject ? ` · ${esc(item.subject)}` : ""}</span><span class="panel-meta">${esc(item.summary)}</span></span>` : "").join("");
  const partial = risk.risks.length > 8 || risk.drift?.truncated === true || risk.bug_patterns?.truncated === true;
  const candidates = validProofCount(risk.bug_patterns?.matches) ? risk.bug_patterns.matches : 0;
  const note = localized("Shown only when the risk status itself carries this exact captured code + Design revision; risk is context, not Verification Evidence or symbol-level proof.", "仅当风险状态自身携带与本次捕获完全一致的代码 + Design 版本时展示；风险只是上下文，不是 Verification Evidence 或符号级证明。");
  return `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(note)}</p><p class="panel-meta">${esc(localized(`${risk.level} risk · ${risk.risks.length} structured risks · ${candidates} heuristic bug candidates`, `${risk.level} 风险 · ${risk.risks.length} 项结构化风险 · ${candidates} 项启发式缺陷候选`))}</p>${partial ? `<p class="warn">${esc(localized("Risk coverage is bounded/partial.", "风险覆盖有界或不完整。"))}</p>` : ""}${rows}</section>`;
}
function changeProofPanel(current) {
  const revision = current.repositoryRevision, proof = state.project?.proof;
  const title = localized("Captured-revision verification", "捕获版本验证");
  const unavailable = () => `<section class="change-symbol-impact"><h4>${esc(title)}</h4><p class="panel-meta">${esc(localized("No matching current-version proof is available for this captured repository revision. Historical passes and mapped tests are not shown as current proof.", "当前捕获的仓库版本没有匹配的当前版本证明；历史通过和测试映射不会被显示成当前证明。"))}</p></section>`;
  const projectRevision = state.project?.repository_revision;
  if (!validChangeRepositoryRevision(revision) || !validChangeRepositoryRevision(projectRevision) || !proof
    || projectRevision.code !== revision.code || (projectRevision.design ?? null) !== (revision.design ?? null)
    || proof.revision_code !== revision.code || (proof.revision_design ?? null) !== (revision.design ?? null)) return unavailable();
  if (!validChangeProofPayload(proof)) return unavailable();
  const effective = proof.effective, acceptance = proof.acceptance;
  const summary = `<p class="panel-meta">${esc(localized(`${effective.passed} passed · ${effective.failed} failed · ${effective.inconclusive} inconclusive · ${effective.disagreed} disagreed`, `${effective.passed} 项通过 · ${effective.failed} 项失败 · ${effective.inconclusive} 项未定 · ${effective.disagreed} 项分歧`))}</p>`;
  const acceptanceSummary = `<p class="panel-meta">${esc(localized(`Acceptance: ${acceptance.mapped}/${acceptance.total} mapped · ${acceptance.executed} executed · ${acceptance.passed} passed · ${acceptance.fresh} current-revision`, `验收：${acceptance.mapped}/${acceptance.total} 已映射 · ${acceptance.executed} 已执行 · ${acceptance.passed} 已通过 · ${acceptance.fresh} 当前版本`))}</p>`;
  const plans = `<p class="panel-meta">${esc(localized(`${proof.current_verification_ready}/${proof.current_verification_plans} current plans ready · ${proof.current_verification_blocked} blocked`, `${proof.current_verification_ready}/${proof.current_verification_plans} 个当前计划就绪 · ${proof.current_verification_blocked} 个阻塞`))}</p>`;
  const rows = effective.items.slice(0,12).map(item => {
    if (!item || typeof item.producer !== "string" || typeof item.result !== "string") return "";
    const subject = typeof item.subject === "string" ? item.subject : "";
    const detail = typeof item.summary === "string" ? item.summary : "";
    return `<span class="change-symbol-before"><code>${esc(item.result)}</code><span>${esc(item.producer)}</span><span class="panel-meta">${esc(subject)}${detail ? ` · ${esc(detail)}` : ""}</span></span>`;
  }).join("");
  const note = `<p class="panel-meta">${esc(localized("This is project-level Verification Evidence for the exact captured code + Design revision; it is not proof that the selected symbol is correct.", "这里只展示与精确捕获的代码 + Design 版本一致的项目级 Verification Evidence；它不证明所选符号本身正确。"))}</p>`;
  const truncation = effective.truncated || proof.evidence_scan_truncated ? `<p class="warn">${esc(localized("Evidence coverage is bounded/partial.", "证据覆盖有界或不完整。"))}</p>` : "";
  return `<section class="change-symbol-impact"><h4>${esc(title)}</h4>${note}${summary}${acceptanceSummary}${plans}${truncation}${rows || `<span class="panel-meta">${esc(localized("No effective evidence items returned.", "没有返回有效证据项。"))}</span>`}</section>` + changeRiskPanel(state.project?.risk, revision);
}
function revealChangeSourceLine(side, nodeId, line) {
  const current = state.changeInspection, view = current?.data, mapping = current?.symbolImpact;
  const sourceLine = Number(line);
  if (!view || !mapping || !["before", "after"].includes(side) || !Number.isInteger(sourceLine) || sourceLine < 1
    || mapping.path !== view.path || mapping.snapshot_id !== view.snapshot_id) return false;
  const symbols = side === "before" ? mapping.before_symbols : mapping.after_symbols;
  const symbol = Array.isArray(symbols) ? symbols.find(item => item?.node_id === nodeId) : null;
  const changedLine = directChangedSymbolLine(symbol);
  if (!changedLine || !symbol.changed_ranges.some(range => sourceLine >= Number(range.start_line) && sourceLine <= Number(range.end_line))) return false;
  const host = q("#changeInspector");
  const target = host?.querySelector(`.change-source-line[data-change-${side}-line="${sourceLine}"]`);
  if (!target) return false;
  target.scrollIntoView?.({block: "center"});
  target.focus?.({preventScroll: true});
  return true;
}
function changeDisplayRows(view) {
  let oldLine = null, newLine = null, inHunk = false;
  return view.content.split("\n").map((text, index) => {
    if (view.kind === "untracked_source") return {before: "", after: index + 1, text, tone: "context"};
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) { oldLine = Number(hunk[1]); newLine = Number(hunk[2]); inHunk = true; }
    else if (text.startsWith("diff --git ")) inHunk = false;
    const row = {before: "", after: "", text, tone: "meta"};
    if (inHunk && !hunk) {
      if (text.startsWith("-")) { row.before = oldLine++; row.tone = "removed"; }
      else if (text.startsWith("+")) { row.after = newLine++; row.tone = "added"; }
      else if (text.startsWith(" ")) { row.before = oldLine++; row.after = newLine++; row.tone = "context"; }
    }
    return row;
  });
}
async function openChangeInspection(path, layer = "working", fresh = false) {
  if (typeof path !== "string" || !path || !["working", "staged", "unstaged"].includes(layer)) return;
  const previous = state.changeInspection;
  const expected = !fresh && previous?.path === path
    ? (previous.snapshotId || previous.data?.snapshot_id || null)
    : null;
  previous?.controller?.abort();
  const sequence = state.changeRequestSequence = (state.changeRequestSequence || 0) + 1;
  const workspace = state.current, epoch = state.workspaceEpoch, controller = new AbortController();
  state.changeInspection = {path, layer, snapshotId: expected, data: null, symbolImpact: null, relationImpact: null, repositoryRevision: null,
    impactNodeId: "", impactLoading: false, impactError: "", impactController: null, error: "", loading: true, controller};
  renderChangeInspector();
  const params = new URLSearchParams({path, layer});
  if (expected) params.set("expected_snapshot", expected);
  const active = () => state.current === workspace && state.workspaceEpoch === epoch
    && state.changeRequestSequence === sequence && !controller.signal.aborted;
  try {
    const result = await uiJson(`/intelligence/change-detail?${params}`, "GET", undefined,
      {workspace, signal: controller.signal});
    if (!active()) return;
    const view = result.change;
    if (result.workspace !== workspace || (result.repository_revision != null && !validChangeRepositoryRevision(result.repository_revision))
      || view?.path !== path || view?.layer !== layer
      || !/^[a-f0-9]{64}$/i.test(view?.snapshot_id || "")
      || !["unified_diff", "untracked_source", "binary"].includes(view?.kind)
      || typeof view.content !== "string" || view.content.length > 65536
      || !validChangeRanges(view) || !validChangeSymbolImpact(result.symbol_impact, view)
      || typeof view.truncated !== "boolean" || typeof view.redacted !== "boolean") {
      throw new Error(localized("Invalid change snapshot", "变更快照无效"));
    }
    state.changeInspection.data = view;
    state.changeInspection.symbolImpact = result.symbol_impact || null;
    state.changeInspection.repositoryRevision = result.repository_revision || null;
    state.changeInspection.snapshotId = view.snapshot_id;
  } catch (error) {
    if (!active()) return;
    state.changeInspection.error = error.status === 409
      ? localized("The file or index changed. Reload instead of mixing versions.", "文件或暂存区已变化，请重新读取，不能混合版本。")
      : requestFailureMessage(error);
  } finally {
    if (active()) { state.changeInspection.loading = false; renderChangeInspector(); }
  }
}
function renderChangeInspector() {
  const host = q("#changeInspector"), current = state.changeInspection;
  if (!host || !current) { if (host) host.innerHTML = ""; return; }
  const view = current.data;
  const layers = ["working", "staged", "unstaged"].map(layer => `<button type="button" data-change-layer="${layer}" aria-pressed="${current.layer === layer}"${current.loading ? " disabled" : ""}>${esc(changeLayerLabel(layer))}</button>`).join("");
  let body;
  if (current.loading) body = `<p role="status">${esc(localized("Reading bounded change snapshot…", "正在读取有界变更快照…"))}</p>`;
  else if (current.error) body = `<p class="bad" role="alert">${esc(current.error)}</p>`;
  else if (!view) body = "";
  else if (view.kind === "binary") body = `<p>${esc(localized("Binary difference; text preview is unavailable.", "二进制差异，不能作为文本预览。"))}</p>`;
  else {
    const rows = changeDisplayRows(view), limited = rows.length > 1500;
    const warning = view.truncated || view.changed_ranges_truncated || limited || view.redacted
      ? `<p class="warn">${esc(localized("Partial or redacted preview — not a complete source view; changed-line ranges may also be partial.", "部分内容或已脱敏预览，不代表完整源码；变更行范围也可能不完整。"))}</p>` : "";
    const label = view.kind === "untracked_source"
      ? localized("Untracked source; no committed baseline. Right gutter is the source line.", "未跟踪源码，没有已提交基线；右侧行号是源码行。")
      : localized("Unified diff · left: before line · right: after line. Context is intentionally bounded.", "统一差异 · 左侧为改前行号，右侧为改后行号；上下文有界。");
    body = `${warning}<p class="panel-meta">${esc(label)}</p>` + (view.content.length
      ? `<pre class="change-source" tabindex="0" role="region" aria-label="${esc(localized("Read-only source difference", "只读源码差异"))}">${rows.slice(0,1500).map(row => { const before = Number.isInteger(row.before) ? ` data-change-before-line="${row.before}"` : ""; const after = Number.isInteger(row.after) ? ` data-change-after-line="${row.after}"` : ""; return `<span class="change-source-line ${row.tone}" tabindex="-1"${before}${after}><span class="change-gutter">${row.before}</span><span class="change-gutter">${row.after}</span><span>${esc(row.text) || " "}</span></span>`; }).join("")}</pre>`
      : `<p>${esc(localized("No differences in this selected layer. This does not mean every layer is clean.", "所选层没有差异，不代表其他层也没有改动。"))}</p>`);
    body += changeSymbolImpactPanel(current.symbolImpact, current.repositoryRevision) + changeRelationImpactPanel(current) + changeProofPanel(current);
  }
  const repositoryRevision = current.repositoryRevision;
  const identity = view ? `<details class="change-identity"><summary>${esc(localized("Captured snapshot identity (not continuous live proof)", "本次读取的快照身份（不是持续实时证明）"))}</summary><code>HEAD ${esc(view.head || "—")}<br>index ${esc(view.index_fingerprint || "—")}<br>worktree ${esc(view.worktree_sha256 || localized("missing", "不存在"))}<br>snapshot ${esc(view.snapshot_id)}<br>repository code ${esc(repositoryRevision?.code || "—")}<br>repository Design ${esc(repositoryRevision?.design || "—")}</code></details>` : "";
  host.innerHTML = `<section class="change-inspector"><header><div><h3>${esc(localized("Read-only code changes", "只读代码变更"))}</h3><code>${esc(current.path)}</code></div><button type="button" data-change-close>${esc(localized("Close", "关闭"))}</button></header><div class="change-toolbar">${layers}<button type="button" data-change-reload>${esc(localized("Reload file", "重新读取"))}</button></div>${body}${identity}</section>`;
  host.querySelectorAll("[data-change-layer]").forEach(button => {
    button.onclick = () => { void openChangeInspection(current.path, button.dataset.changeLayer); };
  });
  host.querySelector("[data-change-close]")?.addEventListener("click", clearChangeInspection);
  host.querySelector("[data-change-reload]")?.addEventListener("click", () => {
    void openChangeInspection(current.path, current.layer, true);
  });
  host.querySelectorAll("[data-change-symbol]").forEach(button => {
    button.onclick = () => { void openChangeSymbolInGraph(button.dataset.changeSymbol); };
  });
  host.querySelectorAll("[data-change-impact]").forEach(button => { button.onclick = () => { void openChangeSymbolImpact(button.dataset.changeImpact); }; });
  host.querySelectorAll("[data-change-source-line]").forEach(button => { button.onclick = () => { revealChangeSourceLine(button.dataset.changeSourceSide, button.dataset.changeSourceNode, button.dataset.changeSourceLine); }; });
  host.querySelectorAll("[data-change-relation-node]").forEach(button => { button.onclick = () => { void openChangeRelationNodeInGraph(button.dataset.changeRelationNode); }; });
}
