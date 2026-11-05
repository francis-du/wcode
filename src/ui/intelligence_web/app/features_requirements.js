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
    (all.length === 0 && state.project?.design_valid === false
      ? `<div class="empty">${esc(localized("Design State is not initialized or failed validation, so there are no requirements to observe yet.", "设计状态未初始化或校验失败，暂时没有可观测的需求。"))}</div>`
      : state.filter === "changed" && state.project?.git_review?.available !== true
        ? `<div class="empty">${esc(localized(
            "Working-tree review is unavailable, so no requirement can be shown as changed. This is not a clean change state.",
            "工作树检查不可用，无法判定哪些需求被变更；这不表示变更状态干净。",
          ))}</div>`
        : `<div class="empty">${esc(t("No requirements match this filter."))}</div>`);
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
function implementationRows(items, sourcePaths = repositorySourcePaths()) {
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
      }</div>${designSourceAction(item, sourcePaths)}${codeReaderSourceAction(item, sourcePaths)}</div>`
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
function actualComponents(r, sourcePaths) {
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
    }</div></div>${implementationRows(component.implementation, sourcePaths)}</div>`
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
function verificationBlock(r, sourcePaths) {
  if (!r.acceptance.length) {
    return `<div class="empty">${esc(t("No acceptance criteria."))}</div>`;
  }
  return `<div class="verification">${
    r.acceptance.map((a) =>
      `<div class="verification-item"><div class="req-top"><b>${
        esc(a.title)
      }</b><code>${esc(a.id)}</code></div><div class="statement">${
        esc(a.statement)
      }</div>${implementationRows(a.verification, sourcePaths)}</div>`
    ).join("")
  }</div>`;
}
function featureFlow(r) {
  const impl = (r.components || []).flatMap((c) => c.implementation || []),
    resolved = impl.filter((i) => i.resolved).length,
    ver = (r.acceptance || []).flatMap((a) => a.verification || []),
    verified = ver.filter((i) => i.resolved).length,
    blockers = r.convergence_blockers || [],
    reviewAvailable = state.project?.git_review?.available === true;
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
    reviewAvailable
      ? r.changed
        ? localized("changing", "变更中")
        : localized("no mapped change", "无映射变更")
      : localized("change state unknown", "变更状态未知"),
    reviewAvailable
      ? r.changed
        ? `+${r.change_additions}/-${r.change_deletions}`
        : localized("working tree stable", "工作树稳定")
      : localized("working-tree review unavailable", "工作树检查不可用"),
    !reviewAvailable || r.changed ? "warn" : "good",
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
// Explicit `path::symbol` design mappings are the only source that ties a
// requirement to a definition. Unmapped symbols stay unmapped; nothing here
// guesses an owner from a name that the design never declared.
function requirementSymbolMap(path) {
  const project = state.project;
  if (!project || project.workspace !== state.current || typeof path !== "string") {
    return new Map();
  }
  const matches = new Map();
  for (const requirement of project.requirements || []) {
    if (!requirement?.id) continue;
    const references = [
      ...(requirement.components || []).flatMap(component => component.implementation || []),
      ...(requirement.acceptance || []).flatMap(criterion => criterion.verification || []),
    ];
    for (const reference of references) {
      if (designReferencePath(reference) !== path) continue;
      const symbol = typeof reference?.symbol === "string" ? reference.symbol : "";
      if (!symbol) continue;
      if (!matches.has(symbol)) matches.set(symbol, []);
      const ids = matches.get(symbol);
      if (!ids.includes(requirement.id)) ids.push(requirement.id);
    }
  }
  return matches;
}
// A false `changed` flag means the working-tree review observed nothing, which
// is not the same as observing no change. Callers pass the review availability
// so an unavailable review stays an explicit unknown instead of a clean claim.
function requirementChangePill(r, reviewAvailable) {
  if (reviewAvailable !== true) {
    return pill(localized("change state unknown", "变更状态未知"), "warn");
  }
  return r.changed
    ? pill(
        localized(
          `changed +${r.change_additions}/-${r.change_deletions}`,
          `已变更 +${r.change_additions}/-${r.change_deletions}`,
        ),
        "warn",
      )
    : pill(localized("no mapped implementation change", "无映射实现变更"), "good");
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
  const sourcePaths = repositorySourcePaths(), deps = r.dependency_alignment || [],
    blockingDeps = deps.filter((dep) => dep.blocking),
    advisories = deps.filter((dep) =>
      !dep.blocking && dep.status !== "aligned"
    ),
    changes = requirementChanges(r);
  // `changed` is derived from the working-tree review. When that review is
  // unavailable the flag is false because nothing was observed, not because
  // nothing changed, so the requirement must not claim a clean mapping.
  const reviewAvailable = state.project?.git_review?.available === true;
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
  }${requirementChangePill(r, reviewAvailable)}${pill(unit(r.implementation_files, "file", "files", "个文件"))}${
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
    actualComponents(r, sourcePaths)
  }</div><div class="lane-spacer"></div>${
    dependencyRows(r)
  }</div></section><section class="section"><div class="two"><div><h3>${
    esc(t("Acceptance & verification"))
  }</h3>${verificationBlock(r, sourcePaths)}</div><div><h3>${
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
        esc(reviewAvailable
          ? t("No current working-tree file is mapped to this requirement.")
          : localized("Working-tree review is unavailable, so mapped changes are unknown.", "工作树检查不可用，映射的变更状态未知。"))
      }</div>`
  }</section>`;
  setSourceActionHtml("detail", els.detail, html, { tab: "requirements", selection: "requirement", id: r.id }, () => {
    bindCodeReaderActions(els.detail);
    bindChangeOpenActions(els.detail);
  });
}
