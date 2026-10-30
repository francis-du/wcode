// Inspect only the already bounded, redacted stream returned for an exact job.
// Search and scrolling are local view operations and never make a request.
const runtimeJobOutputStreams = ["all", "stdout", "stderr"];
const runtimeJobOutputMatchLimit = 200;
function runtimeJobOutputView(scope, id) {
  let view = state.runtimeJobOutputView;
  if (!view || view.scope !== scope || view.id !== id) {
    view = { scope, id, query: "", stream: "all", filterKey: "", rendered: false };
    state.runtimeJobOutputView = view;
    invalidate("runtimeJobOutputFilters"); invalidate("runtimeJobOutput");
  }
  return view;
}
function runtimeJobOutputProjection(job, query = "", stream = "all") {
  const needle = query.toLowerCase();
  return ["stdout", "stderr"].filter(name => stream === "all" || name === stream).map(name => {
    const output = job[name], lines = output.text ? output.text.split("\n") : [];
    if (lines.at(-1) === "") lines.pop();
    const matches = needle ? lines.flatMap((line, index) => line.toLowerCase().includes(needle) ? [{ line, number: index + 1 }] : []) : [];
    return { name, output, lines: lines.length, matches: matches.length,
      omitted: needle ? Math.max(0, matches.length - runtimeJobOutputMatchLimit) : 0,
      text: needle ? matches.slice(0, runtimeJobOutputMatchLimit).map(row => `${row.number}: ${row.line}`).join("\n") : output.text };
  });
}
function renderRuntimeJobOutput(job, scope) {
  const filters = document.getElementById("runtimeJobOutputFilters"), host = document.getElementById("runtimeJobOutput");
  const id = state.runtimeJobSelected, view = runtimeJobOutputView(scope, id);
  const current = () => runtimeJobsScope() === scope && state.runtimeJobOutputView === view
    && state.runtimeJobSelected === id && state.runtimeJob?.task_id === id && state.workspaceTab === "activity";
  if (!job || job.task_id !== id || job.workspace !== scope.workspace) {
    setHtml("runtimeJobOutputFilters", filters, "");
    setHtml("runtimeJobOutput", host, "");
    view.rendered = false;
    return;
  }
  // This controls subtree has stable markup while typing and polling, keeping
  // keyboard focus and the search caret in the existing input element.
  setHtml("runtimeJobOutputFilters", filters, `<label>${esc(localized("Search retained output", "搜索保留输出"))}<input type="search" data-job-output-query maxlength="256" autocomplete="off" spellcheck="false" placeholder="${esc(localized("Literal text, ignoring case", "文本匹配，不区分大小写"))}"></label>
    <label>${esc(localized("Output stream", "输出流"))}<select data-job-output-stream><option value="all">${esc(localized("stdout + stderr", "stdout + stderr"))}</option><option value="stdout">stdout</option><option value="stderr">stderr</option></select></label>
    <div class="runtime-output-actions"><button type="button" class="quiet-action" data-job-output-clear>${esc(localized("Clear search", "清空搜索"))}</button><button type="button" class="quiet-action" data-job-output-end>${esc(localized("Jump to end", "跳到末尾"))}</button></div>`, () => {
    const query = filters?.querySelector("[data-job-output-query]"), stream = filters?.querySelector("[data-job-output-stream]");
    query?.addEventListener("input", () => {
      if (!current()) return;
      view.query = query.value.slice(0, 256); renderRuntimeJobOutput(state.runtimeJob, scope);
    });
    stream?.addEventListener("change", () => {
      if (!current() || !runtimeJobOutputStreams.includes(stream.value)) return;
      view.stream = stream.value; renderRuntimeJobOutput(state.runtimeJob, scope);
    });
    filters?.querySelector("[data-job-output-clear]")?.addEventListener("click", () => {
      if (!current()) return;
      view.query = ""; renderRuntimeJobOutput(state.runtimeJob, scope); query?.focus();
    });
    filters?.querySelector("[data-job-output-end]")?.addEventListener("click", () => {
      if (!current()) return;
      host?.querySelectorAll("[data-job-output-text]").forEach(output => { output.scrollTop = output.scrollHeight; });
    });
  });
  const query = filters?.querySelector("[data-job-output-query]"), stream = filters?.querySelector("[data-job-output-stream]");
  if (query && query.value !== view.query) query.value = view.query;
  if (stream) stream.value = view.stream;
  const filterKey = JSON.stringify([view.query, view.stream]), positions = new Map();
  if (view.rendered && view.filterKey === filterKey) {
    host?.querySelectorAll("[data-job-output-stream-name]").forEach(details => {
      const output = details.querySelector("[data-job-output-text]");
      positions.set(details.dataset.jobOutputStreamName, { open: details.open, top: output?.scrollTop || 0, left: output?.scrollLeft || 0,
        focused: !!output && document.activeElement === output });
    });
  }
  const rows = runtimeJobOutputProjection(job, view.query, view.stream);
  const result = view.query ? localized(
    `${rows.reduce((total, row) => total + row.matches, 0)} matching lines in the selected retained streams. Line numbers are relative to the retained window.`,
    `选中输出流的保留窗口内有 ${rows.reduce((total, row) => total + row.matches, 0)} 行匹配。行号相对于保留窗口。`)
    : localized("Showing the selected retained output. Search is local and does not retrieve older logs.", "显示选中输出流的保留内容。搜索在本地完成，不会读取更早的日志。");
  const warnings = (job.stdout.truncated || job.stderr.truncated ? `<p class="warn">${esc(localized("Output was truncated. No match here does not rule out a match in omitted output.", "输出已截断。此处无匹配不代表被省略的输出中没有匹配。"))}</p>` : "")
    + (job.stdout.redacted || job.stderr.redacted ? `<p class="panel-meta">${esc(localized("Output was redacted before display and search.", "输出在显示与搜索前已经脱敏。"))}</p>` : "");
  const streams = rows.map(row => `<details open data-job-output-stream-name="${row.name}"><summary>${row.name} · ${num(row.output.total_bytes)} ${esc(localized("reported bytes", "报告字节"))}${row.output.truncated ? " · " + esc(localized("truncated", "已截断")) : ""}${row.output.redacted ? " · " + esc(localized("redacted", "已脱敏")) : ""}</summary>${row.text ? `<pre data-job-output-text tabindex="0" role="region" aria-label="${esc(localized(`${row.name} retained output`, `${row.name} 保留输出`))}">${esc(row.text)}</pre>` : `<p class="panel-meta">${esc(view.query ? localized("No matching lines in this retained stream.", "此输出流的保留内容中没有匹配行。") : localized("No text retained for this stream.", "此输出流没有保留文本。"))}</p>`}${row.omitted ? `<p class="warn">${esc(localized(`Showing the first ${runtimeJobOutputMatchLimit} matching lines; ${row.omitted} additional matching lines are not displayed. Refine the search.`, `显示前 ${runtimeJobOutputMatchLimit} 行匹配；另有 ${row.omitted} 行匹配未显示，请缩小搜索范围。`))}</p>` : ""}</details>`).join("");
  setHtml("runtimeJobOutput", host, `<p class="panel-meta" role="status">${esc(result)}</p>${warnings}${streams}`);
  host?.querySelectorAll("[data-job-output-stream-name]").forEach(details => {
    const previous = positions.get(details.dataset.jobOutputStreamName), output = details.querySelector("[data-job-output-text]");
    if (!previous) return;
    details.open = previous.open;
    if (output) {
      output.scrollTop = previous.top; output.scrollLeft = previous.left;
      if (previous.focused) output.focus?.({ preventScroll: true });
    }
  });
  view.filterKey = filterKey; view.rendered = true;
}
