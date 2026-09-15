function formatBytes(value) {
  const bytes = Number(value || 0);
  if (bytes < 1024) return `${num(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function buildFileTree(entries) {
  const root = { directories: new Map(), files: [] };
  for (const entry of entries) {
    const parts = String(entry.path || "").split("/")
      .filter((part) => part && part !== ".");
    if (!parts.length) continue;
    const fileName = parts.pop();
    let node = root;
    for (const directory of parts) {
      if (!node.directories.has(directory)) {
        node.directories.set(directory, { directories: new Map(), files: [] });
      }
      node = node.directories.get(directory);
    }
    node.files.push({ ...entry, name: fileName });
  }
  return root;
}

function renderTreeContents(node, depth, expand = false) {
  const directories = [...node.directories.entries()]
    .sort(([left], [right]) => left.localeCompare(right));
  const files = [...node.files]
    .sort((left, right) => left.name.localeCompare(right.name));
  const directoryHtml = directories.map(([name, child]) =>
    `<details class="tree-directory" ${depth < 1 || expand ? "open" : ""}>
      <summary><span class="tree-marker" aria-hidden="true"></span><span>${
      esc(name)
    }</span><small>${
      num(
        child.files.length + child.directories.size,
      )
    }</small></summary>
      <div class="tree-children">${renderTreeContents(child, depth + 1, expand)}</div>
    </details>`
  ).join("");
  const fileHtml = files.map((file) =>
    `<div class="tree-file ${file.over_limit ? "over-limit" : ""}" title="${esc(file.path)}">
      <span class="tree-file-name">${esc(file.name)}</span>
      <span class="tree-file-meta">${esc(file.language)} · ${
      num(file.lines)
    }L${file.generated ? ` · ${esc(localized("generated", "生成文件"))}` : ""}</span>
    </div>`
  ).join("");
  return directoryHtml + fileHtml;
}

function renderLargestFiles(structure) {
  const files = structure.largest_files || [];
  if (!files.length) {
    return `<div class="empty">${
      esc(t("No source files in this snapshot."))
    }</div>`;
  }
  const lineLimit = Number(structure.line_limit || 1000);
  return files.map((file, index) => {
    const policyLabel = file.over_limit
      ? localized("over line limit", "超过行数限制")
      : file.generated
        ? Number(file.lines || 0) > lineLimit
          ? localized("generated · line limit exempt", "生成文件 · 不受行数限制")
          : localized("generated", "生成文件")
        : "";
    return `<div class="large-file ${file.over_limit ? "over-limit" : ""}">
      <span class="large-rank">${index + 1}</span>
      <span class="large-path"><code title="${esc(file.path)}">${esc(file.path)}</code><small>${
      esc(
        file.language,
      )
    } · ${formatBytes(file.bytes)}${policyLabel ? ` · ${esc(policyLabel)}` : ""}</small></span>
      <strong>${num(file.lines)}L</strong>
    </div>`;
  }).join("");
}

function renderProjectStructure() {
  const structure = state.project?.structure || {};
  const entries = structure.entries || [];
  const query = String(els.fileSearch?.value || "").trim().toLowerCase();
  const visibleEntries = query
    ? entries.filter(file => String(file.path || "").toLowerCase().includes(query))
    : entries;
  if (els.fileSearchStatus) {
    els.fileSearchStatus.textContent = query
      ? localized(`${num(visibleEntries.length)} matching files`, `${num(visibleEntries.length)} 个匹配文件`)
      : "";
  }
  const lineLimit = Number(structure.line_limit || 1000);
  const oversized = Number(structure.oversized_files || 0);
  const summary = [
    pill(
      structure.truncated
        ? localized(
          `${num(entries.length)} files shown`,
          `展示 ${num(entries.length)} 个文件`,
        )
        : unit(entries.length, "file", "files", "个文件"),
    ),
    pill(
      unit(
        structure.directory_count || 0,
        "directory",
        "directories",
        "个目录",
      ),
    ),
    pill(
      localized(
        `depth ${num(structure.max_depth || 0)}`,
        `深度 ${num(structure.max_depth || 0)}`,
      ),
    ),
    oversized
      ? pill(
        localized(
          `${num(oversized)} over ${num(lineLimit)} lines`,
          `${num(oversized)} 个超过 ${num(lineLimit)} 行`,
        ),
        "bad",
      )
      : structure.truncated
        ? pill(
          localized("No oversized files in snapshot", "当前快照未发现超长文件"),
          "warn",
        )
        : entries.length ? pill(t("Within line limit"), "good") : "",
    structure.truncated ? pill(t("Snapshot truncated"), "warn") : "",
  ].join("");
  setHtml("structureSummary", els.structureSummary, summary);
  setHtml(
    "fileTree",
    els.fileTree,
    visibleEntries.length
      ? renderTreeContents(buildFileTree(visibleEntries), 0, Boolean(query))
      : `<div class="empty">${
        esc(t(query ? "No matching files." : "No source files in this snapshot."))
      }</div>`,
  );
  setHtml("largeFiles", els.largeFiles, renderLargestFiles(structure));
}
