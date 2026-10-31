// Local inspection of retained, redacted output; no request or command is constructed here.
function runtimeJobOutputScope() {
  const owner = runtimeJobsScope(), job = state.runtimeJob;
  const task = job?.task_id === state.runtimeJobSelected ? job.task_id : "";
  const previous = state.runtimeJobOutput;
  if (!previous || previous.owner !== owner || previous.task !== task) {
    state.runtimeJobOutput = { owner, task, query: "", stream: "all", page: 0, composing: false, caseSensitive: true, rendered: false, filterKey: "" };
  }
  return state.runtimeJobOutput;
}
function setRuntimeJobOutput(query, stream, page = 0, view = runtimeJobOutputScope()) {
  if (runtimeJobOutputScope() !== view || typeof query !== "string" || query.length > 256
    || /[\r\n\0]/.test(query) || !["all", "stdout", "stderr"].includes(stream)
    || !Number.isSafeInteger(page) || page < 0) return false;
  const changed = view.query !== query || view.stream !== stream;
  view.query = query; view.stream = stream; view.page = changed ? 0 : page;
  renderRuntimeJobs(); return true;
}
function runtimeJobOutputView() {
  const view = runtimeJobOutputScope(), job = state.runtimeJob;
  const available = !!view.task && validRuntimeJob(job, state.current, view.task);
  const names = view.stream === "all" ? ["stdout", "stderr"] : [view.stream];
  const rows = []; let totalLines = 0, truncated = false, redacted = false;
  if (available) for (const name of names) {
    const stream = job[name], lines = stream.text === "" ? [] : stream.text.split(/\r\n|\r|\n/);
    totalLines += lines.length; truncated ||= stream.truncated; redacted ||= stream.redacted;
    if (view.query) lines.forEach((text, index) => {
      if (view.caseSensitive ? text.includes(view.query) : text.toLowerCase().includes(view.query.toLowerCase())) rows.push({ stream: name, line: index + 1, text });
    });
  }
  const pages = Math.max(1, Math.ceil(rows.length / 25));
  view.page = Math.min(view.page, pages - 1);
  return { available, query: view.query, stream: view.stream, page: view.page, pages,
    total_lines: totalLines, total_matches: rows.length, rows: rows.slice(view.page * 25, view.page * 25 + 25),
    limited: truncated || redacted, truncated, redacted };
}
function runtimeJobOutputCoverage(view) {
  return '<p class="panel-meta" data-job-output-coverage>'
    + esc(localized("Search covers retained output only. Line numbers are relative to each retained fragment; streams have no shared time order.",
      "仅搜索已保留输出。行号相对于各自保留片段；两个输出流没有共同时间顺序。"))
    + (view.truncated ? " " + esc(localized("Output is truncated; omitted text is not searched.", "输出已截断，省略的内容不参与搜索。")) : "")
    + (view.redacted ? " " + esc(localized("Output is redacted; hidden text is not searched.", "输出已脱敏，隐藏的内容不参与搜索。")) : "") + "</p>";
}
function runtimeJobOutputHtml(view, job) {
  if (!view.available) return "";
  const names = view.stream === "all" ? ["stdout", "stderr"] : [view.stream];
  const streams = names.map(name => {
    const output = job[name], matches = view.rows.filter(row => row.stream === name);
    const contents = view.query ? matches.map(row => '<span data-job-output-match>' + row.line + ': ' + esc(row.text) + '</span>').join("\n") : esc(output.text);
    return '<details open data-job-output-raw="' + name + '" data-job-output-stream-name="' + name + '"><summary>' + name + " · " + num(output.total_bytes)
      + " " + esc(localized("reported bytes", "报告字节"))
      + (output.truncated ? " · " + esc(localized("truncated", "已截断")) : "")
      + (output.redacted ? " · " + esc(localized("redacted", "已脱敏")) : "") + "</summary>"
      + (contents ? '<pre data-job-output-text tabindex="0" role="region" aria-label="' + esc(localized(name + " retained output", name + " 保留输出")) + '">' + contents + "</pre>"
        : '<p class="panel-meta">' + esc(view.query ? localized("No matching lines on this page for this retained stream.", "此输出流在当前页没有匹配行。") : localized("No text retained for this stream.", "此输出流没有保留文本。")) + "</p>") + "</details>";
  }).join("");
  const summary = view.query ? localized(view.total_matches + " matching lines in the selected retained streams; showing " + view.rows.length + ".", "选中输出流的保留窗口内有 " + view.total_matches + " 行匹配，当前显示 " + view.rows.length + " 行。")
    : localized("Showing the selected retained output. Search is local and does not retrieve older logs.", "显示选中输出流的保留内容。搜索在本地完成，不会读取更早的日志。");
  const warnings = (job.stdout.truncated || job.stderr.truncated ? '<p class="warn">' + esc(localized("Output was truncated. No match here does not rule out a match in omitted output.", "输出已截断。此处无匹配不代表被省略的输出中没有匹配。")) + "</p>" : "")
    + (job.stdout.redacted || job.stderr.redacted ? '<p class="panel-meta">' + esc(localized("Output was redacted before display and search.", "输出在显示与搜索前已经脱敏。")) + "</p>" : "");
  const pages = view.pages > 1 ? '<div class="runtime-job-output-pages"><button type="button" class="quiet-action" data-job-output-previous data-job-output-page="' + (view.page - 1) + '"' + (!view.page ? " disabled" : "") + ">"
    + esc(localized("Previous", "上一页")) + "</button><span>" + esc(localized("Page ", "第 ")) + (view.page + 1) + " / " + view.pages
    + '</span><button type="button" class="quiet-action" data-job-output-next data-job-output-page="' + (view.page + 1) + '"' + (view.page + 1 >= view.pages ? " disabled" : "") + ">" + esc(localized("Next", "下一页")) + "</button></div>" : "";
  return '<p class="panel-meta" role="status" data-job-output-note>' + esc(summary) + "</p>" + runtimeJobOutputCoverage(view) + warnings
    + (view.query && !view.total_matches ? '<p data-job-output-empty>' + esc(localized("No matches in the selected retained output.", "所选保留输出中没有匹配。")) + "</p>" : "") + streams + pages;
}
function renderRuntimeJobOutput(job, scope) {
  const filters = document.getElementById("runtimeJobOutputFilters"), host = document.getElementById("runtimeJobOutput");
  const view = runtimeJobOutputScope(), id = state.runtimeJobSelected, projected = runtimeJobOutputView();
  const current = () => runtimeJobsScope() === scope && runtimeJobOutputScope() === view
    && state.runtimeJobSelected === id && state.runtimeJob?.task_id === id && state.workspaceTab === "activity";
  if (host?._runtimeJobOutputView !== view) {
    invalidate("runtimeJobOutputFilters"); invalidate("runtimeJobOutput");
    if (host) host._runtimeJobOutputView = view;
  }
  if (!projected.available) {
    setHtml("runtimeJobOutputFilters", filters, ""); setHtml("runtimeJobOutput", host, "");
    view.rendered = false; return;
  }
  // This subtree stays stable during polling and composition; task/scope replacement invalidates it.
  setHtml("runtimeJobOutputFilters", filters, '<label>' + esc(localized("Search retained output", "搜索保留输出"))
    + '<input type="search" data-job-output-query maxlength="256" autocomplete="off" spellcheck="false" aria-describedby="runtimeJobOutputHint"></label><label>'
    + esc(localized("Output stream", "输出流")) + '<select data-job-output-stream><option value="all">stdout + stderr</option><option value="stdout">stdout</option><option value="stderr">stderr</option></select></label>'
    + '<label class="runtime-output-case"><input type="checkbox" data-job-output-case checked>' + esc(localized("Match case", "区分大小写")) + '</label>'
    + '<div class="runtime-output-actions"><button type="button" class="quiet-action" data-job-output-clear>' + esc(localized("Clear search", "清空搜索")) + '</button><button type="button" class="quiet-action" data-job-output-end>' + esc(localized("Jump to end", "跳到末尾")) + '</button></div>'
    + '<p id="runtimeJobOutputHint" class="panel-meta">' + esc(localized("Literal text, up to 256 characters. Matching lines are paged, 25 at a time.", "按字面匹配，最多 256 个字符。匹配行分页显示，每页 25 行。")) + "</p>", () => {
      const query = filters?.querySelector("[data-job-output-query]"), stream = filters?.querySelector("[data-job-output-stream]");
      const commit = () => { if (current()) setRuntimeJobOutput(query.value.slice(0, 256), view.stream, 0, view); };
      query?.addEventListener("compositionstart", () => { if (current()) view.composing = true; });
      query?.addEventListener("compositionend", () => { if (current()) { view.composing = false; commit(); } });
      query?.addEventListener("input", event => { if (!event.isComposing && !view.composing) commit(); });
      stream?.addEventListener("change", () => { if (current()) setRuntimeJobOutput(view.query, stream.value, 0, view); });
      filters?.querySelector("[data-job-output-case]")?.addEventListener("change", event => {
        if (!current()) return; view.caseSensitive = event.target.checked; view.page = 0; renderRuntimeJobs();
      });
      filters?.querySelector("[data-job-output-clear]")?.addEventListener("click", () => {
        if (!current() || view.composing) return; setRuntimeJobOutput("", view.stream, 0, view); query?.focus();
      });
      filters?.querySelector("[data-job-output-end]")?.addEventListener("click", () => {
        if (!current()) return;
        host?.querySelectorAll("[data-job-output-text]").forEach(output => { output.scrollTop = output.scrollHeight; });
      });
    });
  const query = filters?.querySelector("[data-job-output-query]"), stream = filters?.querySelector("[data-job-output-stream]");
  if (query && !view.composing && query.value !== view.query) query.value = view.query;
  if (stream) stream.value = view.stream;
  const caseControl = filters?.querySelector("[data-job-output-case]"); if (caseControl) caseControl.checked = view.caseSensitive;
  const filterKey = JSON.stringify([view.query, view.stream, view.page, view.caseSensitive]), positions = new Map();
  if (view.rendered && view.filterKey === filterKey) host?.querySelectorAll("[data-job-output-stream-name]").forEach(details => {
    const output = details.querySelector("[data-job-output-text]");
    positions.set(details.dataset.jobOutputStreamName, { open: details.open, top: output?.scrollTop || 0, left: output?.scrollLeft || 0, focused: !!output && document.activeElement === output });
  });
  const active = document.activeElement;
  const focusedPage = host?.querySelector("[data-job-output-next]") === active ? "next" : host?.querySelector("[data-job-output-previous]") === active ? "previous" : null;
  setHtml("runtimeJobOutput", host, runtimeJobOutputHtml(projected, job), () => {
    host?.querySelectorAll("[data-job-output-page]").forEach(button => button.addEventListener("click", () => {
      if (current()) setRuntimeJobOutput(view.query, view.stream, Number(button.dataset.jobOutputPage), view);
    }));
  });
  host?.querySelectorAll("[data-job-output-stream-name]").forEach(details => {
    const previous = positions.get(details.dataset.jobOutputStreamName), output = details.querySelector("[data-job-output-text]");
    if (!previous) return;
    details.open = previous.open;
    if (output) { output.scrollTop = previous.top; output.scrollLeft = previous.left; if (previous.focused) output.focus?.({ preventScroll: true }); }
  });
  if (focusedPage) {
    let restored = host?.querySelector("[data-job-output-" + focusedPage + "]");
    if (!restored || restored.disabled) restored = host?.querySelector("[data-job-output-" + (focusedPage === "next" ? "previous" : "next") + "]");
    if (!restored || restored.disabled) restored = query;
    restored?.focus({ preventScroll: true });
  }
  view.filterKey = filterKey; view.rendered = true;
}
