// Bounded historical source inspection for one exact recorded revision.
// The revision list comes from the backend and the path comes from the change
// inspector; neither a revision expression nor a free-form path is accepted.
const HISTORICAL_SOURCE_PAGE = 240;
const HISTORICAL_REVISION_LIMIT = 32;
const HISTORICAL_REVISION_PATTERN = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;

function historicalRevisionId(value) {
  const revision = typeof value === "string" ? value.trim().toLowerCase() : "";
  return HISTORICAL_REVISION_PATTERN.test(revision) ? revision : "";
}
function historicalRevisionLabel(row) {
  return `${row.short_revision} · ${row.committed_at} · ${row.subject}`;
}
function historicalState(current) {
  if (!current) return null;
  if (!current.historical) {
    current.historical = {
      revisions: null, revisionsLoading: false, revisionsError: "", revisionsController: null,
      revision: "", source: null, sourceLoading: false, sourceError: "", controller: null,
    };
  }
  return current.historical;
}
function validHistoricalRevisionList(payload) {
  if (!payload || typeof payload !== "object" || !Array.isArray(payload.revisions)
    || payload.revisions.length > HISTORICAL_REVISION_LIMIT
    || typeof payload.truncated !== "boolean") return false;
  const seen = new Set();
  return payload.revisions.every(row => {
    const revision = historicalRevisionId(row?.revision);
    if (!revision || typeof row?.short_revision !== "string" || !row.short_revision
      || row.short_revision.length > 12
      || typeof row?.committed_at !== "string" || !row.committed_at || row.committed_at.length > 16
      || typeof row?.subject !== "string" || !row.subject || row.subject.length > 240
      || /[\x00-\x1f\x7f]/.test(row.subject) || seen.has(revision)) return false;
    seen.add(revision);
    return true;
  });
}
function validHistoricalSourceResponse(data, expected) {
  const source = data?.source;
  const empty = source?.total_lines === 0 && source?.start_line === 1 && source?.end_line === 0;
  if (data?.workspace !== expected.workspace
    || source?.path !== expected.path
    || historicalRevisionId(source?.revision) !== expected.revision
    || !["source", "binary", "redacted", "directory", "truncated"].includes(source?.kind)
    || !Number.isInteger(source?.start_line) || source.start_line < 1
    || !Number.isInteger(source?.end_line)
    || !Number.isInteger(source?.total_lines) || source.total_lines < 0
    || (source.kind === "source" && !empty && source.end_line < source.start_line)
    || (source.kind === "source" && source.total_lines < source.end_line)
    || typeof source?.content !== "string" || source.content.length > 262144
    || typeof source?.redacted !== "boolean" || typeof source?.truncated !== "boolean") return false;
  if (source.kind !== "source") {
    return source.content === "" && source.sha256 === ""
      && source.redacted === (source.kind === "redacted")
      && source.truncated === (source.kind === "truncated")
      && source.start_line === 1 && source.end_line === 0 && source.total_lines === 0;
  }
  if (!/^[0-9a-f]{64}$/i.test(source.sha256 || "")) return false;
  return empty || source.content.split("\n").length === source.end_line - source.start_line + 1;
}
function historicalSourceFailure(error) {
  const status = Number.isInteger(error?.status) ? error.status : null;
  switch (error?.code) {
    case "unknown_revision":
      return localized("This revision is no longer available in this repository. Reload the revision list.", "该版本在当前仓库中已不可用，请重新读取版本列表。");
    case "source_missing":
      return localized("This path does not exist at the selected revision.", "所选版本中不存在该路径。");
    case "path_restricted":
      return localized("This path is restricted by the workspace protected-path policy.", "该路径受工作区受保护路径策略限制。");
    case "invalid_revision":
    case "invalid_path":
      return localized("The historical source request is invalid. Reload the file and try again.", "历史源码请求无效，请重新读取文件后重试。");
    default:
      break;
  }
  if (status === 404) {
    return localized("This revision or path is not available. Reload the revision list.", "该版本或路径不可用，请重新读取版本列表。");
  }
  return requestFailureMessage(error);
}
async function loadHistoricalRevisions(current) {
  const path = current?.path;
  if (!path) return false;
  const historical = historicalState(current);
  historical.revisionsController?.abort();
  const controller = new AbortController();
  historical.revisionsController = controller;
  historical.revisionsLoading = true;
  historical.revisionsError = "";
  renderChangeInspector();
  const workspace = state.current, epoch = state.workspaceEpoch;
  const active = () => state.changeInspection === current && state.current === workspace
    && state.workspaceEpoch === epoch && historical.revisionsController === controller
    && !controller.signal.aborted;
  try {
    const data = await uiJson(`/intelligence/revisions?limit=${HISTORICAL_REVISION_LIMIT}`,
      "GET", undefined, {workspace, signal: controller.signal, timeout: 20000});
    if (!active()) {
      if (historical.revisionsController === controller) historical.revisionsLoading = false;
      return false;
    }
    if (data.workspace !== workspace || !validHistoricalRevisionList(data)) {
      historical.revisionsLoading = false;
      historical.revisionsError = localized("Invalid revision list response.", "版本列表响应无效。");
      renderChangeInspector();
      return false;
    }
    historical.revisions = data.revisions;
    historical.revisionsLoading = false;
    renderChangeInspector();
    return true;
  } catch (error) {
    if (!active()) {
      if (historical.revisionsController === controller) historical.revisionsLoading = false;
      return false;
    }
    historical.revisionsLoading = false;
    historical.revisionsError = error.status === 422
      ? localized("This repository has no readable recorded revisions yet.", "当前仓库还没有可读取的已记录版本。")
      : requestFailureMessage(error);
    renderChangeInspector();
    return false;
  }
}
async function openHistoricalSource(current, revision, startLine) {
  const path = current?.path;
  const requested = historicalRevisionId(revision);
  const historical = historicalState(current);
  if (!path || !requested || !Array.isArray(historical.revisions)
    || !historical.revisions.some(row => historicalRevisionId(row.revision) === requested)) return false;
  const start = Number.isSafeInteger(startLine) && startLine >= 1 ? startLine : 1;
  historical.controller?.abort();
  const controller = new AbortController();
  historical.controller = controller;
  historical.revision = requested;
  historical.source = null;
  historical.sourceLoading = true;
  historical.sourceError = "";
  renderChangeInspector();
  const workspace = state.current, epoch = state.workspaceEpoch;
  const active = () => state.changeInspection === current && state.current === workspace
    && state.workspaceEpoch === epoch && historical.controller === controller
    && !controller.signal.aborted;
  const params = new URLSearchParams({path, revision: requested, start_line: String(start)});
  try {
    const data = await uiJson(`/intelligence/historical-source?${params}`, "GET", undefined,
      {workspace, signal: controller.signal, timeout: 30000});
    if (!active()) {
      if (historical.controller === controller) historical.sourceLoading = false;
      return false;
    }
    if (data.workspace !== workspace
      || !validHistoricalSourceResponse(data, {workspace, path, revision: requested})) {
      historical.sourceLoading = false;
      historical.sourceError = localized("Invalid historical source response.", "历史源码响应无效。");
      renderChangeInspector();
      return false;
    }
    historical.source = data.source;
    historical.sourceLoading = false;
    renderChangeInspector();
    return true;
  } catch (error) {
    if (!active()) {
      if (historical.controller === controller) historical.sourceLoading = false;
      return false;
    }
    historical.source = null;
    historical.sourceLoading = false;
    historical.sourceError = historicalSourceFailure(error);
    renderChangeInspector();
    return false;
  }
}
function historicalSourcePreview(source) {
  const identity = `<p class="panel-meta"><code>${esc(source.revision.slice(0, 12))}</code> · ${esc(source.path)} · sha256 <code>${esc(source.sha256.slice(0, 16))}</code></p>`;
  if (source.kind !== "source") {
    const message = {
      binary: localized("Binary content at this revision; no text preview is available.", "该版本为二进制内容，无法提供文本预览。"),
      redacted: localized("The bounded Git output for this revision was redacted, so no source identity is published.", "该版本的有界 Git 输出已被脱敏，因此不发布源码身份。"),
      directory: localized("This path is a directory at the selected revision.", "所选版本中该路径是目录。"),
      truncated: localized("This file exceeds the bounded historical read window; no partial identity is published.", "该文件超出有界历史读取窗口，不发布部分身份。"),
    }[source.kind] || localized("Historical source is unavailable at this revision.", "该版本的历史源码不可用。");
    return `${identity}<p class="warn">${esc(message)}</p>`;
  }
  const expectedRows = Math.max(0, source.end_line - source.start_line + 1);
  const rows = source.content.split("\n").slice(0, expectedRows).map((line, index) =>
    `<div class="historical-source-line"><span>${source.start_line + index}</span><code>${esc(line)}</code></div>`).join("");
  const flags = [
    source.redacted ? pill(localized("redacted", "已脱敏"), "warn") : "",
    source.truncated ? pill(localized("bounded", "有界"), "warn") : "",
  ].join("");
  const paging = source.total_lines === 0 ? "" : `<nav class="historical-source-paging"><button type="button" data-historical-page="${Math.max(1, source.start_line - HISTORICAL_SOURCE_PAGE)}" ${source.start_line <= 1 ? "disabled" : ""}>${esc(localized("Previous", "上一页"))}</button><span>${num(source.start_line)}–${num(source.end_line)} / ${num(source.total_lines)} ${esc(localized("lines", "行"))}</span><button type="button" data-historical-page="${source.end_line + 1}" ${source.end_line >= source.total_lines ? "disabled" : ""}>${esc(localized("Next", "下一页"))}</button></nav>`;
  return `${identity}<div class="pills">${flags}</div>${paging}<div class="historical-source-window" role="region" tabindex="0" aria-label="${esc(localized("Historical source code", "历史源代码"))}">${source.total_lines === 0 ? `<div class="empty">${esc(localized("This source file is empty at this revision.", "该版本下此源码文件为空。"))}</div>` : rows}</div><p class="panel-meta">${esc(localized("Historical source is bound to the exact commit shown above, not to the current change snapshot.", "历史源码绑定到上方显示的精确提交，而不是当前变更快照。"))}</p>`;
}
function historicalSourcePanel(current) {
  if (!current?.path) return "";
  const historical = historicalState(current);
  const title = `<h4>${esc(localized("Historical source", "历史源码"))}</h4>`;
  if (historical.revisionsLoading) {
    return `<section class="historical-source">${title}<p role="status">${esc(localized("Loading recorded revisions…", "正在读取已记录的版本…"))}</p></section>`;
  }
  if (historical.revisionsError) {
    return `<section class="historical-source">${title}<p class="bad" role="alert">${esc(historical.revisionsError)}</p><button type="button" data-historical-revisions>${esc(localized("Reload revision list", "重新读取版本列表"))}</button></section>`;
  }
  if (!historical.revisions) {
    return `<section class="historical-source">${title}<p>${esc(localized("Read this file as of a recorded commit. The revision list comes from this repository; no revision expression is accepted.", "按已记录的提交读取此文件。版本列表来自当前仓库，不接受版本表达式。"))}</p><button type="button" data-historical-revisions>${esc(localized("Browse file history", "浏览文件历史"))}</button></section>`;
  }
  if (!historical.revisions.length) {
    return `<section class="historical-source">${title}<p>${esc(localized("No recorded revision is available for this repository.", "当前仓库没有可读取的已记录版本。"))}</p></section>`;
  }
  const options = historical.revisions.map(row => {
    const revision = historicalRevisionId(row.revision);
    return `<option value="${esc(revision)}"${revision === historical.revision ? " selected" : ""}>${esc(historicalRevisionLabel(row))}</option>`;
  }).join("");
  const toolbar = `<div class="historical-source-toolbar"><label class="historical-source-select">${esc(localized("Recorded revision", "已记录版本"))}<select data-historical-revision>${options}</select></label><button type="button" data-historical-open>${esc(localized("Read at this revision", "按此版本读取"))}</button><button type="button" data-historical-close>${esc(localized("Close history", "关闭历史"))}</button></div>`;
  if (historical.sourceLoading) {
    return `<section class="historical-source">${title}${toolbar}<p role="status">${esc(localized("Reading bounded historical source…", "正在读取有界历史源码…"))}</p></section>`;
  }
  if (historical.sourceError) {
    return `<section class="historical-source">${title}${toolbar}<p class="bad" role="alert">${esc(historical.sourceError)}</p></section>`;
  }
  if (!historical.source) {
    return `<section class="historical-source">${title}${toolbar}<p>${esc(localized("Pick a recorded revision to read the exact bytes stored at that commit.", "选择一个已记录版本，读取该提交保存的精确字节。"))}</p></section>`;
  }
  return `<section class="historical-source">${title}${toolbar}${historicalSourcePreview(historical.source)}</section>`;
}
function bindHistoricalSourcePanel(current) {
  const host = q("#changeInspector");
  if (!host) return;
  host.querySelector("[data-historical-revisions]")?.addEventListener("click", () => {
    void loadHistoricalRevisions(current);
  });
  host.querySelector("[data-historical-close]")?.addEventListener("click", () => {
    const historical = historicalState(current);
    historical.controller?.abort();
    historical.revisionsController?.abort();
    current.historical = null;
    renderChangeInspector();
  });
  host.querySelector("[data-historical-open]")?.addEventListener("click", () => {
    const select = host.querySelector("[data-historical-revision]");
    const revision = historicalRevisionId(select?.value);
    if (revision) void openHistoricalSource(current, revision, 1);
  });
  host.querySelectorAll("[data-historical-page]").forEach(button => {
    button.addEventListener("click", () => {
      if (button.disabled) return;
      const historical = historicalState(current);
      void openHistoricalSource(current, historical.revision, Number(button.dataset.historicalPage));
    });
  });
}
