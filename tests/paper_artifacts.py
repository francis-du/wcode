"""Read-only checks of paper data, tables, and links. No model or benchmark calls."""
from pathlib import Path
import hashlib
import json
import math
import re
import shutil
import sys

ROOT = Path(__file__).resolve().parents[1]
PAPER = ROOT / "docs" / "paper"
EXPECTED = "b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def load(path: Path) -> dict:
    require(not path.is_symlink(), f"symlink refused: {path.name}")
    require(path.stat().st_size < 2_000_000, "oversized artifact")
    return json.loads(path.read_text(encoding="utf-8"))


def equivalent(left, right) -> bool:
    if isinstance(left, (int, float)) and not isinstance(left, bool):
        return isinstance(right, (int, float)) and math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-12)
    return left == right


def groups_from_raw(report: dict) -> dict:
    groups = {}
    for item in report["summary"]:
        group = dict(item)
        group["fresh_sha_hits_derived"] = group["required_gold_count"] - group["missing_current_sha_count"]
        groups[(group["budget"], group["phase"])] = group
    return groups


def groups_from_selected(selected: dict) -> dict:
    groups = {}
    for item in selected["summary_selected_fields"]:
        group = dict(item)
        group["latency_across_tasks"] = {
            "p50_us": item["latency_p50_us"],
            "p95_us": item["latency_p95_us"],
        }
        groups[(item["budget"], item["phase"])] = group
    return groups


def verify_raw_report(raw_path: Path, selected: dict) -> dict:
    require(not raw_path.is_symlink(), "symlink raw report refused")
    data = raw_path.read_bytes()
    require(len(data) == selected["source_report_bytes"], "raw size mismatch")
    require(hashlib.sha256(data).hexdigest() == EXPECTED == selected["source_report_sha256"], "raw SHA mismatch")
    report = load(raw_path)
    require(all(report["metadata"].get(k) == v for k, v in selected["metadata"].items()), "metadata mismatch")
    require(len(report["rows"]) == 360, "expected 360 cells")
    require(len({(r["case_id"], r["budget"], r["phase"]) for r in report["rows"]}) == 360, "duplicate cells")
    require(all(len(r["samples"]) == 1 for r in report["rows"]), "not single-sample protocol")
    require(sum(r["warmup_us"] is not None for r in report["rows"]) == 180, "warmup count")
    groups = {(g["budget"], g["phase"]): g for g in report["summary"]}
    require(len(groups) == 6, "expected six groups")
    for item in selected["summary_selected_fields"]:
        group = groups[(item["budget"], item["phase"])]
        for key, expected in item.items():
            if key == "fresh_sha_hits_derived":
                actual = group["required_gold_count"] - group["missing_current_sha_count"]
            elif key.startswith("latency_"):
                actual = group["latency_across_tasks"][key.removeprefix("latency_")]
            else:
                actual = group[key]
            require(equivalent(actual, expected), f"selected field mismatch: {key}")
        require(all(group[k] == v for k, v in selected["denominators_per_group"].items()), "denominator mismatch")
        rows = [r for r in report["rows"] if (r["budget"], r["phase"]) == (item["budget"], item["phase"])]
        samples = [r["samples"][0] for r in rows]
        require(sum(r["required_count"] for r in rows) == 98, "Gold denominator")
        require(sum(bool(r["writable"] and r["required_count"]) for r in rows) == 58, "eligibility")
        require(sum(s["error"] is not None for s in samples) == group["errors"], "error count")
        for field in ["required_hits", "complete_body_hits"]:
            total = sum((s["score"] or {}).get(field, 0) for s in samples)
            require(total == group[field], f"raw-sample aggregation: {field}")
        edits = sum(bool((s["score"] or {}).get("all_required_edit_inputs")) for s in samples)
        require(edits == group["all_required_edit_inputs_count"], "edit aggregation")
    require(len(report["controls"]) == 9 and all(c["passed"] for c in report["controls"]), "historical controls")
    return report


def verify_manuscript(groups: dict) -> int:
    manuscript = PAPER / "paper.en.md"
    require(manuscript.is_file(), "English manuscript missing")
    text = manuscript.read_text(encoding="utf-8")
    require("## 9 Conclusion" in text and "## References" in text, "incomplete manuscript")
    require(all(f"[S{i}]" in text for i in range(1, 9)), "missing source definitions")
    require(all(f"[{i}]" in text for i in range(1, 8)), "missing references")
    tables = []
    current = []
    for line in text.splitlines() + [""]:
        if line.startswith("|"):
            current.append([v.strip() for v in line.strip("|").split("|")])
        elif current:
            tables.append(current)
            current = []
    quality = next(t for t in tables if "Required identities" in t[0])
    costs = next(t for t in tables if "Mean response bytes" in t[0])
    for cells in quality[2:]:
        group = groups[(int(cells[0][0]) * 1000, cells[1])]
        expected = [f"{group['required_hits']}/98", f"{group['complete_body_hits']}/98", f"{group['all_required_edit_inputs_count']}/58"]
        require(all(cells[k + 2].startswith(v + " ") for k, v in enumerate(expected)), "quality table mismatch")
        require(int(cells[5]) == group["errors"], "quality errors mismatch")
    for cells in costs[2:]:
        group = groups[(int(cells[0][0]) * 1000, cells[1])]
        require(cells[2] == f"{group['mean_response_bytes']:,.2f}", "byte table mismatch")
        for column, key in [(3, "p50_us"), (4, "p95_us")]:
            require(cells[column] == f"{group['latency_across_tasks'][key] / 1000:.3f}", "latency table mismatch")
        require(int(cells[5]) == group["over_budget"], "budget table mismatch")
    return 2


def verify_links() -> None:
    nav_files = [ROOT / "docs/manual/paper.md", ROOT / "docs/manual/paper.zh-CN.md"]
    require(all(p.is_file() for p in nav_files), "bilingual paper landing page missing")
    for path in nav_files:
        text = path.read_text(encoding="utf-8")
        for target in re.findall(r"\]\((/paper/[^)#]+)\)", text):
            require((ROOT / "docs" / target.lstrip("/")).is_file(), f"broken paper download: {target}")
    english, chinese = [p.read_text(encoding="utf-8") for p in nav_files]
    require("alternate: /zh/docs/paper/" in english and "alternate: /docs/paper/" in chinese, "alternate routes")
    require(english.count("\n## ") == chinese.count("\n## "), "bilingual section parity")
    for name in ["README.md", "README.zh-CN.md"]:
        require("(paper/)" in (ROOT / "docs/manual" / name).read_text(encoding="utf-8"), "missing index link")


def main() -> None:
    selected = load(PAPER / "snapshot.json")
    archived = (PAPER / "fitness.raw.json").is_file()
    raw_path = PAPER / "fitness.raw.json" if archived else ROOT / selected["source_report"]
    raw_verified = raw_path.is_file()
    report = verify_raw_report(raw_path, selected) if raw_verified else None
    if not raw_verified:
        print(
            f"warning: raw report unavailable at {raw_path}; "
            "raw-sample and control verification skipped, manuscript checked against selected fields only",
            file=sys.stderr,
        )
    groups = groups_from_raw(report) if raw_verified else groups_from_selected(selected)
    checked_tables = verify_manuscript(groups)
    verify_links()
    if raw_verified:
        example = next(r for r in report["rows"] if r["case_id"] == "rust-refresh-natural" and r["budget"] == 1000 and r["phase"] == "cold")
        score = example["samples"][0]["score"]
        require(score["required_hits"] == 1 and score["fresh_sha_hits"] == 1 and score["complete_body_hits"] == 0, "example evidence mismatch")
        require(score["delivery"]["required_source_bytes"] == 93, "example byte count")
    sizes = {p.name: p.stat().st_size for p in PAPER.iterdir() if p.is_file()}
    print(json.dumps({
        "status": "passed" if raw_verified else "raw_report_unavailable",
        "scope": "historical paper artifact consistency, not a new runtime evaluation",
        "raw_report_present": raw_verified,
        "raw_verified": raw_verified,
        "raw_sha256": EXPECTED,
        "raw_archived": archived,
        "measurement_cells": 360 if raw_verified else None,
        "warmups": 180 if raw_verified else None,
        "groups_verified": 6,
        "manuscript_tables_verified": checked_tables,
        "recorded_controls_passed": 9 if raw_verified else None,
        "artifact_bytes": sizes,
        "build_tools": {name: shutil.which(name) for name in ["pandoc", "xelatex", "pdflatex"]},
    }, indent=2))


if __name__ == "__main__":
    main()
