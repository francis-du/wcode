#!/usr/bin/env python3
"""Build the English preprint and/or its Chinese companion with standard TeX tools."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], log: Path) -> None:
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    log.write_text(result.stdout + "\n" + result.stderr, encoding="utf-8")
    if result.returncode:
        print(result.stdout[-6000:] + result.stderr[-2000:], file=sys.stderr)
        raise SystemExit(f"Build failed; see {log}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--language", choices=["en", "zh", "all"], default="all")
    parser.add_argument("--output-dir", default="build")
    args = parser.parse_args()
    out = (ROOT / args.output_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)
    targets = [("en", "main", "pdflatex"), ("zh", "main.zh-CN", "xelatex")]
    results = []
    for language, stem, engine in targets:
        if args.language not in ("all", language):
            continue
        for program in (engine, "bibtex"):
            if not shutil.which(program):
                raise SystemExit(f"Required TeX executable is missing: {program}")
        source_paths = [ROOT / (stem + ".tex"), ROOT / "paper-style.sty", ROOT / "refs.bib"]
        source_paths += sorted((ROOT / ("sections" if language == "en" else "zh")).glob("*.tex"))
        source_paths += [ROOT / "figures/budget_exposure.pdf", ROOT / "figures/latency_ecdf.pdf"]
        inputs = {str(path.relative_to(ROOT)): digest(path) for path in source_paths}
        command = [engine, "-interaction=nonstopmode", "-halt-on-error",
                   "-file-line-error", f"-output-directory={out}", stem + ".tex"]
        run(command, out / (stem + ".pass1.txt"))
        run(["bibtex", str((out / stem).relative_to(ROOT))], out / (stem + ".bibtex.txt"))
        run(command, out / (stem + ".pass2.txt"))
        run(command, out / (stem + ".pass3.txt"))
        after = {str(path.relative_to(ROOT)): digest(path) for path in source_paths}
        if after != inputs:
            raise SystemExit("A manuscript input changed during compilation; rebuild after editing ends.")
        log = (out / (stem + ".log")).read_text(errors="replace")
        errors = [line for line in log.splitlines() if any(x in line for x in (
            "There were undefined references", "Citation", "undefined on input line",
            "Missing character:", "Overfull \\hbox", "Overfull \\vbox"))]
        pdf = out / (stem + ".pdf")
        results.append({"language": language, "engine": engine,
                        "source": stem + ".tex", "pdf": str(pdf.relative_to(ROOT)),
                        "bytes": pdf.stat().st_size, "sha256": digest(pdf),
                        "inputs_sha256": inputs,
                        "layout_or_reference_warnings": errors})
        (out / (stem + "-build.json")).write_text(json.dumps(results[-1], indent=2) + "\n")
        print(json.dumps(results[-1], ensure_ascii=False))
    (out / ("build-" + args.language + ".json")).write_text(
        json.dumps({"outputs": results}, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
