"""Build this manuscript using installed Pandoc and XeLaTeX; no downloads."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent


def run(args: list[str], cwd: Path, text: str | None = None) -> str:
    result = subprocess.run(args, cwd=cwd, input=text, text=True, capture_output=True,
                            timeout=90, check=False)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed: {(result.stdout + result.stderr)[-6000:]}")
    return result.stdout


def build() -> None:
    tools = {name: shutil.which(name) for name in ("pandoc", "xelatex")}
    if not all(tools.values()):
        raise RuntimeError(f"Required tools are not installed; no installation attempted: {tools}")
    source = HERE / "paper.en.md"
    header = HERE / "print.tex"
    if source.is_symlink() or header.is_symlink():
        raise ValueError("source/header symlinks are refused")
    text = source.read_text(encoding="utf-8")
    title, edition, body = text.split("\n\n", 2)
    title = title.removeprefix("# ")
    edition = edition.strip("*").replace("·", "|")
    output_root = HERE / ".build"
    if output_root.is_symlink():
        raise ValueError("build-directory symlink is refused")
    output_root.mkdir(exist_ok=True)
    destination = Path(tempfile.mkdtemp(prefix="paper-", dir=output_root))
    tex = run([tools["pandoc"], "--from=markdown+tex_math_dollars", "--to=latex", "--standalone",
               "--wrap=none", "--shift-heading-level-by=-1", "--metadata", f"title={title}", "--metadata", f"date={edition}",
               "--metadata", "lang=en-US", "--variable", "fontsize=11pt", "--variable", "papersize=a4",
               "--variable", "geometry=left=22mm,right=22mm,top=22mm,bottom=23mm,headheight=14pt,headsep=14pt",
               "--include-in-header", str(header)], HERE, body)
    abstract = re.search(r"\\section\{Abstract\}\\label\{abstract\}\s*(.*?)\s*(?=\\textbf\{Keywords:)", tex, re.S)
    if not abstract:
        raise ValueError("Pandoc abstract structure changed; inspect the generated format")
    tex = tex[:abstract.start()] + "\\begin{abstract}\n\\noindent " + abstract.group(1) + "\n\\end{abstract}\n\n" + tex[abstract.end():]
    tex = re.sub(r"\\texttt\{([^{}]*[/][^{}]*)\}",
                 lambda m: "\\nolinkurl{" + m.group(1).replace("\\_", "_") + "}", tex)
    tex = tex.replace("\\textbf{Table 1.", "\\Needspace{15\\baselineskip}\n\\textbf{Table 1.")
    tex = tex.replace("\\textbf{Table 2.", "\\Needspace{15\\baselineskip}\n\\textbf{Table 2.")
    tex = tex.replace("\\section{References}", "\\clearpage\n\\section{References}")
    tex_path = destination / "paper.en.tex"
    tex_path.write_text(tex, encoding="utf-8")
    for _ in range(2):
        run([tools["xelatex"], "-no-shell-escape", "-interaction=nonstopmode", "-halt-on-error", tex_path.name], destination)
    pdf = destination / "paper.en.pdf"
    if not pdf.read_bytes().startswith(b"%PDF-"):
        raise ValueError("missing PDF output")
    log = (destination / "paper.en.log").read_text(encoding="utf-8", errors="replace")
    warnings = [line for line in log.splitlines() if "Overfull" in line or "Missing character" in line]
    manifest = {"source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "tex_sha256": hashlib.sha256(tex_path.read_bytes()).hexdigest(),
                "pdf_sha256": hashlib.sha256(pdf.read_bytes()).hexdigest(),
                "pandoc": run([tools["pandoc"], "--version"], HERE).splitlines()[0],
                "xelatex": run([tools["xelatex"], "--version"], HERE).splitlines()[0],
                "typesetting_warnings": warnings, "visual_check_required": True}
    (destination / "build.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output_directory": str(destination), **manifest}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-env", action="store_true", help="Report dependencies without building")
    options = parser.parse_args()
    if options.check_env:
        print(json.dumps({name: shutil.which(name) for name in ("pandoc", "xelatex")}, indent=2))
    else:
        build()
