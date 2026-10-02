#!/usr/bin/env python3
"""Verify the real OSS source workspace without a commercial checkout.

Python 3.10+, Cargo, Git and the repository's test runtimes are required.
The aggregate bundle contains every declared OSS workspace package; extracted
leaf crates are also validated as real Cargo packages. Every Cargo stage is
offline. Child output is bounded and never echoed with credentials or source.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import signal
import stat
import subprocess
import tarfile
import tempfile
import threading
import time

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None  # Python 3.10 uses the actual Cargo metadata contract.

MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_TAR_BYTES = 160 * 1024 * 1024
MAX_CONTENT_BYTES = 128 * 1024 * 1024
MAX_FILE_BYTES = 16 * 1024 * 1024
MAX_MEMBERS = 8192
MAX_OUTPUT_BYTES = 8 * 1024 * 1024
MAX_METADATA_BYTES = 1024 * 1024
MAX_REPORT_BYTES = 64 * 1024
MAX_REPORT_EVENTS = 64
MAX_FAILURE_TESTS = 256
MAX_FAILURE_LINE_BYTES = 1024
# Only fixed native status codes and known test files may leave captured output.
# These are diagnostic hints, never evidence that a verification passed.
NATIVE_FAILURE_CODES = frozenset({
    "revision_incomplete", "discovery_incomplete", "mapping_incomplete",
    "git_capture_incomplete", "git_candidate_changed", "git_commit_binding_incomplete",
    "policy_binding_missing", "policy_inactive", "policy_revoked", "policy_expired",
    "policy_definition_changed", "policy_unavailable", "policy_workspace_mismatch",
    "policy_generation_changed", "policy_native_matrix_missing", "plan_requirements_incomplete",
    "required_check_unavailable", "required_check_unmapped", "required_check_timed_out",
    "required_check_execution_unavailable", "required_check_execution_unknown",
    "native_check_failed", "native_check_inconclusive", "required_check_skipped",
    "check_evidence_stale", "required_check_not_executed", "native_aggregate_coverage_missing",
    "native_stage_failed", "native_stage_inconclusive", "native_stage_binding_missing",
})
NATIVE_FAILURE_FILES = (
    "tests/unit/integrations/git/native.rs", "tests/unit/runtime/harness/acceptance.rs",
    "tests/unit/workspace/execution.rs", "tests/unit/workspace/execution_timing.rs",
    "tests/unit/evidence/failure_memory.rs", "tests/unit/integrations/git/watch.rs",
)
REQUIRED_FILES = (
    "Cargo.toml", "Cargo.lock", "LICENSE", "NOTICE", "README.md",
    "crates/core-types/Cargo.toml", "crates/core-types/src/lib.rs",
    "crates/core-types/src/reports.rs",
    ".gitattributes", ".wcode/architecture.toml", "tests/architecture.rs",
    "install.sh", "install.ps1", "marketplace.json",
    ".wcode/project.yaml", ".wcode/design/product.yaml",
    ".github/workflows/release.yml", ".github/workflows/oss.yml",
    "docs/assets/wcode-logo.svg", "plugin/plugin.json", "plugin/mcp.json",
    "plugin/README.md", "plugin/CONNECTIONS.md", "plugin/marketplace.json",
    "plugin/.claude-plugin/plugin.json", "plugin/.codex-plugin/plugin.json",
    "plugin/.zcode-plugin/plugin.json", "plugin/skills/wcode/SKILL.md",
    "src/lib.rs", "src/main.rs", "tests/source_layout.rs",
    "tests/standalone.py", "tests/unit/architecture/standalone.py",
)
ENV_KEYS = (
    "PATH", "HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "LOCALAPPDATA",
    "APPDATA", "SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT",
    "TEMP", "TMP", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
    "LANG", "LC_ALL", "TZ", "NUMBER_OF_PROCESSORS",
)
# Preserve tool discovery and library paths, not CL/LINK/RUSTFLAGS options.
# MSVC requires INCLUDE/LIB/LIBPATH; find-msvc-tools also uses the installation
# and SDK variables below. Keep them Windows-only and match names as Windows does.
WINDOWS_ENV_KEYS = (
    "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData",
    "ALLUSERSPROFILE", "INCLUDE", "LIB", "LIBPATH", "VCINSTALLDIR",
    "VSINSTALLDIR", "VCToolsInstallDir", "VCToolsVersion", "WindowsSdkDir",
    "WindowsSDKVersion", "WindowsSDKLibVersion", "UniversalCRTSdkDir", "UCRTVersion",
    "VSCMD_ARG_HOST_ARCH", "VSCMD_ARG_TGT_ARCH",
)
GENERATED_FILES = {"Cargo.toml.orig", ".cargo_vcs_info.json"}
OSS_PACKAGE_SPECS = (
    ("wcode", ".", ""),
    ("wcode-core-types", "crates/core-types", "crates/core-types"),
)
WINDOWS_DEVICES = {"con", "prn", "aux", "nul"} | {
    f"{prefix}{number}" for prefix in ("com", "lpt") for number in range(1, 10)
}


class StandaloneError(RuntimeError):
    """A controlled diagnostic with no child output or environment."""


class StageReport:
    """Atomic test progress, never verification Evidence or an acceptance receipt."""

    def __init__(self, path: Path):
        self.path = path.absolute()
        self.events: list[dict[str, object]] = []
        self.started = time.monotonic()
        self.last_bytes: bytes | None = None
        self.status = "running"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._persist(initial=True)

    def _persist(self, initial: bool = False) -> None:
        payload = json.dumps({
            "format_version": 1, "kind": "oss_standalone_test_report_not_evidence",
            "status": self.status, "elapsed_seconds": round(time.monotonic() - self.started, 3),
            "updated_at_unix_ms": int(time.time() * 1000), "stages": self.events,
        }, separators=(",", ":")).encode("utf-8") + b"\n"
        if len(payload) > MAX_REPORT_BYTES:
            raise StandaloneError("test report exceeds its fixed byte bound")
        temporary = None
        try:
            if not initial:
                if self.path.is_symlink() or self.path.stat().st_size > MAX_REPORT_BYTES:
                    raise StandaloneError("test report was replaced or enlarged")
                if self.path.read_bytes() != self.last_bytes:
                    raise StandaloneError("test report changed outside this run")
            descriptor, name = tempfile.mkstemp(prefix=".oss-report-", dir=self.path.parent)
            temporary = Path(name)
            with os.fdopen(descriptor, "wb") as output:
                output.write(payload)
                output.flush()
                os.fsync(output.fileno())
            if initial:
                # Publish a complete JSON document without replacing an unknown run.
                os.link(temporary, self.path)
            else:
                os.replace(temporary, self.path)
            self.last_bytes = payload
        except OSError as error:
            raise StandaloneError("test report could not be atomically persisted") from error
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)

    def record(self, stage: str, status: str, details: dict[str, object]) -> None:
        if (not re.fullmatch(r"[a-z0-9_]{1,64}", stage)
                or status not in ("running", "passed", "failed")
                or len(self.events) >= MAX_REPORT_EVENTS):
            raise StandaloneError("test report event is invalid or exceeds its fixed bound")
        event: dict[str, object] = {"stage": stage, "status": status}
        for key in ("seconds", "exit_code", "output_bytes"):
            if key in details:
                value = details[key]
                if value is not None and (not isinstance(value, (int, float))
                                           or isinstance(value, bool)):
                    raise StandaloneError("test report metric is invalid")
                event[key] = value
        self.events.append(event)
        if stage == "oss_standalone" and status in ("passed", "failed"):
            self.status = status
        self._persist()


REPORT: StageReport | None = None


def emit(stage: str, status: str, **details: object) -> None:
    if REPORT is not None:
        REPORT.record(stage, status, details)
    print(json.dumps({"stage": stage, "status": status, **details}), flush=True)


def controlled_env(source: dict[str, str] | None = None) -> dict[str, str]:
    source = os.environ if source is None else source
    if os.name == "nt":
        folded = {key.upper(): value for key, value in source.items()}
        env = {key: folded[key.upper()] for key in (*ENV_KEYS, *WINDOWS_ENV_KEYS)
               if key.upper() in folded}
    else:
        env = {key: source[key] for key in ENV_KEYS if key in source}
    env.update({
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never", "RUST_BACKTRACE": "0",
        "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0", "GIT_ATTR_NOSYSTEM": "1",
    })
    return env


def stop_process(process: subprocess.Popen[bytes], env: dict[str, str]) -> None:
    if os.name == "nt":
        try:
            subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, env=env, timeout=10, check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            pass
        if process.poll() is None:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired as error:
        raise StandaloneError("subprocess cleanup did not complete") from error


class RustFailureDiagnostics:
    """Untrusted diagnostic hints: accept only standard ASCII Rust test IDs/counts."""

    def __init__(self):
        self.pending = bytearray()
        self.discarding = False
        self.partial = False
        self.tests: set[str] = set()
        self.summaries: list[dict[str, int]] = []
        self.locations: set[tuple[str, int, int]] = set()
        self.error_codes: set[str] = set()
        self.categories: set[str] = set()
        self.native_codes: set[str] = set()
        self.native_locations: set[tuple[str, str, int, int]] = set()

    def _line(self, raw: bytes) -> None:
        try:
            line = raw.rstrip(b"\r").decode("ascii")
        except UnicodeDecodeError:
            return
        # Never echo the message body: it may contain paths, source or credentials.
        # These are untrusted hints only; the subprocess exit code still decides failure.
        code = re.match(r"^error\[(E[0-9]{4})\]: ", line)
        linker_code = re.search(r"(?:^|[ \t])(?:fatal )?error (LNK[0-9]{4}): ", line)
        if code or linker_code:
            value = (code or linker_code).group(1)
            if len(self.error_codes) < MAX_FAILURE_TESTS or value in self.error_codes:
                self.error_codes.add(value)
            else:
                self.partial = True
            self.categories.add("rust_compiler_error" if code else "linker_error")
        native = re.fullmatch(r'\s*"code": "([a-z_]{1,64})",?', line)
        if native and native.group(1) in NATIVE_FAILURE_CODES:
            self.native_codes.add(native.group(1))
        native_location = re.fullmatch(
            r"thread '((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*)'"
            r"(?: \([0-9]{1,9}\))? panicked at "
            r"(?:src[/\\](?:(?:[A-Za-z0-9_-]+|\.\.)[/\\]){0,12})?"
            r"(tests[/\\]unit[/\\][A-Za-z0-9_/\\.-]+\.rs):"
            r"([1-9][0-9]{0,8}):([1-9][0-9]{0,8}):", line)
        if native_location:
            name, path, row, column = native_location.groups()
            path = path.replace("\\", "/")
            if path in NATIVE_FAILURE_FILES and len(name) <= 256:
                entry = (name, path, int(row), int(column))
                if len(self.native_locations) < MAX_FAILURE_TESTS or entry in self.native_locations:
                    self.native_locations.add(entry)
                else:
                    self.partial = True
        lower = line.lower()
        categories = {
            "manifest_error": ("failed to parse manifest", "failed to load manifest"),
            "lockfile_update_required": ("lock file", "needs to be updated", "--locked"),
            "dependency_not_cached": ("no matching package named", "failed to download"),
            "toolchain_unavailable": ("toolchain", "is not installed"),
            "linker_unavailable": ("linker", "not found"),
            "linker_error": ("linking with", "failed"),
            "disk_space": ("no space left on device", "not enough space on the disk"),
            "permission_denied": ("permission denied", "access is denied"),
            "path_too_long": ("filename or extension is too long", "file name too long"),
            "process_spawn": ("could not execute process", "failed to run custom build command",
                              "failed to start command", "failed to start workspace verification executable"),
            "process_queue_timeout": ("process queue remained busy", "cargo contention gate remained busy"),
            "sandbox_unavailable": ("sandbox_unavailable:",),
        }
        all_required = {"lockfile_update_required", "toolchain_unavailable",
                        "linker_unavailable", "linker_error"}
        if not any(ord(character) < 32 and character != "\t" for character in line):
            for category, markers in categories.items():
                matches = [marker in lower for marker in markers]
                if (all(matches) if category in all_required else any(matches)):
                    self.categories.add(category)
        match = re.fullmatch(
            r"test ((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*) \.\.\. FAILED",
            line)
        if match:
            name = match.group(1)
            if len(name) > 256:
                self.partial = True
            elif len(self.tests) < MAX_FAILURE_TESTS or name in self.tests:
                self.tests.add(name)
            else:
                self.partial = True
            return
        location = re.fullmatch(
            r"thread '((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*)'"
            r"(?: \([0-9]{1,9}\))? panicked at tests[/\\]architecture\.rs:"
            r"([1-9][0-9]{0,8}):([1-9][0-9]{0,8}):", line)
        if location:
            name, row, column = location.groups()
            entry = (name, int(row), int(column))
            if len(name) > 256:
                self.partial = True
            elif len(self.locations) < MAX_FAILURE_TESTS or entry in self.locations:
                self.locations.add(entry)
            else:
                self.partial = True
            return
        summary = re.fullmatch(
            r"test result: FAILED\. ([0-9]{1,9}) passed; ([0-9]{1,9}) failed; "
            r"([0-9]{1,9}) ignored; ([0-9]{1,9}) measured; ([0-9]{1,9}) filtered out; "
            r"finished in [0-9]{1,9}\.[0-9]{1,9}s", line)
        if summary:
            if len(self.summaries) < MAX_FAILURE_TESTS:
                self.summaries.append(dict(zip(
                    ("passed", "failed", "ignored", "measured", "filtered_out"),
                    map(int, summary.groups()))))
            else:
                self.partial = True

    def feed(self, chunk: bytes) -> None:
        pieces = chunk.split(b"\n")
        for index, piece in enumerate(pieces):
            complete = index < len(pieces) - 1
            if not self.discarding:
                if len(self.pending) + len(piece) > MAX_FAILURE_LINE_BYTES:
                    self.pending.clear()
                    self.discarding = True
                    self.partial = True
                else:
                    self.pending.extend(piece)
            if complete:
                if not self.discarding:
                    self._line(bytes(self.pending))
                self.pending.clear()
                self.discarding = False

    def finish(self) -> dict[str, object]:
        if self.pending and not self.discarding:
            self._line(bytes(self.pending))
        self.pending.clear()
        return {"failure_tests": sorted(self.tests), "failure_summaries": self.summaries,
                "failure_error_codes": sorted(self.error_codes),
                "failure_categories": sorted(self.categories),
                "failure_locations": [{"test": name, "line": row, "column": column}
                                      for name, row, column in sorted(self.locations)],
                "failure_native_codes": sorted(self.native_codes),
                "failure_native_locations": [
                    {"test": name, "file": path, "line": row, "column": column}
                    for name, path, row, column in sorted(self.native_locations)],
                "failure_diagnostics_partial": self.partial}


def cached_cargo_args(command: str, target: Path) -> list[str]:
    if command not in ("build", "test"):
        raise StandaloneError("unsupported top-level Cargo cache operation")
    return ["cargo", command, "--locked", "--offline", "--target-dir", str(target)]


def run_stage(stage: str, argv: list[str], cwd: Path, env: dict[str, str],
              timeout: float, output_limit: int = MAX_OUTPUT_BYTES,
              capture_output: bool = False, diagnostics: bool = False) -> bytes:
    started = time.monotonic()
    emit(stage, "running")
    try:
        process = subprocess.Popen(
            argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE if capture_output else subprocess.STDOUT,
            bufsize=0, start_new_session=os.name != "nt",
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0,
        )
    except OSError as error:
        emit(stage, "failed", seconds=round(time.monotonic() - started, 3),
             exit_code=None, output_bytes=0)
        raise StandaloneError("required executable could not start") from error
    output_bytes = 0
    captured: list[bytes] = []
    failures = RustFailureDiagnostics() if diagnostics else None
    lock = threading.Lock()
    overflow = threading.Event()
    read_error = threading.Event()

    def drain(stream, retain: bool, inspect: bool) -> None:
        nonlocal output_bytes
        try:
            while chunk := stream.read(65536):
                if inspect and failures is not None:
                    failures.feed(chunk)
                with lock:
                    output_bytes += len(chunk)
                    if output_bytes > output_limit:
                        overflow.set()
                    elif retain:
                        captured.append(chunk)
        except OSError:
            read_error.set()
        finally:
            stream.close()

    assert process.stdout is not None
    streams = [(process.stdout, capture_output, True)]
    if capture_output:
        assert process.stderr is not None
        streams.append((process.stderr, False, False))
    readers = [threading.Thread(target=drain, args=entry, daemon=True) for entry in streams]
    for reader in readers:
        reader.start()
    failure = None
    try:
        while process.poll() is None:
            if overflow.is_set():
                failure = "subprocess output exceeded the fixed bound"
                break
            if time.monotonic() - started >= timeout:
                failure = "subprocess exceeded its stage deadline"
                break
            overflow.wait(0.05)
        if failure:
            stop_process(process, env)
        for reader in readers:
            reader.join(timeout=2)
        if any(reader.is_alive() for reader in readers):
            stop_process(process, env)
            for reader in readers:
                reader.join(timeout=2)
            failure = failure or "subprocess retained output handles"
        if overflow.is_set():
            failure = failure or "subprocess output exceeded the fixed bound"
        if read_error.is_set():
            failure = failure or "subprocess output could not be read completely"
        if process.returncode != 0:
            failure = failure or "subprocess returned a nonzero exit code"
    except BaseException:
        stop_process(process, env)
        raise
    hints = failures.finish() if failure and failures is not None else {}
    if hints and (overflow.is_set() or read_error.is_set()):
        hints["failure_diagnostics_partial"] = True
    emit(stage, "failed" if failure else "passed",
         seconds=round(time.monotonic() - started, 3),
         exit_code=process.returncode, output_bytes=output_bytes, **hints)
    if failure:
        raise StandaloneError(failure)
    return b"".join(captured)


def archive_parts(name: str, expected_root: str) -> tuple[str, ...]:
    if not name or len(name) > 4096 or "\\" in name or "\0" in name:
        raise StandaloneError("archive contains an invalid path")
    path = PurePosixPath(name)
    raw_parts = name.rstrip("/").split("/")
    if (path.is_absolute() or len(raw_parts) > 64
            or any(part in ("", ".", "..") for part in raw_parts)):
        raise StandaloneError("archive path escapes its package root")
    if not raw_parts or raw_parts[0] != expected_root:
        raise StandaloneError("archive contains an unexpected or multiple root")
    for part in raw_parts:
        if (len(part) > 255 or part.endswith((".", " "))
                or any(character in part for character in ':<>"|?*')
                or any(ord(character) < 32 for character in part)
                or part.split(".")[0].casefold() in WINDOWS_DEVICES):
            raise StandaloneError("archive path is not portable")
        if part.casefold() in ("commercial", ".git"):
            raise StandaloneError("archive contains a forbidden source subtree")
    return tuple(raw_parts)


def validate_package_sources(
        root: Path, payload: bytes, package_root: str,
        generated_files: set[str] | None = None) -> set[str]:
    """Check the real pre-package inputs, before Cargo can dereference links."""
    if len(payload) > MAX_METADATA_BYTES:
        raise StandaloneError("Cargo source inventory exceeds its fixed byte bound")
    try:
        names = payload.decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        raise StandaloneError("Cargo source inventory is not valid UTF-8") from error
    if not names or len(names) > MAX_MEMBERS:
        raise StandaloneError("Cargo source inventory is empty or exceeds its fixed count bound")
    root = root.resolve(strict=True)
    generated_files = GENERATED_FILES if generated_files is None else generated_files
    checked: set[str] = set()
    portable: set[str] = set()
    for name in names:
        # Cargo may display native separators on Windows; archives use '/'.
        if os.name == "nt":
            name = name.replace("\\", "/")
        parts = archive_parts(package_root + "/" + name, package_root)[1:]
        if not parts or name.endswith("/"):
            raise StandaloneError("Cargo source inventory contains a non-file path")
        relative = "/".join(parts)
        if relative.casefold() in portable:
            raise StandaloneError("Cargo source inventory contains a duplicate path")
        portable.add(relative.casefold())
        current = root
        mode = None
        for index, part in enumerate(parts):
            current = current / part
            try:
                mode = current.lstat().st_mode
            except FileNotFoundError as error:
                if relative in generated_files and index == len(parts) - 1:
                    mode = None
                    break
                raise StandaloneError("Cargo source inventory has a missing input") from error
            if stat.S_ISLNK(mode):
                raise StandaloneError("Cargo source inventory contains a source symlink")
            canonical = current.resolve(strict=True)
            try:
                owned = canonical.relative_to(root)
            except ValueError as error:
                raise StandaloneError("Cargo source input escapes the OSS root") from error
            if owned.parts and owned.parts[0].casefold() == "commercial":
                raise StandaloneError("Cargo source input belongs to commercial")
            if index < len(parts) - 1 and not stat.S_ISDIR(mode):
                raise StandaloneError("Cargo source inventory has a non-directory ancestor")
        if mode is not None and not stat.S_ISREG(mode):
            raise StandaloneError("Cargo source inventory contains a non-file input")
        checked.add(relative)
    return checked


def unpack_package(archive_path: Path, destination: Path,
                   expected_root: str) -> tuple[Path, dict[str, str]]:
    if archive_path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise StandaloneError("compressed source package exceeds its fixed bound")
    destination.mkdir(parents=True, exist_ok=False)
    tar_path = destination / "source.tar"
    expanded = 0
    try:
        with gzip.open(archive_path, "rb") as source, tar_path.open("xb") as output:
            while chunk := source.read(65536):
                expanded += len(chunk)
                if expanded > MAX_TAR_BYTES:
                    raise StandaloneError("expanded source package exceeds its fixed bound")
                output.write(chunk)
        entries: dict[str, tuple[str, bool]] = {}
        files: dict[str, str] = {}
        count = 0
        content_bytes = 0
        with tarfile.open(tar_path, "r:") as archive:
            for member in archive:
                count += 1
                if count > MAX_MEMBERS:
                    raise StandaloneError("source package has too many entries")
                parts = archive_parts(member.name, expected_root)
                if not (member.isfile() or member.isdir()) or (len(parts) < 2 and member.isfile()):
                    raise StandaloneError("archive contains a link, special or root file entry")
                if member.size < 0 or member.size > MAX_FILE_BYTES:
                    raise StandaloneError("source package entry exceeds its fixed bound")
                if member.isdir() and member.size:
                    raise StandaloneError("archive directory contains a payload")
                content_bytes += member.size
                if content_bytes > MAX_CONTENT_BYTES:
                    raise StandaloneError("source package contents exceed their fixed bound")
                for index in range(1, len(parts) + 1):
                    prefix = "/".join(parts[:index])
                    key = prefix.casefold()
                    is_file = index == len(parts) and member.isfile()
                    previous = entries.get(key)
                    if previous and (previous[0] != prefix or previous[1] or is_file):
                        raise StandaloneError("archive contains a duplicate or conflicting path")
                    entries[key] = (prefix, is_file)
                target = destination.joinpath(*parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                if source is None:
                    raise StandaloneError("archive file payload is missing")
                digest = hashlib.sha256()
                remaining = member.size
                with source, target.open("xb") as output:
                    while remaining:
                        chunk = source.read(min(65536, remaining))
                        if not chunk:
                            raise StandaloneError("archive file payload is incomplete")
                        output.write(chunk)
                        digest.update(chunk)
                        remaining -= len(chunk)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)
                files["/".join(parts[1:])] = digest.hexdigest()
            with tar_path.open("rb") as tail:
                tail.seek(archive.offset)
                while chunk := tail.read(65536):
                    if any(chunk):
                        raise StandaloneError("archive contains trailing hidden payload")
        if not files:
            raise StandaloneError("source package is empty")
    except (OSError, EOFError, tarfile.TarError) as error:
        raise StandaloneError("source package could not be safely decoded") from error
    finally:
        tar_path.unlink(missing_ok=True)
    return destination / expected_root, files


def validate_tree(root: Path, files: dict[str, str], env: dict[str, str]) -> None:
    if any(not (root / name).is_file() for name in REQUIRED_FILES):
        raise StandaloneError("source package omits a required OSS build or test resource")
    if not any(name.startswith("tests/unit/") for name in files):
        raise StandaloneError("source package omits the attached unit tests")
    validate_package(read_package(root, env, "standalone_metadata"))


def remove_generated_metadata(root: Path, files: dict[str, str], unpack_parent: Path) -> int:
    """Remove only verified generated files in this run's private unpack tree."""
    if root.is_symlink() or unpack_parent.is_symlink():
        raise StandaloneError("generated metadata cleanup cannot follow a symlink")
    root = root.resolve(strict=True)
    if root.parent != unpack_parent.resolve(strict=True):
        raise StandaloneError("generated metadata cleanup is outside the private unpack tree")
    removals: list[Path] = []
    for name in sorted(GENERATED_FILES):
        path = root / name
        try:
            information = path.lstat()
        except FileNotFoundError:
            if name in files:
                raise StandaloneError("verified generated metadata disappeared before cleanup")
            continue
        if name not in files or not stat.S_ISREG(information.st_mode):
            raise StandaloneError("generated metadata is unverified or is not a regular file")
        if information.st_size > MAX_FILE_BYTES:
            raise StandaloneError("generated metadata exceeds its fixed byte bound")
        digest = hashlib.sha256()
        size = 0
        with path.open("rb") as source:
            while chunk := source.read(65536):
                size += len(chunk)
                if size > MAX_FILE_BYTES:
                    raise StandaloneError("generated metadata changed beyond its fixed bound")
                digest.update(chunk)
        if digest.hexdigest() != files[name]:
            raise StandaloneError("generated metadata changed after safe unpacking")
        removals.append(path)
    for path in removals:
        path.unlink()
    return len(removals)


def compare_packages(first: dict[str, str], second: dict[str, str]) -> None:
    first = {name: digest for name, digest in first.items() if name not in GENERATED_FILES}
    second = {name: digest for name, digest in second.items() if name not in GENERATED_FILES}
    if first != second:
        raise StandaloneError("standalone repackaging changed the OSS source payload")


def validate_package(package: dict[str, object], expected_name: str = "wcode") -> str:
    name, version = package.get("name"), package.get("version")
    if name != expected_name or not isinstance(version, str) or not re.fullmatch(
            r"[0-9A-Za-z][0-9A-Za-z.+_-]*", version):
        raise StandaloneError("unexpected OSS package identity")
    if package.get("license") != "Apache-2.0":
        raise StandaloneError("OSS package license is not Apache-2.0")
    return f"{name}-{version}"


def parse_cargo_package(
        payload: bytes, manifest: Path, expected_name: str = "wcode") -> dict[str, object]:
    if len(payload) > MAX_METADATA_BYTES:
        raise StandaloneError("Cargo metadata exceeds its fixed byte bound")
    try:
        metadata = json.loads(payload.decode("utf-8"))
        if (not isinstance(metadata, dict) or type(metadata.get("version")) is not int
                or metadata.get("version") != 1
                or not isinstance(metadata.get("packages"), list)
                or len(metadata["packages"]) > 128):
            raise StandaloneError("Cargo metadata has an invalid package inventory")
        matches = []
        expected = manifest.resolve(strict=True)
        for package in metadata["packages"]:
            if not isinstance(package, dict) or not isinstance(package.get("manifest_path"), str):
                raise StandaloneError("Cargo metadata has an invalid manifest identity")
            candidate = Path(package["manifest_path"])
            if not candidate.is_absolute():
                raise StandaloneError("Cargo metadata manifest identity is not absolute")
            if candidate.resolve() == expected:
                matches.append(package)
        if len(matches) != 1:
            raise StandaloneError("Cargo metadata does not identify one exact OSS package")
        validate_package(matches[0], expected_name)
        return matches[0]
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise StandaloneError("Cargo metadata is incomplete or malformed") from error


def read_package(
        root: Path, env: dict[str, str], stage: str,
        expected_name: str = "wcode") -> dict[str, object]:
    manifest = root / "Cargo.toml"
    if manifest.stat().st_size > MAX_FILE_BYTES:
        raise StandaloneError("Cargo manifest exceeds the fixed bound")
    if tomllib is not None:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        package = data.get("package")
        if not isinstance(package, dict):
            raise StandaloneError("Cargo manifest omits the OSS package")
        return package
    payload = run_stage(stage, [
        "cargo", "metadata", "--locked", "--offline", "--no-deps",
        "--format-version", "1",
    ], root, env, 120, MAX_METADATA_BYTES, capture_output=True)
    return parse_cargo_package(payload, manifest, expected_name)


def workspace_inventories(
        root: Path, env: dict[str, str], stage_prefix: str
) -> tuple[str, list[tuple[Path, str, set[str]]], set[str]]:
    root = root.resolve(strict=True)
    version: str | None = None
    inventories: list[tuple[Path, str, set[str]]] = []
    combined: dict[str, str] = {}
    for package_name, relative_root, prefix in OSS_PACKAGE_SPECS:
        package_dir = (root / relative_root).resolve(strict=True)
        try:
            package_dir.relative_to(root)
        except ValueError as error:
            raise StandaloneError("OSS package root escapes the workspace") from error
        safe_name = package_name.replace("-", "_")
        package = read_package(
            package_dir, env, f"{stage_prefix}_{safe_name}_metadata", package_name)
        package_root = validate_package(package, package_name)
        current_version = str(package["version"])
        if version is None:
            version = current_version
        elif current_version != version:
            raise StandaloneError("OSS workspace package versions diverge")
        payload = run_stage(
            f"{stage_prefix}_{safe_name}_inventory",
            ["cargo", "package", "--list", "--locked", "--offline", "--allow-dirty",
             "-p", package_name],
            root, env, 300, MAX_METADATA_BYTES, capture_output=True)
        generated = GENERATED_FILES if relative_root == "." else GENERATED_FILES | {"Cargo.lock"}
        names = validate_package_sources(
            package_dir, payload, package_root, generated)
        retained: set[str] = set()
        for name in names:
            source = package_dir / name
            if name in generated:
                if source.exists() or source.is_symlink():
                    raise StandaloneError("reserved generated metadata exists in OSS source")
                continue
            relative = f"{prefix}/{name}" if prefix else name
            folded = relative.casefold()
            if folded in combined:
                raise StandaloneError("OSS workspace packages overlap by portable path")
            combined[folded] = relative
            retained.add(name)
        inventories.append((package_dir, prefix, retained))
    if version is None:
        raise StandaloneError("OSS workspace has no packages")
    return version, inventories, set(combined.values())


def source_file_identity(package_root: Path, source: Path) -> tuple[int, int, str]:
    try:
        information = source.lstat()
    except OSError as error:
        raise StandaloneError("OSS source input could not be inspected") from error
    if not stat.S_ISREG(information.st_mode) or stat.S_ISLNK(information.st_mode):
        raise StandaloneError("OSS source input is not a regular file")
    try:
        source.resolve(strict=True).relative_to(package_root.resolve(strict=True))
    except (OSError, ValueError) as error:
        raise StandaloneError("OSS source input escapes its package root") from error
    if information.st_size > MAX_FILE_BYTES:
        raise StandaloneError("OSS source input exceeds its fixed byte bound")
    digest = hashlib.sha256()
    size = 0
    try:
        with source.open("rb") as stream:
            while chunk := stream.read(65536):
                size += len(chunk)
                if size > MAX_FILE_BYTES:
                    raise StandaloneError("OSS source input grew beyond its fixed byte bound")
                digest.update(chunk)
    except OSError as error:
        raise StandaloneError("OSS source input could not be read completely") from error
    return size, information.st_mode, digest.hexdigest()


def write_workspace_archive(
        archive_path: Path, bundle_root: str,
        inventories: list[tuple[Path, str, set[str]]]
) -> dict[str, str]:
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    if archive_path.exists() or archive_path.is_symlink():
        raise StandaloneError("OSS workspace archive destination already exists")
    files: dict[str, str] = {}
    portable: set[str] = set()
    content_bytes = 0
    count = 0
    try:
        with tarfile.open(archive_path, "x:gz", format=tarfile.PAX_FORMAT) as archive:
            for package_root, prefix, names in inventories:
                for name in sorted(names):
                    source = package_root / name
                    relative = f"{prefix}/{name}" if prefix else name
                    archive_parts(f"{bundle_root}/{relative}", bundle_root)
                    key = relative.casefold()
                    if key in portable:
                        raise StandaloneError("OSS workspace archive contains duplicate paths")
                    portable.add(key)
                    size, mode, digest = source_file_identity(package_root, source)
                    count += 1
                    content_bytes += size
                    if count > MAX_MEMBERS or content_bytes > MAX_CONTENT_BYTES:
                        raise StandaloneError("OSS workspace archive exceeds its fixed bounds")
                    info = tarfile.TarInfo(f"{bundle_root}/{relative}")
                    info.size = size
                    info.mode = 0o755 if mode & 0o111 else 0o644
                    info.mtime = 0
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    with source.open("rb") as stream:
                        archive.addfile(info, stream)
                    after_size, _, after_digest = source_file_identity(package_root, source)
                    if after_size != size or after_digest != digest:
                        raise StandaloneError("OSS source input changed during archive creation")
                    files[relative] = digest
    except (OSError, tarfile.TarError) as error:
        raise StandaloneError("OSS workspace archive could not be created safely") from error
    if not files:
        raise StandaloneError("OSS workspace archive is empty")
    return files


def verify_standalone(root: Path, target: Path, diagnostics: bool = False) -> None:
    env = controlled_env()
    version, inventories, source_names = workspace_inventories(root, env, "source")
    bundle_root = f"wcode-workspace-{version}"
    package_args = ["cargo", "package", "--locked", "--offline",
                    "--allow-dirty", "--no-verify"]
    with tempfile.TemporaryDirectory(prefix="wcode-oss-") as temporary:
        work = Path(temporary)
        source_archive = work / "source-workspace.tar.gz"
        expected_files = write_workspace_archive(
            source_archive, bundle_root, inventories)
        if set(expected_files) != source_names:
            raise StandaloneError("workspace archive inventory changed before packing")
        standalone, original_files = unpack_package(
            source_archive, work / "unpacked", bundle_root)
        if original_files != expected_files:
            raise StandaloneError("workspace archive differs from validated source inputs")
        validate_tree(standalone, original_files, env)
        emit("safe_unpack", "passed", files=len(original_files),
             source_sha256=hashlib.sha256(source_archive.read_bytes()).hexdigest(),
             commercial_present=False)

        core_target = work / "core-package-target"
        run_stage("core_package", package_args + [
            "-p", "wcode-core-types", "--target-dir", str(core_target),
        ], root, env, 300)
        core_root = f"wcode-core-types-{version}"
        core_archive = core_target / "package" / f"{core_root}.crate"
        if not core_archive.is_file():
            raise StandaloneError("Cargo did not produce the extracted core package")
        core_tree, core_files = unpack_package(
            core_archive, work / "core-unpacked", core_root)
        validate_package(
            read_package(core_tree, env, "core_package_metadata", "wcode-core-types"),
            "wcode-core-types")
        if not {"Cargo.toml", "src/lib.rs", "src/reports.rs"}.issubset(core_files):
            raise StandaloneError("extracted core package omits required source")
        emit("core_safe_unpack", "passed", files=len(core_files),
             source_sha256=hashlib.sha256(core_archive.read_bytes()).hexdigest(),
             commercial_present=False)

        hooks = work / "empty-hooks"
        hooks.mkdir()
        git = ["git", "-c", "core.hooksPath=" + str(hooks),
               "-c", "core.autocrlf=false", "-c", "commit.gpgSign=false",
               "-c", "user.name=OSS standalone verification",
               "-c", "user.email=oss-standalone@invalid"]
        run_stage("git_baseline_init", git + ["init"], standalone, env, 30)
        run_stage("git_baseline_stage", git + ["add", "--force", "--all"], standalone, env, 30)
        run_stage("git_baseline_commit", git + ["commit", "--no-verify", "-m",
                  "Temporary OSS workspace baseline"], standalone, env, 30)
        run_stage("standalone_build", cached_cargo_args("build", target) + ["--workspace"],
                  standalone, env, 1800, diagnostics=diagnostics)
        run_stage("standalone_test", cached_cargo_args("test", target) + ["--workspace"],
                  standalone, env, 2400, diagnostics=diagnostics)

        repack_version, repack_inventories, repack_names = workspace_inventories(
            standalone, env, "repack")
        if repack_version != version or repack_names != source_names:
            raise StandaloneError("standalone workspace source inventory changed")
        repack_archive = work / "repacked-workspace.tar.gz"
        repacked_expected = write_workspace_archive(
            repack_archive, bundle_root, repack_inventories)
        _, repacked_files = unpack_package(
            repack_archive, work / "repacked", bundle_root)
        if repacked_expected != expected_files:
            raise StandaloneError("standalone workspace source bytes changed")
        compare_packages(original_files, repacked_files)
        emit("payload_identity", "passed", files=len(repacked_files),
             commercial_present=False)
    emit("oss_standalone", "passed",
         stages=["source_wcode_inventory", "source_wcode_core_types_inventory",
                 "safe_unpack", "core_package", "core_safe_unpack",
                 "standalone_build", "standalone_test",
                 "repack_wcode_inventory", "repack_wcode_core_types_inventory",
                 "payload_identity"])


def main() -> int:
    global REPORT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--target-dir", type=Path,
                        help="External Cargo build cache; never copied into the OSS source tree")
    parser.add_argument("--report", type=Path,
                        help="New atomic JSON test report; existing reports are never overwritten")
    parser.add_argument("--diagnostics", action="store_true",
                        help="Emit bounded standard Rust failure test IDs/counts; never raw output")
    args = parser.parse_args()
    try:
        script_root = Path(__file__).resolve().parents[1]
        allowed_target = script_root / "target"
        if allowed_target.is_symlink():
            raise StandaloneError("the OSS target root must not be a symlink")
        if args.report is not None:
            report_path = args.report.absolute()
            try:
                report_path.parent.resolve().relative_to(allowed_target.resolve())
            except ValueError as error:
                raise StandaloneError("test report must be inside the OSS target root") from error
            REPORT = StageReport(report_path)
        root = args.workspace.resolve(strict=True)
        if root != script_root:
            raise StandaloneError("workspace must be this script's OSS source root")
        target = args.target_dir or Path(os.environ.get("CARGO_TARGET_DIR", str(root / "target")))
        target = target.resolve()
        try:
            target.relative_to(allowed_target.resolve())
        except ValueError as error:
            raise StandaloneError("target-dir must be inside the OSS target root") from error
        verify_standalone(root, target, diagnostics=args.diagnostics)
    except (StandaloneError, OSError, ValueError, KeyError) as error:
        reason = str(error) if isinstance(error, StandaloneError) else (
            "verification input or filesystem is invalid")
        try:
            emit("oss_standalone", "failed", reason=reason)
        except StandaloneError:
            # The last durable report remains non-successful when persistence fails.
            REPORT = None
            emit("oss_standalone", "failed", reason="test report persistence failed")
        return 1
    finally:
        REPORT = None
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
