// Requirement-scoped changed line ranges for the Engineering Observatory.
// Ranges are bounded on the server (500 files, 256 ranges per file, explicit
// truncation flag) and are never invented here: unavailable stays unavailable.
function changeRangeLabel(range) {
  if (!range || !Number.isInteger(range.start_line) || !Number.isInteger(range.end_line)
    || range.start_line < 1 || range.end_line < range.start_line) return "";
  return range.start_line === range.end_line
    ? `L${range.start_line}`
    : `L${range.start_line}–${range.end_line}`;
}
function changeRangePairs(item) {
  const before = Array.isArray(item.before_changed_ranges) ? item.before_changed_ranges : [];
  const after = Array.isArray(item.after_changed_ranges) ? item.after_changed_ranges : [];
  const pairs = [];
  for (let index = 0; index < Math.max(before.length, after.length); index += 1) {
    const left = changeRangeLabel(before[index]);
    const right = changeRangeLabel(after[index]);
    if (!left && !right) continue;
    const focus = Number.isInteger(after[index]?.start_line) ? after[index].start_line
      : Number.isInteger(before[index]?.start_line) ? before[index].start_line : null;
    pairs.push({ label: `${left || "—"} → ${right || "—"}`, focus });
  }
  return pairs;
}
function changeRangesCell(item) {
  const unavailable = `<span class="change-ranges unavailable">${esc(t("ranges not retained"))}</span>`;
  if (!Array.isArray(item.before_changed_ranges) || !Array.isArray(item.after_changed_ranges)) {
    return unavailable;
  }
  const pairs = changeRangePairs(item);
  if (item.changed_ranges_truncated) {
    const text = pairs.map((pair) => `<span class="change-ranges warn">${esc(pair.label)}</span>`).join(", ");
    return `<span class="change-ranges warn">${esc(t("ranges truncated"))}</span>${text ? ` ${text}` : ""}`;
  }
  if (!pairs.length) {
    return `<span class="change-ranges">${esc(t("no text line ranges"))}</span>`;
  }
  return pairs.map((pair) => pair.focus
    ? `<button type="button" class="change-range-open" data-change-path="${esc(item.path)}" data-change-layer="working" data-change-focus-line="${pair.focus}">${esc(pair.label)}</button>`
    : `<span class="change-ranges">${esc(pair.label)}</span>`
  ).join(", ");
}
function bindChangeOpenActions(host) {
  host?.querySelectorAll("[data-change-path]").forEach((button) => {
    button.addEventListener("click", () => {
      const line = Number(button.dataset.changeFocusLine);
      void openChangeInspection(
        button.dataset.changePath,
        button.dataset.changeLayer || "working",
        false,
        Number.isInteger(line) && line > 0 ? line : null,
      );
    });
  });
}
