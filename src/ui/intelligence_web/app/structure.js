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

function renderTreeContents(node, depth) {
  const directories = [...node.directories.entries()]
    .sort(([left], [right]) => left.localeCompare(right));
  const files = [...node.files]
    .sort((left, right) => left.name.localeCompare(right.name));
  const directoryHtml = directories.map(([name, child]) =>
    `<details class="tree-directory" ${depth < 1 ? "open" : ""}>
      <summary><span class="tree-marker">▸</span><span>${
      esc(name)
    }</span><small>${
      num(
        child.files.length + child.directories.size,
      )
    }</small></summary>
      <div class="tree-children">${renderTreeContents(child, depth + 1)}</div>
    </details>`
  ).join("");
  const fileHtml = files.map((file) =>
    `<div class="tree-file ${file.over_limit ? "over-limit" : ""}">
      <span class="tree-file-name">${esc(file.name)}</span>
      <span class="tree-file-meta">${esc(file.language)} · ${
      num(file.lines)
    }L</span>
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
  return files.map((file, index) =>
    `<div class="large-file ${file.over_limit ? "over-limit" : ""}">
      <span class="large-rank">${index + 1}</span>
      <span class="large-path"><code>${esc(file.path)}</code><small>${
      esc(
        file.language,
      )
    } · ${formatBytes(file.bytes)}</small></span>
      <strong>${num(file.lines)}L</strong>
    </div>`
  ).join("");
}

function renderProjectStructure() {
  const structure = state.project?.structure || {};
  const entries = structure.entries || [];
  const lineLimit = Number(structure.line_limit || 1000);
  const oversized = Number(structure.oversized_files || 0);
  const summary = [
    pill(unit(entries.length, "file", "files", "个文件")),
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
        "warn",
      )
      : pill(t("Within line limit"), "good"),
    structure.truncated ? pill(t("Snapshot truncated"), "warn") : "",
  ].join("");
  setHtml("structureSummary", els.structureSummary, summary);
  setHtml(
    "fileTree",
    els.fileTree,
    entries.length
      ? renderTreeContents(buildFileTree(entries), 0)
      : `<div class="empty">${
        esc(t("No source files in this snapshot."))
      }</div>`,
  );
  setHtml("largeFiles", els.largeFiles, renderLargestFiles(structure));
}
