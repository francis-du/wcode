// Fitness observes retained tool outcomes, not model success or verification readiness.
function fitnessNumber(value, digits = 0) {
  return typeof value === "number" && Number.isFinite(value)
    ? value.toLocaleString(state.language, { maximumFractionDigits: digits }) : "—";
}
function fitnessRate(numerator, denominator) {
  return Number.isSafeInteger(numerator) && Number.isSafeInteger(denominator)
    && denominator > 0 && numerator >= 0 && numerator <= denominator
    ? `${fitnessNumber(100 * numerator / denominator, 1)}% (${fitnessNumber(numerator)}/${fitnessNumber(denominator)})`
    : localized("Not measured", "未测量");
}
function fitnessMetricValid(row) {
  const counts = [row?.succeeded, row?.partial, row?.blocked, row?.failed];
  return Number.isSafeInteger(row?.samples) && row.samples > 0 && row.samples <= 64
    && counts.every(value => Number.isSafeInteger(value) && value >= 0)
    && counts.reduce((sum, value) => sum + value, 0) === row.samples;
}
function fitnessTrend(row, suppress) {
  const trend = row.trend;
  if (suppress || !trend || !Number.isSafeInteger(trend.earlier_samples)
    || !Number.isSafeInteger(trend.recent_samples) || trend.earlier_samples < 3
    || trend.recent_samples < 3 || trend.earlier_samples + trend.recent_samples !== row.samples
    || !Number.isFinite(trend.success_rate_delta_pp) || Math.abs(trend.success_rate_delta_pp) > 100
    || !Number.isFinite(trend.p50_delta_ms)) return localized("Insufficient comparable samples", "可比样本不足");
  const signed = value => `${value > 0 ? "+" : ""}${fitnessNumber(value, 1)}`;
  return `${signed(trend.success_rate_delta_pp)} pp · ${signed(trend.p50_delta_ms)} ms · ${trend.earlier_samples} → ${trend.recent_samples}`;
}
function renderFitnessBenchmark() {
  if (!els.fitnessBenchmark) return;
  const project = state.project || {}, benchmark = project.fitness?.benchmark;
  const empty = message => setHtml("fitnessBenchmark", els.fitnessBenchmark,
    `<div class="fitness-boundaries"><strong>${esc(localized("Check history benchmark", "工程 Fitness 基准测量"))}</strong><p>${esc(message)}</p></div>`);
  if (!benchmark || benchmark.available !== true || (state.current && project.workspace !== state.current)) {
    return empty(localized("Benchmark summaries are unavailable. No quality score has been inferred.", "基准摘要不可用，未推断任何质量分数。"));
  }
  const report = benchmark.latest;
  if (!report) return empty(benchmark.status === "not_measured"
    ? localized("No diagnostic or trial report is available. This page never runs a benchmark automatically.", "暂无诊断或试验报告。此页面不会自动运行基准测试。")
    : localized("No valid benchmark summary could be read; incomplete or invalid reports are not success.", "未能读取有效基准摘要；不完整或无效报告不代表成功。"));
  const proof = project.proof || {}, before = report.revision_before || {}, after = report.revision_after || {};
  const same = typeof proof.revision_code === "string" && after.code === proof.revision_code
    && (after.design ?? null) === (proof.revision_design ?? null)
    && before.code === after.code && (before.design ?? null) === (after.design ?? null);
  const current = same && report.source_stable_during_run === true
    && report.source_snapshot_before === report.source_snapshot_after
    && benchmark.status === "current" && !benchmark.partial && !state.syncError && !state.fitnessSnapshotFromCache
    && project.snapshot_cache !== "stale-while-revalidate";
  const stateLabel = current ? localized("Current source observation", "当前源码观测")
    : localized("Historical, partial or unconfirmed observation", "历史、部分或尚未确认的观测");
  const ratio = value => typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1
    ? `${fitnessNumber(value * 100, 2)}%` : localized("Not measured", "未测量");
  const ms = value => typeof value === "number" && Number.isFinite(value) && value >= 0 ? fitnessNumber(value / 1000, 2) : "—";
  const rows = Array.isArray(report.rows) ? report.rows.slice(0, 6) : [];
  const cells = rows.map(row => `<tr><th scope="row">${fitnessNumber(row.budget)} / ${esc(row.phase === "cold" ? localized("cold", "冷") : row.phase === "warm" ? localized("warm", "暖") : "—")}</th><td>${esc(fitnessRate(row.required_hits, row.required_count))}</td><td>${esc(fitnessRate(row.complete_body_hits, row.required_count))}</td><td>${esc(fitnessRate(row.fresh_sha_hits, row.required_count))}</td><td>${esc(fitnessRate(row.edit_input_ready, row.edit_input_eligible))}</td><td>${fitnessNumber(row.mean_ndcg_at_10, 3)}<small>n=${fitnessNumber(row.ranking_attempts)}</small></td><td>${esc(ratio(row.mean_non_gold_fraction))}<small>n=${fitnessNumber(row.noise_samples)}</small></td><td>${ms(row.p50_us)} / ${ms(row.p95_us)}</td><td>${fitnessNumber(row.query_errors)} / ${fitnessNumber(row.attempts)}<small>${esc(localized("Over budget", "超预算"))}: ${fitnessNumber(row.over_budget)} · ${esc(localized("Warmup errors", "预热错误"))}: ${fitnessNumber(row.warmup_errors)}</small></td></tr>`).join("");
  const labels = [localized("Budget / cache", "预算 / 缓存"), localized("Required recall", "必要目标召回"), localized("Full body", "完整源码"), localized("Fresh SHA", "有效 SHA"), localized("Edit inputs", "编辑输入"), "NDCG@10", localized("Non-Gold fraction", "非 Gold 比例"), "p50 / p95 (ms)", localized("Errors / attempts", "错误 / 尝试")];
  const history = Array.isArray(benchmark.history) ? benchmark.history.slice(0, 32).map(entry => `<div class="revision"><span class="revision-mark"></span><div><code>${esc(entry.revision?.code || "—")}</code><small>${esc(entry.revision?.design || "—")}</small><small>${esc(entry.comparable_to_latest ? localized("Matching recorded configuration; hardware not verified", "已记录的配置匹配，未验证硬件一致性") : localized("Different measurement configuration", "测量配置不同"))}</small><code>${esc(entry.artifact_sha256 || "—")}</code></div><div class="revision-meta">${typeof entry.captured_at_ms === "number" ? time(entry.captured_at_ms) : "—"}</div></div>`).join("") : "";
  const html = `<div class="fitness-summary"><div class="pills">${pill(localized("Explicit synthetic benchmark", "显式合成基准测试"), "info")}${pill(stateLabel, current ? "info" : "warn")}${pill(report.profile || "—", "info")}</div><p>${fitnessNumber(report.case_count)} ${esc(localized("cases", "个用例"))} · ${fitnessNumber(report.samples_per_case)} ${esc(localized("samples per case/budget/cache phase", "份样本 / 用例 / 预算 / 缓存阶段"))} · ${esc(report.os)} / ${esc(report.arch)} · ${esc(report.wcode_version || "—")} · ${typeof report.captured_at_ms === "number" && report.captured_at_ms > 0 ? time(report.captured_at_ms) : "—"} · ${esc(localized("Model calls", "模型调用"))}: ${fitnessNumber(report.model_calls)} · ${esc(localized("Controls passed", "控制实验通过"))}: ${fitnessNumber(report.controls_passed)} / ${fitnessNumber(report.controls_total)}</p><p class="panel-meta">${esc(localized("Report data is separate from live tool outcomes and release verification. A source-stable run and binary fingerprint do not prove that the binary was compiled from the currently observed source.", "报告数据独立于实时工具结果与发布验证。源码在运行期间稳定及二进制指纹，不能证明二进制由当前观测源码编译。"))}</p><code class="fitness-revision">${esc(after.code || "—")}</code><small>${esc(localized("Design", "设计"))}: ${esc(after.design || "—")}</small></div><div class="table-wrap fitness-table-wrap fitness-benchmark-table" role="region" aria-label="${esc(localized("Benchmark measurements", "基准测量数据"))}" tabindex="0"><table class="table"><caption>${esc(localized("Selected valid benchmark report · budget is serialized bytes / 4, not model token usage", "选中的有效基准报告 · 预算为序列化字节 / 4，不是模型实际 token 用量"))}</caption><thead><tr>${labels.map(label => `<th scope="col">${esc(label)}</th>`).join("")}</tr></thead><tbody>${cells}</tbody></table></div><p class="fitness-note">${esc(localized("Synthetic development corpus, not an external holdout. Cold means a new Harness, not a cleared OS cache; warm follows an identical-query warmup. Latency aggregates heterogeneous tasks and is descriptive, not an SLO. Recall and edit-input coverage are not model understanding or correct generated patches.", "使用合成开发语料，而非外部留出集。冷态指新 Harness，不会清空系统缓存；暖态经过同一查询预热。延迟汇总不同任务，仅作描述，不是服务指标。召回率与编辑输入覆盖率不代表模型理解力或生成补丁正确率。"))}</p><details class="fitness-history"><summary>${esc(localized("Benchmark history and provenance", "基准历史与来源"))} · ${fitnessNumber(benchmark.report_count)}</summary><p class="panel-meta">${esc(localized("Invalid / duplicate reports", "无效 / 重复报告"))}: ${fitnessNumber(benchmark.invalid_reports)} / ${fitnessNumber(benchmark.duplicate_reports)}</p><small>${esc(localized("Corpus / evaluator / binary", "语料 / 评估器 / 二进制"))}</small><code>${esc(report.corpus_sha256)}</code><code>${esc(report.evaluator_sha256)}</code><code>${esc(report.test_binary_sha256)}</code>${history}</details>`;
  setHtml("fitnessBenchmark", els.fitnessBenchmark, html);
}
function renderFitnessObservatory() {
  renderFitnessBenchmark();
  if (!els.fitnessObservatory) return;
  const project = state.project || {}, fitness = project.fitness;
  const unavailable = text => setHtml("fitnessObservatory", els.fitnessObservatory,
    `<div class="empty"><strong>${esc(localized("Fitness observations unavailable", "Fitness 观测不可用"))}</strong><p>${esc(text)}</p></div>`);
  if (!fitness || fitness.available !== true || fitness.schema_version !== 1
    || (state.current && project.workspace !== state.current)) {
    return unavailable(localized("No compatible journal snapshot was received. This is not a healthy or idle signal.", "没有收到兼容的日志快照；这不表示健康或空闲。"));
  }
  const proof = project.proof || {}, revision = fitness.revision || {};
  const sameRevision = typeof proof.revision_code === "string" && revision.code === proof.revision_code
    && (revision.design ?? null) === (proof.revision_design ?? null);
  const stale = state.syncError === true || state.fitnessSnapshotFromCache === true || project.snapshot_cache === "stale-while-revalidate" || !sameRevision;
  const partial = fitness.partial === true, rows = Array.isArray(fitness.current) ? fitness.current.slice(0, 64) : [];
  const validRows = sameRevision ? rows.filter(fitnessMetricValid) : [];
  const title = stale ? localized("Cached or mismatched snapshot", "缓存或版本不匹配的快照")
    : partial ? localized("Partial retained window", "部分保留窗口")
      : fitness.window_limited ? localized("Latest 64 retained milestones", "最近 64 条保留里程碑") : localized("Observed sample", "已观测样本");
  const formatDate = value => typeof value === "number" && value > 0 ? time(value) : "—";
  const note = localized("Only the last 64 retained milestones, not all tool calls. Legacy events without a revision are excluded. A tool returning successfully is not proof of a correct patch, a passed test or a healthy application.", "仅统计最近 64 条保留里程碑，并非全部工具调用。无版本的旧事件不参与当前统计。工具返回成功不代表补丁正确、测试通过或应用健康。");
  const cells = validRows.map(row => `<tr><th scope="row"><code>${esc(row.tool)}</code>${row.verification_level ? `<small>${esc(statusLabel(row.verification_level))}</small>` : ""}</th><td>${esc(fitnessRate(row.succeeded, row.samples))}</td><td>${fitnessNumber(row.partial)} / ${fitnessNumber(row.blocked)} / ${fitnessNumber(row.failed)}</td><td>${fitnessNumber(row.p50_ms, 1)} / ${fitnessNumber(row.p95_ms, 1)}</td><td>${esc(fitnessTrend(row, stale || partial))}</td></tr>`).join("");
  const ledger = Array.isArray(fitness.history) ? fitness.history.slice(0, 16).map(entry => {
    const current = !stale && entry.current === true && entry.revision?.code === revision.code
      && (entry.revision?.design ?? null) === (revision.design ?? null);
    return `<div class="revision"><span class="revision-mark"></span><div><code>${esc(entry.revision?.code || "—")}</code><small>${esc(localized("Design", "设计"))}: ${esc(entry.revision?.design || "—")}</small><small>${esc(current ? localized("Current observed revision", "当前观测版本") : localized("Historical observation", "历史观测"))}</small></div><div class="revision-meta">${fitnessNumber(entry.samples)} ${esc(localized("samples", "条样本"))}<br>${formatDate(entry.last_at_ms)}</div></div>`;
  }).join("") : "";
  const invalidRows = rows.length - validRows.length;
  const table = cells ? `<div class="table-wrap fitness-table-wrap" role="region" aria-label="${esc(localized("Recorded tool outcomes", "记录的工具结果"))}" tabindex="0"><table class="table"><caption>${esc(localized("Current observed revision · descriptive tool outcomes", "当前观测版本 · 描述性工具结果"))}</caption><thead><tr><th scope="col">${esc(localized("Tool / level", "工具 / 级别"))}</th><th scope="col">${esc(localized("Successful returns / recorded outcomes", "成功返回 / 已记录结果"))}</th><th scope="col">${esc(localized("Partial / blocked / failed", "部分 / 阻止 / 失败"))}</th><th scope="col">${esc(localized("p50 / p95 (ms)", "p50 / p95（毫秒）"))}</th><th scope="col">${esc(localized("Earlier → recent half", "前半 → 后半样本"))}</th></tr></thead><tbody>${cells}</tbody></table></div>`
    : `<div class="empty">${esc(localized("No usable observations for this code and design revision. No rate has been inferred.", "当前代码与设计版本暂无可用观测，未推断成功率。"))}</div>`;
  const html = `<div class="fitness-summary"><div class="pills">${pill(title, stale || partial ? "warn" : "info")}${pill(localized("Not a release gate", "不是发布门禁"), "info")}</div><p>${esc(note)}</p><div class="fitness-counts"><span>${esc(localized("Sampled", "取样"))}: ${fitnessNumber(fitness.sampled_records)} / ${fitnessNumber(fitness.retained_records)} ${esc(localized("retained", "已保留"))}</span><span>${esc(localized("Unbound", "未绑定版本"))}: ${fitnessNumber(fitness.unbound_records)}</span><span>${esc(localized("Other revisions", "其他版本"))}: ${fitnessNumber(fitness.stale_records)}</span><span>${esc(localized("Duplicate / conflicting", "重复 / 冲突"))}: ${fitnessNumber(fitness.duplicate_records)} / ${fitnessNumber(fitness.conflicting_events)}</span></div><p class="panel-meta">${formatDate(fitness.window_start_ms)} → ${formatDate(fitness.window_end_ms)} · ${esc(localized("Snapshot captured", "快照生成"))}: ${formatDate(fitness.observed_at_ms)}</p><code class="fitness-revision">${esc(revision.code || "—")}</code><small>${esc(localized("Design", "设计"))}: ${esc(revision.design || "—")}</small></div>${invalidRows ? `<p class="warn">${esc(localized("Invalid or mismatched rows were excluded.", "已排除无效或版本不匹配的数据行。"))}</p>` : ""}${table}<p class="panel-meta fitness-note">${esc(localized("Revision binding is a best-effort post-operation observation, not proof of stable execution inputs. Trends compare halves of the same tool, level and revision with at least three observations each; they do not establish a regression or statistical significance. Small-sample p95 can be the maximum.", "版本绑定是操作后的尽力观测，不是执行输入稳定性的证明。趋势只比较相同工具、级别和版本内前后各至少三条观测，不证明回归或统计显著性。小样本 p95 可能就是最大值。"))}</p><details class="fitness-history"><summary>${esc(localized("Retained revision history", "保留的版本历史"))}${fitness.history_truncated ? ` · ${esc(localized("partial", "部分"))}` : ""}</summary>${ledger || `<div class="empty">${esc(localized("No revision-bound history yet.", "暂无绑定版本的历史。"))}</div>`}</details><div class="fitness-boundaries"><strong>${esc(localized("Separate measurements", "独立测量口径"))}</strong><p>${esc(localized("Benchmark recall, precision, NDCG and cold/warm costs come from explicit Check history reports, not this journal. Flakiness, retry/recovery, live LSP accuracy and application readiness are not measured here. Use the Proof workspace for revision-bound verification; Worklist completion is not a performance score.", "召回率、精度、NDCG 与冷暖成本应来自显式 Check history 报告，而非本日志。这里尚未测量不稳定率、重试恢复率、真实 LSP 准确率和应用就绪率。版本验证请查看证据工作区；工作项完成数不是性能分数。"))}</p></div>`;
  setHtml("fitnessObservatory", els.fitnessObservatory, html);
}

function bars(items) {
  if (!items?.length) return `<div class="empty">${esc(t("No data."))}</div>`;
  const max = Math.max(...items.map((item) => Number(item.lines || 0)), 1);
  return `<div class="bars">${
    items.map((item) =>
      `<div class="bar-row"><div class="bar-name">${
        esc(item.name)
      }</div><div class="bar-track"><progress class="bar-progress" max="100" value="${
        Math.max(0, Math.min(100, Math.round((Number(item.lines) || 0) / max * 100)))
      }"></progress></div><div class="bar-val">${
        esc(unit(item.files, "file", "files", "个文件"))
      } · ${esc(unit(item.lines, "line", "lines", "行"))}</div></div>`
    ).join("")
  }</div>`;
}
function renderCodeStats() {
  const c = state.project.code || {},
    html = `<div class="code-distribution"><div><h3>${esc(t("Languages"))}</h3>${
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
function qualityMatches(language, capability) {
  return (language.providers || []).filter((provider) =>
    provider.capability === capability || (provider.covers || []).includes(capability)
  );
}
function qualityProviders(language, capability) {
  return qualityMatches(language, capability).filter((provider) =>
    provider.declared && provider.available && provider.runnable && provider.check_only
  );
}
function qualityCell(language, capability) {
  const providers = qualityProviders(language, capability);
  if (providers.length) {
    const external = providers.some((provider) => provider.external_advisory_data);
    return `<span title="${
      esc(providers.map((provider) => provider.id).join(", "))
    }">${pill(t("covered"), "good")}${
      external ? ` ${pill(localized("advisory data", "外部数据"), "info")}` : ""
    }</span>`;
  }
  const declared = qualityMatches(language, capability).filter((provider) => provider.declared);
  if (declared.some((provider) => provider.check_only && !provider.available)) {
    return `<span class="panel-meta">${esc(localized("tool missing", "工具缺失"))}</span>`;
  }
  if (declared.some((provider) => provider.check_only && provider.available && !provider.runnable)) {
    return `<span class="panel-meta">${esc(localized("not runnable", "不可运行"))}</span>`;
  }
  if (declared.some((provider) => !provider.check_only)) {
    return `<span class="panel-meta">${esc(localized("discovery only", "仅发现"))}</span>`;
  }
  return `<span class="panel-meta">${esc(t("gap"))}</span>`;
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
                  localized("LSP server", "LSP Server"),
              )
            }">${
              pill(
                statusLabel(language.semantic_runnable ? "ready" : "available"),
                language.semantic_runnable ? "good" : "info",
              )
            }</span>`
            : `<span class="panel-meta">${esc(t("Tree-sitter only"))}</span>`
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
