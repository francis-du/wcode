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
  if (!state.selected || !all.some((r) => r.id === state.selected)) {
    state.selected = (all.find((r) => r.changed) || all[0] || {}).id || "";
  }
  const html = items.map((r) => {
    const dependencies = r.dependency_alignment || [],
      blocking = dependencies.filter((d) => d.blocking).length,
      advisory = dependencies.filter((d) =>
        !d.blocking && d.status !== "aligned"
      ).length + (r.drift || []).length;
    return `<button class="req ${
      r.id === state.selected ? "selected" : ""
    }" data-id="${esc(r.id)}"><div class="req-top"><span class="req-id">${
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
    () =>
      els.requirements.querySelectorAll(".req").forEach((button) =>
        button.addEventListener("click", () => {
          state.selected = button.dataset.id;
          invalidate("requirements", "detail");
          renderRequirements();
          renderDetail();
        })
      ),
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
      }</div></div><div class="arrow">→</div><div><b>${
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
    localized(
      `${verified}/${ver.length} verification references`,
      `${verified}/${ver.length} 个验证引用`,
    ),
    ver.length && verified === ver.length
      ? localized("acceptance mapped", "验收已映射")
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
    steps.map(([label, value, detail, tone]) =>
      `<div class="flow-step"><div class="flow-label">${
        esc(label)
      }</div><div class="flow-value ${tone}">${
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
  }</div><div class="arch-flow"><div class="node"><b>${
    esc(r.title)
  }</b><small>${esc(r.id)} · ${
    esc(t("requirement"))
  }</small></div><div class="connector">→</div><div class="node-stack">${
    designComponents(r)
  }</div><div class="connector">→</div><div class="node-stack">${
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
  return `<table class="table"><thead><tr><th>${esc(t("Path"))}</th><th>${
    esc(t("Status"))
  }</th><th>${esc(t("Scope"))}</th><th>${esc(t("Diff"))}</th>${
    compact ? "" : `<th>${esc(t("Requirements"))}</th>`
  }</tr></thead><tbody>${
    items.slice(0, 120).map((item) =>
      `<tr><td><div class="change-path"><code>${
        esc(item.path)
      }</code></div></td><td>${esc(statusLabel(item.status))}${
        item.untracked ? ` · ${esc(t("untracked"))}` : ""
      }</td><td>${esc(item.scope || "—")}</td><td>${changeNums(item)}</td>${
        compact
          ? ""
          : `<td>${
            (item.affected_requirements || []).slice(0, 6).map((id) =>
              `<a class="click-req" data-req="${esc(id)}">${esc(id)}</a>`
            ).join(" · ") || "—"
          }</td>`
      }</tr>`
    ).join("")
  }</tbody></table>`;
}
function renderChanges() {
  const items = state.project.changes || [],
    html = items.length
      ? changeTable(items, false)
      : `<div class="section empty">${
        esc(t("Working tree is clean or Git review is unavailable."))
      }</div>`;
  setHtml(
    "changes",
    els.changes,
    html,
    () =>
      els.changes.querySelectorAll("[data-req]").forEach((link) =>
        link.addEventListener("click", () => {
          state.selected = link.dataset.req;
          invalidate("requirements", "detail");
          renderRequirements();
          renderDetail();
          els.detail.scrollIntoView({ behavior: "smooth", block: "start" });
        })
      ),
  );
}
