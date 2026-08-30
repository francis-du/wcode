function graphPrecision() {
  return state.project?.graph_precision ||
    {
      primary: "syntax",
      providers: ["tree-sitter"],
      semantic_edges: 0,
      runtime_edges: 0,
      syntax_edges: 0,
    };
}
function semanticAvailable() {
  return (state.project?.language_quality?.languages || []).some((language) =>
    Number(language.detected_files || 0) > 0 && language.semantic_available
  );
}
function renderLive() {
  const p = state.project;
  if (!p) return;
  const precision = graphPrecision(),
    primary = precision.primary || "syntax",
    providers = (precision.providers || []).filter((provider) =>
      provider !== "design-state"
    ).slice(0, 3),
    primaryLabel = statusLabel(primary),
    providerLabel = localized("provider", "数据源");
  els.projectIdentity.textContent = `${
    p.project || p.product || p.workspace
  } · ${p.root}`;
  els.precisionBadge.className = `precision-badge ${primary}`;
  els.precisionBadge.textContent =
    primary === "semantic" || primary === "runtime"
      ? `${primaryLabel} · ${providers.join(", ") || providerLabel}`
      : `${primaryLabel} ${localized("fallback", "回退")} · ${
        providers.join(", ") || "tree-sitter"
      }`;
  els.lastUpdated.textContent = `${t("last updated")} ${
    new Date(state.lastUpdated).toLocaleTimeString(
      state.language === "zh-CN" ? "zh-CN" : "en",
    )
  }`;
}
function stat(k, v, s, cls = "") {
  return `<div class="stat"><div class="k">${
    esc(k)
  }</div><div class="v ${cls}">${esc(v)}</div><div class="s">${
    esc(s)
  }</div></div>`;
}
function renderStats() {
  const p = state.project,
    c = p.code || {},
    proof = p.proof || {},
    conv = p.convergence || {},
    coverage = p.coverage || {},
    risk = p.risk?.level || "none",
    drift = (p.risk?.drift?.implementation_drift || 0) +
      (p.risk?.drift?.design_drift || 0),
    needs = (conv.needs_convergence_requirements || 0) +
      (conv.incomplete_requirements || 0),
    precision = graphPrecision(),
    complete = coverage.complete_requirements || 0,
    total = coverage.requirements_total || 0;
  const proofState = proof.current_failed
      ? statusLabel("failed")
      : proof.current_disagreed
      ? statusLabel("disagreed")
      : proof.current_evidence
      ? localized(
        `${proof.current_passed}/${proof.current_evidence} pass`,
        `${proof.current_passed}/${proof.current_evidence} 通过`,
      )
      : localized("no current evidence", "暂无当前证据"),
    proofTone = proof.current_failed
      ? "bad"
      : proof.current_disagreed
      ? "warn"
      : proof.current_evidence
      ? "good"
      : "info";
  const actualDetail = localized(
    `${num(c.source_lines)} lines · ${num(c.symbols)} symbols · ${
      num(precision.semantic_edges || 0)
    } semantic / ${num(precision.syntax_edges || 0)} syntax edges`,
    `${num(c.source_lines)} 行 · ${num(c.symbols)} 个符号 · ${
      num(precision.semantic_edges || 0)
    } 条语义边 / ${num(precision.syntax_edges || 0)} 条语法边`,
  );
  setHtml(
    "stats",
    els.stats,
    [
      stat(
        t("Desired State"),
        statusLabel(p.design_valid ? "valid" : "invalid"),
        localized(
          `${complete}/${total} requirements complete`,
          `${complete}/${total} 个需求完整`,
        ),
        p.design_valid ? "good" : "bad",
      ),
      stat(
        t("Actual State"),
        unit(c.source_files, "file", "files", "个文件"),
        actualDetail,
        precision.semantic_edges || precision.runtime_edges ? "good" : "info",
      ),
      stat(
        t("Change"),
        c.changed_files
          ? unit(c.changed_files, "file", "files", "个文件")
          : statusLabel("clean"),
        localized(
          `${
            conv.changing_requirements || 0
          } changing · ${drift} drift · risk ${statusLabel(risk)}`,
          `${
            conv.changing_requirements || 0
          } 个需求变更中 · ${drift} 个漂移 · 风险 ${statusLabel(risk)}`,
        ),
        c.changed_files ? "warn" : "good",
      ),
      stat(
        t("Proof"),
        proofState,
        localized(
          `${proof.current_verification_ready || 0} ready / ${
            proof.current_verification_blocked || 0
          } blocked`,
          `${proof.current_verification_ready || 0} 个就绪 / ${
            proof.current_verification_blocked || 0
          } 个阻塞`,
        ),
        proofTone,
      ),
      stat(
        t("Convergence"),
        needs
          ? localized(`${needs} need work`, `${needs} 个需要处理`)
          : conv.changing_requirements
          ? localized(
            `${conv.changing_requirements} changing`,
            `${conv.changing_requirements} 个变更中`,
          )
          : localized(
            `${conv.stable_requirements || 0} stable`,
            `${conv.stable_requirements || 0} 个稳定`,
          ),
        localized(
          `${conv.stable_requirements || 0} stable · ${
            conv.reconciliation_plans || 0
          } reconciliation plans`,
          `${conv.stable_requirements || 0} 个稳定 · ${
            conv.reconciliation_plans || 0
          } 个收敛计划`,
        ),
        needs || conv.changing_requirements ? "warn" : "good",
      ),
    ].join(""),
  );
}
function attentionItem(tone, title, detail) {
  return `<div class="attention-item ${tone}"><strong>${
    esc(title)
  }</strong><span>${esc(detail)}</span></div>`;
}
function renderAttention() {
  if (!state.project) return;
  const p = state.project,
    proof = p.proof || {},
    conv = p.convergence || {},
    risks = p.risk?.risks || [],
    items = [],
    critical = risks.filter((r) => r.level === "critical"),
    high = risks.filter((r) => r.level === "high"),
    needs = (conv.needs_convergence_requirements || 0) +
      (conv.incomplete_requirements || 0),
    precision = graphPrecision(),
    pending = state.accessLoaded
      ? state.authorizations.length
      : Number(p.pending_authorizations || 0);
  if (!p.design_valid) {
    items.push(
      attentionItem(
        "bad",
        t("Design invalid"),
        t("Design diagnostics require attention"),
      ),
    );
  }
  if (proof.current_failed) {
    items.push(
      attentionItem(
        "bad",
        t("Verification failed"),
        localized(
          `${proof.current_failed} current evidence failure(s)`,
          `${proof.current_failed} 条当前证据失败`,
        ),
      ),
    );
  } else if (proof.current_disagreed) {
    items.push(
      attentionItem(
        "warn",
        t("Verification disagreement"),
        localized(
          `${proof.current_disagreed} disagreement record(s)`,
          `${proof.current_disagreed} 条分歧记录`,
        ),
      ),
    );
  }
  if (critical.length) {
    items.push(
      attentionItem(
        "bad",
        t("Critical risk"),
        localized(
          `${critical.length} current risk(s)`,
          `${critical.length} 个当前风险`,
        ),
      ),
    );
  }
  if (high.length) {
    items.push(
      attentionItem(
        "warn",
        t("High risk"),
        localized(
          `${high.length} current risk(s)`,
          `${high.length} 个当前风险`,
        ),
      ),
    );
  }
  if (needs) {
    items.push(
      attentionItem(
        "warn",
        `${needs} ${t("Requirements need convergence")}`,
        localized(
          `${conv.needs_convergence_requirements || 0} convergence · ${
            conv.incomplete_requirements || 0
          } incomplete`,
          `${conv.needs_convergence_requirements || 0} 个需收敛 · ${
            conv.incomplete_requirements || 0
          } 个不完整`,
        ),
      ),
    );
  }
  if (pending) {
    items.push(
      attentionItem(
        "info",
        `${pending} ${t("Pending approval")}`,
        t("Open Manage access to review exact requests"),
      ),
    );
  }
  if (
    !(precision.semantic_edges || precision.runtime_edges) &&
    semanticAvailable()
  ) {
    items.push(
      attentionItem(
        "info",
        t("Syntax fallback"),
        t("Refresh semantics for stronger dependency evidence"),
      ),
    );
  }
  if (!items.length) {
    items.push(
      attentionItem(
        "good",
        t("No critical attention items"),
        t("Design, proof and convergence have no active blockers"),
      ),
    );
  }
  setHtml("attention", els.attention, items.join(""));
}
