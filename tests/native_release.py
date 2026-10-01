#!/usr/bin/env python3
"""Exercise the built release through real stdio MCP, then verify its OSS package.

Run as the declared release runtime canary. The child is supervised, not installed
or detached. Its private temporary state never replaces the running service's
Policy, credentials or Evidence. Only the native verifier may produce a full PASS.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import queue
import stat
import subprocess
import sys
import tempfile
import threading
import time

sys.dont_write_bytecode = True
import standalone

ROOT = Path(__file__).resolve().parents[1]
MAX_MESSAGE = 2 * 1024 * 1024
REQUIRED_CHECKS = {
    "git-diff-check": "git diff --check",
    "rust-check": "cargo check --locked",
    "rust-format": "cargo fmt --check",
    "rust-test": "cargo test --locked",
    "rust-clippy": "cargo clippy --locked --all-targets -- -D warnings",
    "rust-release-build": "cargo build --release --locked",
}


class CanaryError(RuntimeError):
    """Controlled diagnostic; never include raw server output or credentials."""


def structured(response):
    if (not isinstance(response, dict) or response.get("error") is not None
            or not isinstance(response.get("result"), dict)):
        raise CanaryError("MCP request failed or its response was malformed")
    result = response["result"]
    if result.get("isError") is not False or not isinstance(result.get("structuredContent"), dict):
        raise CanaryError("MCP tool returned an error or no structured result")
    return result["structuredContent"]


def require_full_report(report):
    if (not isinstance(report, dict) or report.get("passed") is not True
            or report.get("level") != "full"
            or type(report.get("checks_run")) is not int or report["checks_run"] != 6
            or type(report.get("checks_failed")) is not int or report["checks_failed"] != 0
            or type(report.get("checks_reused")) is not int or report["checks_reused"] != 0
            or report.get("skipped_checks") != []):
        raise CanaryError("Native full verification did not execute all six gates successfully")
    checks = report.get("checks")
    if not isinstance(checks, list) or len(checks) != len(REQUIRED_CHECKS):
        raise CanaryError("Native verification gate coverage is incomplete")
    seen = set()
    for check in checks:
        if not isinstance(check, dict):
            raise CanaryError("Native check result is malformed")
        name = check.get("id")
        if (not isinstance(name, str) or name in seen or name not in REQUIRED_CHECKS
                or check.get("command") != REQUIRED_CHECKS[name]
                or check.get("success") is not True or check.get("reused") is not False
                or type(check.get("exit_code")) is not int or check["exit_code"] != 0):
            raise CanaryError("Native check failed, was reused, duplicated, or changed its command")
        seen.add(name)
    return report


def completed_report(receipt, task_id, workspace):
    if (not isinstance(receipt, dict) or receipt.get("kind") != "verification_task"
            or receipt.get("task_id") != task_id or receipt.get("workspace") != workspace
            or receipt.get("tool") != "verify_project" or receipt.get("terminal") is not True
            or receipt.get("status") != "completed" or receipt.get("result_available") is not True):
        raise CanaryError("Verification receipt is incomplete, failed, or belongs to another task")
    return require_full_report(structured({"result": receipt.get("result")}))


class StdioPeer:
    def __init__(self, binary, root, env):
        self.env = env
        self.sequence = 0
        self.messages = queue.Queue(maxsize=32)
        self.reader_failed = threading.Event()
        self.process = subprocess.Popen(
            [str(binary), "--workspace", str(root), "--performance", "fast",
             "--no-semantic", "--no-tunnel", "mcp-stdio"],
            cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, start_new_session=(os.name != "nt"),
        )
        self.readers = [threading.Thread(target=self._stdout, daemon=True),
                        threading.Thread(target=self._stderr, daemon=True)]
        for reader in self.readers:
            reader.start()

    def _stdout(self):
        try:
            while True:
                line = self.process.stdout.readline(MAX_MESSAGE + 1)
                if not line:
                    break
                if len(line) > MAX_MESSAGE or not line.endswith(b"\n"):
                    raise CanaryError("MCP output message exceeds its bound")
                self.messages.put(json.loads(line), timeout=1)
        except (OSError, ValueError, queue.Full, CanaryError):
            self.reader_failed.set()
        finally:
            try:
                self.messages.put_nowait(None)
            except queue.Full:
                self.reader_failed.set()

    def _stderr(self):
        # Drain, never retain or echo raw native diagnostics.
        try:
            while self.process.stderr.read(16384):
                pass
        except OSError:
            self.reader_failed.set()

    def send(self, message):
        data = json.dumps(message, separators=(",", ":")).encode() + b"\n"
        if len(data) > 16384:
            raise CanaryError("Canary request exceeds its fixed bound")
        try:
            self.process.stdin.write(data)
            self.process.stdin.flush()
        except (OSError, ValueError) as error:
            raise CanaryError("Native MCP input closed") from error

    def rpc(self, method, params, timeout=20):
        self.sequence += 1
        request_id = self.sequence
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.reader_failed.is_set():
                raise CanaryError("Native MCP stream is invalid or exceeded capacity")
            try:
                message = self.messages.get(timeout=min(0.25, max(0.001, deadline - time.monotonic())))
            except queue.Empty:
                continue
            if message is None:
                raise CanaryError("Native MCP process exited before answering")
            if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
                raise CanaryError("Native MCP returned an invalid envelope")
            if "method" in message:
                if "id" in message:
                    raise CanaryError("Canary cannot grant server-requested authorization")
                continue
            if type(message.get("id")) is not int or message["id"] != request_id:
                raise CanaryError("Native MCP returned a mismatched request identity")
            if message.get("error") is not None:
                raise CanaryError("Native MCP request returned an error")
            return message
        raise CanaryError("Native MCP request deadline expired")

    def tool(self, name, arguments):
        return structured(self.rpc("tools/call", {"name": name, "arguments": arguments}))

    def close(self):
        try:
            self.process.stdin.close()
            code = self.process.wait(timeout=10)
        except (OSError, ValueError, subprocess.TimeoutExpired):
            standalone.stop_process(self.process, self.env)
            code = self.process.returncode
        for reader in self.readers:
            reader.join(timeout=2)
        for stream in (self.process.stdout, self.process.stderr):
            stream.close()
        if code != 0:
            raise CanaryError("Canary-owned native MCP process did not shut down cleanly")


def native_full(binary, env):
    with tempfile.TemporaryDirectory(prefix="wcode-release-mcp-") as directory:
        isolated = dict(env, WCODE_STATE_DIR=str(Path(directory) / "state"))
        peer = StdioPeer(binary, ROOT, isolated)
        task_id = workspace = None
        try:
            peer.rpc("initialize", {
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "wcode-native-release-canary", "version": "1"},
            })
            peer.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            receipt = peer.tool("verify_project", {"action": "start", "level": "full"})
            task_id, workspace = receipt.get("task_id"), receipt.get("workspace")
            if (receipt.get("kind") != "verification_task" or not isinstance(task_id, str)
                    or not task_id or len(task_id) > 96 or not isinstance(workspace, str)
                    or not workspace or len(workspace) > 256):
                raise CanaryError("Built release lacks a usable ordinary verification receipt")
            deadline = time.monotonic() + 900
            while receipt.get("terminal") is not True:
                if time.monotonic() >= deadline:
                    raise CanaryError("Full verification task deadline expired")
                interval = receipt.get("poll_interval_ms")
                if type(interval) is not int or not 1 <= interval <= 10000:
                    raise CanaryError("Native verification polling interval is invalid")
                time.sleep(interval / 1000)
                receipt = peer.tool("verify_project", {
                    "action": "status", "task_id": task_id, "workspace": workspace,
                })
                if receipt.get("task_id") != task_id or receipt.get("workspace") != workspace:
                    raise CanaryError("Native verification changed its task identity")
            receipt = peer.tool("verify_project", {
                "action": "result", "task_id": task_id, "workspace": workspace,
            })
            return completed_report(receipt, task_id, workspace)
        except BaseException:
            if task_id and workspace:
                try:
                    peer.tool("verify_project", {
                        "action": "cancel", "task_id": task_id, "workspace": workspace,
                    })
                except CanaryError:
                    pass
            raise
        finally:
            peer.close()


def source_identity(env):
    version, inventories, names = standalone.workspace_inventories(ROOT, env, "canary")
    digest = hashlib.sha256(b"wcode-native-release-inputs-v1\0")
    observed = set()
    for package_root, prefix, paths in inventories:
        for name in sorted(paths):
            relative = f"{prefix}/{name}" if prefix else name
            size, mode, checksum = standalone.source_file_identity(package_root, package_root / name)
            digest.update(json.dumps([relative, size, mode, checksum], separators=(",", ":")).encode())
            observed.add(relative)
    if observed != names:
        raise CanaryError("Canary source inventory changed during identity capture")
    return {"version": version, "files": len(observed), "sha256": digest.hexdigest()}


def binary_identity(binary):
    if binary.is_symlink() or not binary.is_file():
        raise CanaryError("Release binary is missing or aliased")
    before = binary.stat()
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > 512 * 1024 * 1024:
        raise CanaryError("Release binary identity is invalid")
    digest = hashlib.sha256()
    with binary.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    after = binary.stat()
    if any(getattr(before, key) != getattr(after, key) for key in
           ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns", "st_mode", "st_nlink")):
        raise CanaryError("Release binary changed during identity capture")
    return digest.hexdigest()


def write_report(target, report):
    path = target / f"wcode-native-release-{time.time_ns()}.json"
    with path.open("x", encoding="utf-8") as output:
        if os.name != "nt":
            os.chmod(path, 0o600)
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    return str(path.relative_to(ROOT))


def main():
    if len(sys.argv) != 1:
        raise CanaryError("The release canary accepts no caller-controlled command or path")
    target = ROOT / "target"
    if target.is_symlink():
        raise CanaryError("Release canary target must not be a symlink")
    target.mkdir(exist_ok=True)
    env = standalone.controlled_env()
    before = source_identity(env)
    standalone.run_stage("native_release_build", ["cargo", "build", "--release", "--locked"],
                         ROOT, env, 600)
    binary = target / "release" / ("wcode.exe" if os.name == "nt" else "wcode")
    identity = binary_identity(binary)
    full = native_full(binary, env)
    standalone.emit("native_release_full", "passed", checks=full["checks_run"],
                    elapsed_ms=full["elapsed_ms"])
    package_report = target / f"oss-native-release-{time.time_ns()}.json"
    standalone.run_stage("native_release_standalone", [sys.executable, "tests/standalone.py",
                         "--report", str(package_report), "--diagnostics"], ROOT, env, 840)
    package = json.loads(package_report.read_text(encoding="utf-8"))
    if package.get("status") != "passed":
        raise CanaryError("OSS standalone report is incomplete")
    after = source_identity(env)
    if before != after or binary_identity(binary) != identity:
        raise CanaryError("Canary source or binary changed during validation")
    report = {"kind": "native_release_canary_not_acceptance", "passed": True,
              "current_acceptance": False, "remote_deployment_verified": False,
              "input": before, "binary_sha256": identity, "full": full,
              "standalone_report": str(package_report.relative_to(ROOT)),
              "standalone": package, "stable_inputs": True}
    path = write_report(target, report)
    print(json.dumps({"stage": "native_release_canary", "status": "passed", "report": path,
                      "input_sha256": before["sha256"], "current_acceptance": False}), flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (CanaryError, standalone.StandaloneError, OSError, ValueError, KeyError) as error:
        reason = str(error) if isinstance(error, (CanaryError, standalone.StandaloneError)) else type(error).__name__
        print(json.dumps({"stage": "native_release_canary", "status": "failed", "reason": reason}), flush=True)
        raise SystemExit(1)
