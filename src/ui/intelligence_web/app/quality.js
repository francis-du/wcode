function bars(items) {
  if (!items?.length) return `<div class="empty">${esc(t("No data."))}</div>`;
  const max = Math.max(...items.map((item) => Number(item.lines || 0)), 1);
  return `<div class="bars">${
    items.map((item) =>
      `<div class="bar-row"><div class="bar-name">${
        esc(item.name)
      }</div><div class="bar-track"><progress class="bar-progress" max="100" value="${
        Math.max(2, Math.round((item.lines / max) * 100))
      }"></progress></div><div class="bar-val">${
        esc(unit(item.files, "file", "files", "个文件"))
      } · ${esc(unit(item.lines, "line", "lines", "行"))}</div></div>`
    ).join("")
  }</div>`;
}
function renderCodeStats() {
  const c = state.project.code || {},
    html = `<div class="two"><div><h3>${esc(t("Languages"))}</h3>${
      bars(c.languages || [])
    }</div><div><h3>${esc(t("Product Scopes"))}</h3>${
      bars(c.product_scopes || [])
    }</div></div>${
      c.graph_truncated
        ? `<div class="alignment-banner compact-banner"><strong class="warn">${
          esc(t("Bounded snapshot"))
        }</strong><span class="panel-meta">${
          esc(t("bounded graph note"))
        }</span></div>`
        : ""
    }`;
  setHtml("codeStats", els.codeStats, html);
}
function renderRevisions() {
  const p = state.project,
    d = p.latest_delta,
    h = p.history || [],
    risks = p.risk?.risks || [];
  const html = `${
    d
      ? `<div class="delta"><b>${
        esc(t("Latest structural delta"))
      }</b><div class="panel-meta">${time(d.from_captured_at_ms)} → ${
        time(d.to_captured_at_ms)
      }</div><div class="pills">${
        pill(
          `${
            t("nodes")
          } +${d.added_nodes}/-${d.removed_nodes}/~${d.changed_nodes}`,
          "accent",
        )
      }${
        pill(
          `${
            t("edges")
          } +${d.added_edges}/-${d.removed_edges}/~${d.changed_edges}`,
          "accent",
        )
      }</div>${
        d.changed_paths?.length
          ? `<div class="impl-list">${
            d.changed_paths.slice(0, 12).map((path) =>
              `<div class="impl"><div class="impl-path">${
                esc(path)
              }</div></div>`
            ).join("")
          }</div>`
          : ""
      }</div>`
      : `<div class="empty">${
        esc(t("No previous meaningful graph revision yet."))
      }</div>`
  }<div class="revision-list">${
    h.slice(0, 10).map((entry) =>
      `<div class="revision"><div class="revision-mark"></div><div><div class="revision-id">${
        esc(entry.id)
      }</div><div class="revision-meta">${time(entry.captured_at_ms)} · ${
        esc(unit(entry.files_indexed, "file", "files", "个文件"))
      }${
        entry.truncated ? ` · ${esc(t("truncated"))}` : ""
      }</div></div><div class="revision-meta">${num(entry.nodes)} ${
        esc(t("nodes"))
      } / ${num(entry.edges)} ${esc(t("edges"))}</div></div>`
    ).join("")
  }</div>${
    risks.length
      ? `<h3 class="risk-heading">${
        esc(t("Current structured risks"))
      }</h3><div class="risk-list">${
        risks.slice(0, 8).map((risk) =>
          `<div class="risk ${esc(risk.level)}"><b>${
            esc(statusLabel(risk.category))
          } · ${esc(statusLabel(risk.level))}</b><div>${
            esc(risk.summary)
          }</div></div>`
        ).join("")
      }</div>`
      : ""
  }`;
  setHtml("revisions", els.revisions, html);
}
function qualityProviders(language, capability) {
  return (language.providers || []).filter((provider) =>
    provider.capability === capability && provider.declared &&
    provider.available
  );
}
function qualityCell(language, capability) {
  const providers = qualityProviders(language, capability);
  return providers.length
    ? `<span title="${
      esc(providers.map((provider) => provider.id).join(", "))
    }">${pill(t("covered"), "good")}</span>`
    : `<span class="panel-meta">${esc(t("gap"))}</span>`;
}
function renderLanguageQuality() {
  const q = state.project.language_quality || {},
    languages = (q.languages || []).filter((language) =>
      Number(language.detected_files || 0) > 0
    ),
    dimensions = q.dimensions || [],
    gaps = languages.reduce(
      (sum, language) => sum + (language.gaps || []).length,
      0,
    );
  els.qualitySummary.textContent = localized(
    `${languages.length} languages · ${gaps} ${t(gaps === 1 ? "gap" : "gaps")}`,
    `${languages.length} 种语言 · ${gaps} 个缺口`,
  );
  if (!languages.length) {
    setHtml(
      "languageQuality",
      els.languageQuality,
      `<div class="section empty">${
        esc(
          t("No supported source language detected in the bounded repository snapshot."),
        )
      }</div>`,
    );
    return;
  }
  const summary = dimensions.slice(0, 8).map((dimension) =>
    pill(
      `${
        dimensionLabel(dimension.dimension)
      } ${dimension.covered_languages}/${dimension.detected_languages}`,
      dimension.covered_languages === dimension.detected_languages
        ? "good"
        : "info",
    )
  ).join("");
  const html =
    `<div class="section quality-summary"><div class="pills">${summary}</div></div><table class="table"><thead><tr><th>${
      esc(t("Language"))
    }</th><th>${esc(t("Files"))}</th><th>${esc(t("Syntax"))}</th><th>${
      esc(t("Semantic"))
    }</th><th>${esc(t("Format"))}</th><th>${esc(t("Lint"))}</th><th>${
      esc(t("Type"))
    }</th><th>${esc(t("Static"))}</th><th>${esc(t("Test"))}</th><th>${
      esc(t("Security"))
    }</th><th>${esc(t("Advanced"))}</th><th>${
      esc(t("Gaps"))
    }</th></tr></thead><tbody>${
      languages.map((language) =>
        `<tr><td><code>${esc(language.language)}</code></td><td>${
          num(language.detected_files)
        }</td><td>${pill("tree-sitter", "good")}</td><td>${
          language.semantic_available
            ? `<span title="${
              esc(
                language.semantic_provider ||
                  localized("semantic provider", "语义分析器"),
              )
            }">${
              pill(
                statusLabel(language.semantic_runnable ? "ready" : "available"),
                language.semantic_runnable ? "good" : "info",
              )
            }</span>`
            : `<span class="panel-meta">${esc(t("Syntax fallback"))}</span>`
        }</td><td>${qualityCell(language, "format")}</td><td>${
          qualityCell(language, "lint")
        }</td><td>${qualityCell(language, "type_check")}</td><td>${
          qualityCell(language, "static_analysis")
        }</td><td>${qualityCell(language, "test")}</td><td>${
          qualityCell(language, "security")
        }</td><td>${
          (language.advanced_stages || []).length
            ? (language.advanced_stages || []).map((stage) =>
              pill(dimensionLabel(stage), "accent")
            ).join(" ")
            : '<span class="panel-meta">—</span>'
        }</td><td>${
          (language.gaps || []).length
            ? `<span class="info" title="${
              esc(language.gaps.join(" · "))
            }">${language.gaps.length} ${
              esc(t(language.gaps.length === 1 ? "gap" : "gaps"))
            }</span>`
            : `<span class="good">${
              esc(t("declared coverage complete"))
            }</span>`
        }</td></tr>`
      ).join("")
    }</tbody></table>`;
  setHtml("languageQuality", els.languageQuality, html);
}
