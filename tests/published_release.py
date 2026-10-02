"""Verify published release packages against this run's validated candidate."""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import socket
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import zipfile

REPO = os.environ["GITHUB_REPOSITORY"]
TAG = os.environ["RELEASE_TAG"]
TARGET = os.environ["CANDIDATE_SHA"]
VERSION = os.environ["CANDIDATE_VERSION"]
RELEASE_RUN = int(os.environ.get("PUBLICATION_RUN") or os.environ["GITHUB_RUN_ID"])
RUN_ATTEMPT = int(os.environ.get("PUBLICATION_ATTEMPT") or os.environ["GITHUB_RUN_ATTEMPT"])
PACKAGES = {
    "Linux": ["wcode-linux-x86_64.tar.gz"],
    "Windows": ["wcode-windows-x86_64.zip"],
    "Darwin": ["wcode-macos-aarch64.tar.gz", "wcode-macos-x86_64.tar.gz", "wcode-macos-universal.tar.gz"],
}
EXPECTED = {name for names in PACKAGES.values() for name in names}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def read_url(url, bound, api=False):
    headers = {"User-Agent": "wcode-release-download-verification"}
    if api and os.environ.get("GH_TOKEN"):
        require(url.startswith(f"https://api.github.com/repos/{REPO}/"), "Foreign API request")
        headers["Authorization"] = "Bearer " + os.environ["GH_TOKEN"]
        headers["Accept"] = "application/vnd.github+json"
        headers["X-GitHub-Api-Version"] = "2022-11-28"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=60) as response:
        data = response.read(bound + 1)
    require(len(data) <= bound, "Download exceeded its size bound")
    return data


def get_api(path):
    return json.loads(read_url(f"https://api.github.com/repos/{REPO}/{path}", 1_048_576, api=True))


def parse_checksums(data):
    checksums = {}
    for line in data.decode("ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (wcode-[A-Za-z0-9_.-]+)", line)
        require(match is not None, "Malformed checksum manifest")
        digest, name = match.groups()
        require(name in EXPECTED and name not in checksums, "Unknown or duplicate checksum entry")
        checksums[name] = digest
    require(set(checksums) == EXPECTED, "Checksum manifest does not cover all five packages")
    return checksums


def binary_bytes(data, name):
    expected_binary = "wcode.exe" if name.endswith(".zip") else "wcode"
    found = {}
    def accept(member_name, payload):
        while member_name.startswith("./"):
            member_name = member_name[2:]
        require(PurePosixPath(member_name).parts == (member_name,), "Unsafe archive member")
        require(member_name in {expected_binary, "README.md", "LICENSE"}, "Unexpected archive member")
        require(member_name not in found, "Duplicate archive member")
        found[member_name] = payload
    if name.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            require(len(archive.infolist()) <= 8, "Excessive ZIP entries")
            for item in archive.infolist():
                require(not item.is_dir() and ((item.external_attr >> 16) & 0o170000) != 0o120000,
                        "ZIP directories and symbolic links are not executable packages")
                require(item.file_size <= (134_217_728 if item.filename == expected_binary else 4_194_304), "Oversized ZIP member")
                accept(item.filename, archive.read(item))
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            for count, item in enumerate(archive):
                require(count < 8, "Excessive TAR entries")
                if item.isdir() and item.name in (".", "./"):
                    continue
                require(item.isfile() and not item.issym() and not item.islnk(), "Non-regular TAR entry")
                require(item.size <= (134_217_728 if item.name.removeprefix("./") == expected_binary else 4_194_304), "Oversized TAR member")
                payload = archive.extractfile(item).read(item.size + 1)
                require(len(payload) == item.size, "Truncated TAR member")
                accept(item.name, payload)
    require(set(found) == {expected_binary, "README.md", "LICENSE"}, "Incomplete package")
    return found[expected_binary]


def runtime_arguments(binary, workspace, port):
    # Browser launch is opt-in (--open). Suppress the desktop companion explicitly.
    return [str(binary), "--workspace", str(workspace), "--host", "127.0.0.1", "--port", str(port),
            "--read-only", "--no-exec", "--no-semantic", "--no-tunnel", "--no-monitor", "--no-menu-bar", "--allow-sleep"]


def exercise(binary, folder, name):
    workspace = folder / "workspace"
    workspace.mkdir()
    home = folder / "home"
    home.mkdir()
    env = {key: value for key, value in os.environ.items()
           if key not in ("GH_TOKEN", "GITHUB_TOKEN") and not key.startswith("WCODE_")}
    env.update(HOME=str(home), USERPROFILE=str(home), XDG_CONFIG_HOME=str(home / "config"),
               XDG_CACHE_HOME=str(home / "cache"), XDG_DATA_HOME=str(home / "data"))
    def invoke(args):
        result = subprocess.run([str(binary), *args], cwd=workspace, env=env,
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
        require(result.returncode == 0, f"{name}: command {args[0]} failed with {result.returncode}")
        return result.stdout.decode("utf-8")
    require(invoke(["--version"]).strip() == "wcode " + VERSION, "Wrong binary version")
    require("mcp-stdio" in invoke(["--help"]), "Incomplete help")
    catalog = json.loads(invoke(["help-all", "--json"]))
    require(catalog.get("version") == VERSION and catalog.get("runtime_started") is False,
            "Incorrect declarative command catalog")
    if platform.system() == "Darwin":
        subprocess.run(["codesign", "--verify", "--strict", str(binary)], check=True, capture_output=True, timeout=30)
        archs = set(subprocess.check_output(["lipo", "-archs", str(binary)], text=True, timeout=10).split())
        expected_archs = {"arm64", "x86_64"} if "universal" in name else {"arm64" if "aarch64" in name else "x86_64"}
        require(archs == expected_archs, "Mach-O architecture mismatch")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    args = runtime_arguments(binary, workspace, port)
    process = subprocess.Popen(args, cwd=workspace, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    health = None
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            require(process.poll() is None, f"{name}: runtime exited before becoming healthy")
            try:
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(
                        f"http://127.0.0.1:{port}/healthz/probe", timeout=1) as response:
                    health = json.loads(response.read(4096))
                if health.get("ok") is True and isinstance(health.get("instance_id"), str) and health["instance_id"]:
                    break
            except (OSError, ValueError):
                pass
            time.sleep(0.2)
        require(health is not None and health.get("ok") is True and bool(health.get("instance_id")), "Runtime health probe failed")
        require(process.poll() is None, "Runtime did not remain alive after health probe")
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    return {"version": "wcode " + VERSION, "help": True, "command_catalog": True, "http_startup": True,
            "macos_signature_and_architecture": True if platform.system() == "Darwin" else None}


def tag_commit():
    obj = get_api(f"git/ref/tags/{TAG}")["object"]
    for _ in range(4):
        if obj.get("type") == "commit":
            require(re.fullmatch(r"[0-9a-f]{40}", obj.get("sha", "")) is not None, "Invalid tag commit")
            return obj["sha"]
        require(obj.get("type") == "tag" and re.fullmatch(r"[0-9a-f]{40}", obj.get("sha", "")),
                "Unsupported tag object")
        obj = get_api(f"git/tags/{obj['sha']}")["object"]
    raise RuntimeError("Excessive annotated tag depth")


def validate_publication(run, allow_current_dispatch=False):
    require(run.get("id") == RELEASE_RUN and run.get("repository", {}).get("full_name") == REPO
            and run.get("run_attempt") == RUN_ATTEMPT, "Publication run identity mismatch")
    require(run.get("path") == ".github/workflows/release.yml", "Wrong publication workflow")
    if run.get("event") == "push":
        require(run.get("head_sha") == TARGET and run.get("head_branch") == TAG,
                "Publication does not belong to this tagged candidate")
        return "tag_push"
    # Only this running release workflow has the validated quality-job candidate outputs.
    # Historical dispatch runs cannot be reconstructed from their main-branch head.
    require(run.get("event") == "workflow_dispatch" and allow_current_dispatch,
            "Historical dispatch publication requires persisted candidate provenance")
    return "current_workflow_validated_candidate"


def main():
    system = platform.system()
    require(system in PACKAGES, "Unsupported verification platform")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", REPO) is not None, "Invalid repository")
    require(re.fullmatch(r"[0-9a-f]{40}", TARGET) is not None, "Invalid candidate SHA")
    require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?", VERSION) is not None,
            "Invalid candidate version")
    allowed_tags = {"v" + VERSION, "v" + VERSION.removesuffix(".0")}
    require(TAG in allowed_tags and tag_commit() == TARGET, "Release tag identity mismatch")
    run = get_api(f"actions/runs/{RELEASE_RUN}")
    current_publication = not os.environ.get("PUBLICATION_RUN") and RELEASE_RUN == int(os.environ["GITHUB_RUN_ID"])
    provenance = validate_publication(run, allow_current_dispatch=current_publication)
    # Failed archive checks can be rerun without republishing a successful release.
    jobs = get_api(f"actions/runs/{RELEASE_RUN}/jobs?filter=all&per_page=100")
    require(jobs.get("total_count") == len(jobs.get("jobs", [])) <= 100, "Incomplete publication jobs")
    publish = [job for job in jobs["jobs"] if job.get("name") == "Publish GitHub Release"]
    require(bool(publish), "Publication job is missing")
    publish = max(publish, key=lambda job: job["id"])
    require(publish.get("run_id") == RELEASE_RUN
            and publish.get("run_url") == f"https://api.github.com/repos/{REPO}/actions/runs/{RELEASE_RUN}"
            and publish.get("status") == "completed" and publish.get("conclusion") == "success",
            "Latest publication job did not succeed")
    release = get_api(f"releases/tags/{TAG}")
    require(release.get("tag_name") == TAG and release.get("draft") is False
            and release.get("prerelease") is False and release.get("published_at"), "Not a final published release")
    assets = {item["name"]: item for item in release["assets"]}
    require(len(assets) == len(release["assets"]) and set(assets) == EXPECTED | {"SHA256SUMS"}, "Release asset set mismatch")
    def download(name):
        asset = assets[name]
        require(asset.get("state") == "uploaded" and 0 < asset["size"] <= 268_435_456, "Invalid uploaded asset")
        url = f"https://github.com/{REPO}/releases/download/{TAG}/{name}"
        require(asset["browser_download_url"] == url, "Unexpected download origin")
        data = read_url(url, asset["size"])
        require(len(data) == asset["size"], "Truncated asset")
        digest = hashlib.sha256(data).hexdigest()
        require(asset.get("digest") == "sha256:" + digest, "GitHub asset digest mismatch")
        return data, digest
    manifest, manifest_sha = download("SHA256SUMS")
    checksums = parse_checksums(manifest)
    reports = []
    for name in sorted(EXPECTED):
        data, digest = download(name)
        require(digest == checksums[name], "SHA256SUMS mismatch")
        payload = binary_bytes(data, name)
        if name not in PACKAGES[system]:
            reports.append({"asset": name, "archive_sha256": digest, "archive_size": len(data),
                            "safe_members": True, "native_checks": "not_executed_on_this_platform"})
            continue
        with tempfile.TemporaryDirectory(prefix="wcode-published-") as tmp:
            folder = Path(tmp)
            binary = folder / ("wcode.exe" if system == "Windows" else "wcode")
            binary.write_bytes(payload)
            binary.chmod(0o755)
            reports.append({"asset": name, "archive_sha256": digest, "archive_size": len(data),
                            "checks": exercise(binary, folder, name)})
    require(tag_commit() == TARGET, "Tag moved during verification")
    output = {"tag": TAG, "commit": TARGET, "version": VERSION, "release_id": release["id"],
              "release_run": RELEASE_RUN, "publication_attempt": RUN_ATTEMPT, "publish_job": publish["id"],
              "verification_run": int(os.environ["GITHUB_RUN_ID"]),
              "verification_attempt": int(os.environ["GITHUB_RUN_ATTEMPT"]),
              "platform": system, "candidate_provenance": provenance, "checksums_sha256": manifest_sha, "results": reports, "passed": True}
    Path(os.environ["RUNNER_TEMP"], f"release-download-{system}.json").write_text(
        json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(output))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        report = {"tag": TAG, "commit": TARGET, "release_run": RELEASE_RUN,
                  "platform": platform.system(), "passed": False, "error_type": type(error).__name__}
        Path(os.environ["RUNNER_TEMP"], f"release-download-{platform.system()}.json").write_text(
            json.dumps(report, indent=2) + "\n", encoding="utf-8")
        raise
