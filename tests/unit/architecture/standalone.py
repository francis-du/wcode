#!/usr/bin/env python3
"""Safety regressions for the real source-package verifier; no Cargo mocking."""
import contextlib
import gzip
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / "standalone.py"
SPEC = importlib.util.spec_from_file_location("oss_standalone", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
standalone = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(standalone)
ROOT = "wcode-0.9.0"


class SourcePackageSafety(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="wcode-package-test-")
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def archive(self, entries):
        path = self.root / "fixture.crate"
        with tarfile.open(path, "w:gz") as archive:
            for name, content, kind in entries:
                info = tarfile.TarInfo(name)
                info.type = kind
                info.mode = 0o644
                if kind == tarfile.REGTYPE:
                    info.size = len(content)
                    archive.addfile(info, io.BytesIO(content))
                else:
                    info.linkname = ROOT + "/outside"
                    archive.addfile(info)
        return path

    def unpack(self, entries):
        return standalone.unpack_package(
            self.archive(entries), self.root / "output", ROOT)

    def rejected(self, entries):
        with self.assertRaises(standalone.StandaloneError):
            self.unpack(entries)
        self.assertFalse((self.root / "outside").exists())

    def test_hidden_resources_and_original_bytes_survive(self):
        entries = [
            (ROOT, b"", tarfile.DIRTYPE),
            (ROOT + "/plugin/.codex-plugin/plugin.json", b'{"version":"0.9.0"}\r\n',
             tarfile.REGTYPE),
            (ROOT + "/.wcode/design/product.yaml", b"name: wcode\n", tarfile.REGTYPE),
        ]
        tree, files = self.unpack(entries)
        self.assertEqual((tree / "plugin/.codex-plugin/plugin.json").read_bytes(),
                         entries[1][1])
        self.assertIn(".wcode/design/product.yaml", files)
        self.assertFalse((tree / "commercial").exists())

    def test_archive_rejects_link_and_special_entries(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE):
            with self.subTest(kind=kind):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.unpack_package(
                        self.archive([(ROOT + "/link", b"", kind)]),
                        self.root / ("output-" + kind.decode()), ROOT)

    def test_archive_rejects_path_escape_and_nonportable_names(self):
        names = [
            "../outside", "/outside", ROOT + "/../outside",
            ROOT + "/a/../../outside", ROOT + "\\outside", ROOT + "/C:/outside",
            ROOT + "//outside", ROOT + "/./outside", ROOT + "/NUL.txt",
            ROOT + "/trailing.", ROOT + "/a\x01",
            ROOT + "/" + "/".join(["d"] * 65) + "/outside",
            ROOT + "/" + "/".join(["x" * 250] * 17),
        ]
        for index, name in enumerate(names):
            with self.subTest(name=name):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.unpack_package(
                        self.archive([(name, b"data", tarfile.REGTYPE)]),
                        self.root / f"output-{index}", ROOT)
        self.assertFalse((self.root / "outside").exists())

    def test_archive_rejects_multiple_roots_and_commercial_payload(self):
        for index, name in enumerate(("other/src/lib.rs", ROOT + "/commercial/src/lib.rs",
                                      ROOT + "/COMMERCIAL/secret", ROOT + "/.git/config")):
            with self.subTest(name=name):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.unpack_package(
                        self.archive([(ROOT + "/LICENSE", b"Apache", tarfile.REGTYPE),
                                      (name, b"private", tarfile.REGTYPE)]),
                        self.root / f"output-{index}", ROOT)

    def test_archive_rejects_payload_hidden_after_tar_end(self):
        path = self.archive([(ROOT + "/LICENSE", b"Apache", tarfile.REGTYPE)])
        original = gzip.decompress(path.read_bytes())
        self.archive([(ROOT + "/commercial/private", b"secret", tarfile.REGTYPE)])
        path.write_bytes(gzip.compress(original + gzip.decompress(path.read_bytes())))
        with self.assertRaises(standalone.StandaloneError):
            standalone.unpack_package(path, self.root / "output", ROOT)

    def test_archive_rejects_duplicate_case_and_file_directory_conflicts(self):
        pairs = [
            ("a", "a"), ("Dir/a", "dir/b"), ("a", "a/b"), ("a/b", "a"),
        ]
        for index, (first, second) in enumerate(pairs):
            with self.subTest(pair=(first, second)):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.unpack_package(
                        self.archive([(ROOT + "/" + first, b"first", tarfile.REGTYPE),
                                      (ROOT + "/" + second, b"second", tarfile.REGTYPE)]),
                        self.root / f"output-{index}", ROOT)

    def test_archive_bounds_fail_closed(self):
        entries = [(ROOT + "/a", b"aa", tarfile.REGTYPE),
                   (ROOT + "/b", b"bb", tarfile.REGTYPE)]
        archive = self.archive(entries)
        limits = {"MAX_ARCHIVE_BYTES": 1, "MAX_TAR_BYTES": 1,
                  "MAX_CONTENT_BYTES": 3, "MAX_FILE_BYTES": 1, "MAX_MEMBERS": 1}
        for name, limit in limits.items():
            with self.subTest(bound=name), patch.object(standalone, name, limit):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.unpack_package(archive, self.root / name, ROOT)

    def test_malformed_archive_does_not_become_an_empty_success(self):
        path = self.root / "bad.crate"
        path.write_bytes(b"not a gzip archive")
        with self.assertRaises(standalone.StandaloneError):
            standalone.unpack_package(path, self.root / "output", ROOT)

    def test_repack_identity_rejects_changes_and_missing_resources(self):
        original = {"LICENSE": "a", "src/lib.rs": "b", "Cargo.toml": "c",
                    ".cargo_vcs_info.json": "old", "Cargo.toml.orig": "old"}
        same = {**original, ".cargo_vcs_info.json": "new", "Cargo.toml.orig": "new"}
        standalone.compare_packages(original, same)
        for changed in ({**same, "LICENSE": "changed"},
                        {key: value for key, value in same.items() if key != "src/lib.rs"},
                        {**same, "commercial/private": "secret"}):
            with self.assertRaises(standalone.StandaloneError):
                standalone.compare_packages(original, changed)

    def test_environment_drops_credentials_and_runtime_state_overrides(self):
        env = standalone.controlled_env({
            "PATH": os.environ["PATH"], "HOME": "local-home",
            "GH_TOKEN": "secret", "AWS_SECRET_ACCESS_KEY": "secret",
            "WCODE_STATE_DIR": "external-state", "CARGO_REGISTRY_TOKEN": "secret",
            "GIT_CONFIG_COUNT": "1", "RUSTFLAGS": "untrusted",
            "CARGO_TARGET_DIR": "untrusted-shared-target",
        })
        self.assertEqual(env["HOME"], "local-home")
        for key in ("GH_TOKEN", "AWS_SECRET_ACCESS_KEY", "WCODE_STATE_DIR",
                    "CARGO_REGISTRY_TOKEN", "GIT_CONFIG_COUNT", "RUSTFLAGS", "CARGO_TARGET_DIR"):
            self.assertNotIn(key, env)
        self.assertEqual(env["CARGO_NET_OFFLINE"], "true")

    def test_source_inventory_keeps_hidden_assets_and_only_absent_generated_entries(self):
        names = ["Cargo.toml", "docs/logo.svg", "plugin/.codex-plugin/plugin.json",
                 "Cargo.toml.orig", ".cargo_vcs_info.json"]
        for name in names[:3]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"OSS")
        checked = standalone.validate_package_sources(
            self.root, ("\n".join(names) + "\n").encode(), ROOT)
        self.assertEqual(checked, set(names))
        self.assertFalse((self.root / "Cargo.toml.orig").exists())

    def test_source_inventory_rejects_missing_escape_commercial_and_duplicate_paths(self):
        (self.root / "Cargo.toml").write_bytes(b"OSS")
        bad = [b"", b"\xff", b"missing\n", b"src/Cargo.toml.orig\n",
               b"../outside\n", b"/outside\n", b"commercial/private.rs\n",
               b"Cargo.toml\nCargo.toml\n", b"Cargo.toml\ncargo.toml\n", b"Cargo.toml\n\n"]
        for payload in bad:
            with self.subTest(payload=payload), self.assertRaises(standalone.StandaloneError):
                standalone.validate_package_sources(self.root, payload, ROOT)
        with patch.object(standalone, "MAX_MEMBERS", 1):
            with self.assertRaises(standalone.StandaloneError):
                standalone.validate_package_sources(self.root, b"Cargo.toml\nREADME.md\n", ROOT)

    def source_symlink(self, path, target, directory=False):
        try:
            path.symlink_to(target, target_is_directory=directory)
        except (OSError, NotImplementedError) as error:
            self.skipTest("OS does not permit this symlink fixture: " + type(error).__name__)

    def test_source_inventory_rejects_commercial_symlink_before_packaging(self):
        commercial = self.root / "commercial"
        commercial.mkdir()
        private = commercial / "private.rs"
        private.write_bytes(b"private implementation")
        (self.root / "src").mkdir()
        self.source_symlink(self.root / "src" / "public.rs", private)
        with self.assertRaises(standalone.StandaloneError):
            standalone.validate_package_sources(self.root, b"src/public.rs\n", ROOT)

    def test_source_inventory_rejects_external_asset_and_generated_symlinks(self):
        with tempfile.TemporaryDirectory(prefix="wcode-external-fixture-") as outside:
            external = Path(outside) / "LICENSE"
            external.write_bytes(b"external license")
            (self.root / "docs").mkdir()
            self.source_symlink(self.root / "docs" / "logo.svg", external)
            self.source_symlink(self.root / "Cargo.toml.orig", external)
            for payload in (b"docs/logo.svg\n", b"Cargo.toml.orig\n"):
                with self.assertRaises(standalone.StandaloneError):
                    standalone.validate_package_sources(self.root, payload, ROOT)

    def test_source_inventory_rejects_symlink_ancestor(self):
        commercial = self.root / "commercial"
        commercial.mkdir()
        (commercial / "private.rs").write_bytes(b"private implementation")
        self.source_symlink(self.root / "src", commercial, directory=True)
        with self.assertRaises(standalone.StandaloneError):
            standalone.validate_package_sources(self.root, b"src/private.rs\n", ROOT)

    def test_windows_environment_preserves_tool_discovery_but_not_options(self):
        tools = {
            "ProgramFiles(x86)": "C:/Program Files (x86)",
            "ProgramFiles": "C:/Program Files", "ProgramData": "C:/ProgramData",
            "VCINSTALLDIR": "C:/VS/VC", "VCToolsVersion": "14.51",
            "WindowsSdkDir": "C:/Windows Kits/10", "WindowsSDKVersion": "10.0/",
            "INCLUDE": "C:/SDK/include", "LIB": "C:/SDK/lib;C:/VC/lib",
            "LIBPATH": "C:/VC/lib", "PATH": "C:/VC/bin", "SystemRoot": "C:/Windows",
        }
        rejected = {name: "PRIVATE" for name in (
            "CL", "_CL_", "LINK", "_LINK_", "RUSTFLAGS", "RUSTC_WRAPPER",
            "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_TARGET_DIR", "GITHUB_TOKEN", "CARGO_REGISTRY_TOKEN",
        )}
        for rename in (str.upper, str.lower):
            source = {rename(key): value for key, value in {**tools, **rejected}.items()}
            with patch.object(standalone.os, "name", "nt"):
                env = standalone.controlled_env(source)
            for key, value in tools.items():
                self.assertEqual(env[key], value)
            self.assertNotIn("PRIVATE", env.values())
            self.assertEqual(env["CARGO_NET_OFFLINE"], "true")

    def test_posix_environment_does_not_inherit_windows_toolchain_overrides(self):
        with patch.object(standalone.os, "name", "posix"):
            env = standalone.controlled_env({key: "UNEXPECTED" for key in standalone.WINDOWS_ENV_KEYS})
        self.assertNotIn("UNEXPECTED", env.values())

    def test_cargo_cache_is_only_an_explicit_top_level_argument(self):
        target = self.root / "shared-target"
        env = standalone.controlled_env({"PATH": os.environ["PATH"],
                                         "CARGO_TARGET_DIR": str(target)})
        self.assertNotIn("CARGO_TARGET_DIR", env)
        for command in ("build", "test"):
            self.assertEqual(standalone.cached_cargo_args(command, target),
                             ["cargo", command, "--locked", "--offline",
                              "--target-dir", str(target)])
        with contextlib.redirect_stdout(io.StringIO()):
            payload = standalone.run_stage(
                "nested_env", [sys.executable, "-c",
                               "import os;print('CARGO_TARGET_DIR' in os.environ)"],
                self.root, env, 5, capture_output=True)
        self.assertEqual(payload.strip(), b"False")

    def test_build_diagnostics_report_categories_without_echoing_child_output(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b'error: failed to parse manifest at `PRIVATE/project/Cargo.toml`\n')
        parser.feed(b'  lock file PRIVATE/Cargo.lock needs to be updated but --locked was passed\n')
        parser.feed(b'error: linker `link.exe` not found\n')
        parser.feed(b'error[E0308]: PRIVATE type mismatch\n')
        parser.feed(b'error: no matching package named `PRIVATE` found\n')
        hints = parser.finish()
        self.assertEqual(hints['failure_error_codes'], ['E0308'])
        self.assertEqual(hints['failure_categories'], [
            'dependency_not_cached', 'linker_unavailable', 'lockfile_update_required',
            'manifest_error', 'rust_compiler_error',
        ])
        self.assertNotIn('PRIVATE', json.dumps(hints))
        self.assertNotIn('link.exe', json.dumps(hints))

    def test_build_diagnostics_are_bounded_and_reject_injected_ids(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b'error[E12345]: too long\nerror[EABCD]: nonnumeric\n')
        parser.feed(b'error[E0001] injected malformed delimiter\n')
        parser.feed(b'\x1b[31merror[E0002]: colored\n')
        for number in range(standalone.MAX_FAILURE_TESTS + 1):
            parser.feed(f'error[E{number:04d}]: hidden body\n'.encode())
        hints = parser.finish()
        self.assertEqual(len(hints['failure_error_codes']), standalone.MAX_FAILURE_TESTS)
        self.assertTrue(hints['failure_diagnostics_partial'])
        self.assertEqual(hints['failure_categories'], ['rust_compiler_error'])
        self.assertNotIn('hidden', json.dumps(hints))

    def test_linker_diagnostics_keep_only_bounded_standard_codes(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b"  = note: LINK : fatal error LNK1104: cannot open file 'PRIVATE.lib'\n")
        parser.feed(b"PRIVATE.obj : error LNK2019: unresolved PRIVATE\n")
        parser.feed(b"LINK : fatal error LNK11040: malformed\n")
        parser.feed(b"LINK : fatal error LNKABCD: malformed\n")
        parser.feed(b"LINK : fatal error LNK1105 malformed delimiter\n")
        hints = parser.finish()
        self.assertEqual(hints["failure_error_codes"], ["LNK1104", "LNK2019"])
        self.assertEqual(hints["failure_categories"], ["linker_error"])
        self.assertNotIn("PRIVATE", json.dumps(hints))
        for number in range(standalone.MAX_FAILURE_TESTS + 1):
            parser.feed(f"LINK : fatal error LNK{number:04d}: private body\n".encode())
        bounded = parser.finish()
        self.assertEqual(len(bounded["failure_error_codes"]), standalone.MAX_FAILURE_TESTS)
        self.assertTrue(bounded["failure_diagnostics_partial"])

    def test_native_failure_codes_accept_only_fixed_complete_json_fields(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b'  "code": "required_check_timed_')
        parser.feed(b'out",\r\n  "code": "native_check_failed"\n')
        parser.feed(b'"code": "PRIVATE"\n"code": "required_check_timed_out" PRIVATE\n')
        parser.feed(b'"code": "native_check_failed_extra"\n')
        parser.feed(b'\x1b[31m"code": "policy_unavailable"\n')
        parser.feed(b'"subject": "PRIVATE/path"\n')
        hints = parser.finish()
        self.assertEqual(hints["failure_native_codes"],
                         ["native_check_failed", "required_check_timed_out"])
        self.assertNotIn("PRIVATE", json.dumps(hints))
        self.assertNotIn("failure_tests", hints["failure_native_codes"])
        self.assertTrue(set(hints["failure_native_codes"]) <= standalone.NATIVE_FAILURE_CODES)

    def test_native_panic_locations_keep_known_files_and_mixed_platform_separators(self):
        parser = standalone.RustFailureDiagnostics()
        for index, prefix in enumerate(("", "src/integrations/git/../../../",
                                        r"src\integrations\git/../../../")):
            path = "tests/unit/integrations/git/native.rs"
            if index == 2:
                path = path.replace("/", "\\")
            parser.feed(f"thread 'native::case_{index}' (17) panicked at "
                        f"{prefix}{path}:251:5:\r\n".encode())
        hints = parser.finish()
        self.assertEqual(len(hints["failure_native_locations"]), 3)
        for item in hints["failure_native_locations"]:
            self.assertEqual(item["file"], "tests/unit/integrations/git/native.rs")
            self.assertEqual((item["line"], item["column"]), (251, 5))

    def test_native_locations_reject_private_unknown_or_malformed_paths(self):
        parser = standalone.RustFailureDiagnostics()
        for path in ("PRIVATE/tests/unit/integrations/git/native.rs",
                     "/tests/unit/integrations/git/native.rs",
                     "C:/PRIVATE/tests/unit/integrations/git/native.rs",
                     "tests/unit/integrations/git/PRIVATE.rs",
                     "tests/unit/integrations/git/../git/native.rs"):
            parser.feed(f"thread 'native::case' panicked at {path}:1:1:\n".encode())
        parser.feed(b"thread 'native::case' panicked at tests/unit/integrations/git/native.rs:0:1:\n")
        parser.feed(b"thread 'native::case' panicked at tests/unit/integrations/git/native.rs:1:1: PRIVATE\n")
        self.assertEqual(parser.finish()["failure_native_locations"], [])

    def test_native_locations_are_bounded_deduplicated_and_do_not_echo_messages(self):
        parser = standalone.RustFailureDiagnostics()
        for index in range(standalone.MAX_FAILURE_TESTS + 1):
            line = (f"thread 'native::case_{index}' panicked at "
                    "tests/unit/integrations/git/native.rs:251:5:\n").encode()
            parser.feed(line + line + b"PRIVATE panic body\n")
        hints = parser.finish()
        self.assertEqual(len(hints["failure_native_locations"]), standalone.MAX_FAILURE_TESTS)
        self.assertTrue(hints["failure_diagnostics_partial"])
        self.assertNotIn("PRIVATE", json.dumps(hints))

    def test_runtime_diagnostics_do_not_expose_command_or_host_paths(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b'failed to start command PRIVATE_COMMAND in PRIVATE_PATH\n')
        parser.feed(b'cargo contention gate remained busy for the bounded queue wait\n')
        parser.feed(b'sandbox_unavailable: PRIVATE_DETAILS\n')
        hints = parser.finish()
        self.assertEqual(hints["failure_categories"],
                         ["process_queue_timeout", "process_spawn", "sandbox_unavailable"])
        self.assertNotIn("PRIVATE", json.dumps(hints))

    def test_rust_failure_diagnostics_accept_only_complete_standard_lines(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b"test suite::one ... FAI")
        parser.feed(b"LED\r\nPRIVATE source/path/token\n"
                    b"test src/private.rs ... FAILED\n"
                    b"test unsafe token ... FAILED\n"
                    b"\x1b[31mtest suite::colored ... FAILED\n"
                    b"test result: FAILED. 2 passed; 1 failed; 0 ignored; "
                    b"0 measured; 0 filtered out; finished in 0.10s\n")
        hints = parser.finish()
        self.assertEqual(hints["failure_tests"], ["suite::one"])
        self.assertEqual(hints["failure_summaries"],
                         [{"passed": 2, "failed": 1, "ignored": 0,
                           "measured": 0, "filtered_out": 0}])
        self.assertNotIn("PRIVATE", json.dumps(hints))
        self.assertFalse(hints["failure_diagnostics_partial"])

    def test_panic_locations_accept_only_the_known_architecture_file(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b"thread 'suite::one' panicked at tests/architecture.rs:147:")
        parser.feed(b"5:\nPRIVATE panic body\n")
        parser.feed(b"thread 'suite::two' (1234) panicked at tests\\architecture.rs:9:17:\r\n")
        hints = parser.finish()
        self.assertEqual(hints["failure_locations"], [
            {"test": "suite::one", "line": 147, "column": 5},
            {"test": "suite::two", "line": 9, "column": 17},
        ])
        self.assertNotIn("PRIVATE", json.dumps(hints))

    def test_panic_locations_reject_bodies_other_paths_and_unsafe_identifiers(self):
        parser = standalone.RustFailureDiagnostics()
        lines = [
            "thread 'suite::one' panicked at tests/source_layout.rs:1:1:",
            "thread 'suite::one' panicked at /private/tests/architecture.rs:1:1:",
            "thread 'unsafe name' panicked at tests/architecture.rs:1:1:",
            "thread 'src/private.rs' panicked at tests/architecture.rs:1:1:",
            "thread 'suite::one' panicked at tests/architecture.rs:0:1:",
            "thread 'suite::one' panicked at tests/architecture.rs:1:0:",
            "thread 'suite::one' panicked at tests/architecture.rs:-1:1:",
            "thread 'suite::one' panicked at tests/architecture.rs:1:1: PRIVATE body",
            "thread 'suite::one' panicked at 'PRIVATE body', tests/architecture.rs:1:1",
        ]
        parser.feed(("\n".join(lines) + "\n").encode())
        self.assertEqual(parser.finish()["failure_locations"], [])

    def test_panic_location_collection_is_bounded_and_deduplicated(self):
        parser = standalone.RustFailureDiagnostics()
        line = b"thread 'suite::one' panicked at tests/architecture.rs:147:5:\n"
        parser.feed(line + line)
        self.assertEqual(len(parser.locations), 1)
        for index in range(standalone.MAX_FAILURE_TESTS + 1):
            parser.feed(f"thread 'suite::case_{index}' panicked at "
                        f"tests/architecture.rs:{index + 1}:1:\n".encode())
        hints = parser.finish()
        self.assertEqual(len(hints["failure_locations"]), standalone.MAX_FAILURE_TESTS)
        self.assertTrue(hints["failure_diagnostics_partial"])

    def test_rust_failure_diagnostics_bounds_names_lines_and_count(self):
        parser = standalone.RustFailureDiagnostics()
        for index in range(standalone.MAX_FAILURE_TESTS + 1):
            parser.feed(f"test suite::case_{index} ... FAILED\n".encode())
        parser.feed(("test " + "a" * 257 + " ... FAILED\n").encode())
        parser.feed(b"x" * (standalone.MAX_FAILURE_LINE_BYTES + 1))
        parser.feed(b"\ntest suite::last ... FAILED\n")
        hints = parser.finish()
        self.assertEqual(len(hints["failure_tests"]), standalone.MAX_FAILURE_TESTS)
        self.assertTrue(all(len(name) <= 256 for name in hints["failure_tests"]))
        self.assertTrue(hints["failure_diagnostics_partial"])

    def test_failed_child_emits_safe_ids_but_report_excludes_diagnostics(self):
        path = self.root / "report.json"
        report = standalone.StageReport(path)
        code = (
            "print('test suite::broken ... FAILED');"
            "print(\"thread 'suite::broken' (123) panicked at tests/architecture.rs:147:5:\");"
            "print('PRIVATE_UNSTRUCTURED_OUTPUT');"
            "print('test result: FAILED. 1 passed; 1 failed; 0 ignored; "
            "0 measured; 0 filtered out; finished in 0.10s');"
            "raise SystemExit(7)"
        )
        output = io.StringIO()
        with patch.object(standalone, "REPORT", report), contextlib.redirect_stdout(output):
            with self.assertRaises(standalone.StandaloneError):
                standalone.run_stage(
                    "rust_child", [sys.executable, "-c", code], self.root,
                    standalone.controlled_env(), 5, diagnostics=True)
            standalone.emit("oss_standalone", "failed")
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        failed = next(record for record in records
                      if record["stage"] == "rust_child" and record["status"] == "failed")
        self.assertEqual(failed["failure_tests"], ["suite::broken"])
        self.assertEqual(failed["failure_summaries"][0]["failed"], 1)
        self.assertEqual(failed["failure_locations"],
                         [{"test": "suite::broken", "line": 147, "column": 5}])
        self.assertNotIn("PRIVATE_UNSTRUCTURED_OUTPUT", output.getvalue())
        self.assertNotIn(b"suite::broken", path.read_bytes())
        self.assertNotIn(b"failure_tests", path.read_bytes())
        self.assertNotIn(b"failure_locations", path.read_bytes())
        self.assertEqual(json.loads(path.read_bytes())["status"], "failed")

    def test_failure_diagnostics_are_opt_in_and_never_emitted_for_success(self):
        for enabled, exit_code in ((False, 7), (True, 0)):
            output = io.StringIO()
            code = f"print('test suite::broken ... FAILED');raise SystemExit({exit_code})"
            with contextlib.redirect_stdout(output):
                if exit_code:
                    with self.assertRaises(standalone.StandaloneError):
                        standalone.run_stage(
                            "child", [sys.executable, "-c", code], self.root,
                            standalone.controlled_env(), 5, diagnostics=enabled)
                else:
                    standalone.run_stage(
                        "child", [sys.executable, "-c", code], self.root,
                        standalone.controlled_env(), 5, diagnostics=enabled)
            self.assertNotIn("failure_tests", output.getvalue())
            self.assertNotIn("suite::broken", output.getvalue())

    def metadata_tree(self, destination="unpacked"):
        entries = [
            (ROOT + "/src/lib.rs", b"pub fn sample() -> bool { true }\n", tarfile.REGTYPE),
            (ROOT + "/LICENSE", b"Apache-2.0\n", tarfile.REGTYPE),
            (ROOT + "/Cargo.toml.orig", b"generated original\n", tarfile.REGTYPE),
            (ROOT + "/.cargo_vcs_info.json", b'{"git":{"sha1":"fixture"}}\n', tarfile.REGTYPE),
        ]
        parent = self.root / destination
        tree, files = standalone.unpack_package(self.archive(entries), parent, ROOT)
        return parent, tree, files

    def test_generated_cleanup_keeps_all_real_sources_and_license_bytes(self):
        parent, tree, files = self.metadata_tree()
        original = {name: (tree / name).read_bytes() for name in files
                    if name not in standalone.GENERATED_FILES}
        self.assertEqual(standalone.remove_generated_metadata(tree, files, parent), 2)
        for name, content in original.items():
            self.assertEqual((tree / name).read_bytes(), content)
        for name in standalone.GENERATED_FILES:
            self.assertFalse((tree / name).exists())

    def test_generated_cleanup_rejects_identity_change_before_deleting_anything(self):
        parent, tree, files = self.metadata_tree()
        (tree / "Cargo.toml.orig").write_bytes(b"changed after validation")
        with self.assertRaises(standalone.StandaloneError):
            standalone.remove_generated_metadata(tree, files, parent)
        self.assertTrue((tree / ".cargo_vcs_info.json").exists())
        self.assertEqual((tree / "Cargo.toml.orig").read_bytes(), b"changed after validation")

    def test_generated_cleanup_cannot_touch_a_source_root_or_unverified_metadata(self):
        parent, tree, files = self.metadata_tree()
        with self.assertRaises(standalone.StandaloneError):
            standalone.remove_generated_metadata(tree, files, self.root)
        self.assertTrue((tree / "Cargo.toml.orig").exists())
        unverified = {name: value for name, value in files.items()
                      if name != "Cargo.toml.orig"}
        with self.assertRaises(standalone.StandaloneError):
            standalone.remove_generated_metadata(tree, unverified, parent)
        self.assertTrue((tree / ".cargo_vcs_info.json").exists())

    def test_generated_cleanup_rejects_a_metadata_symlink(self):
        parent, tree, files = self.metadata_tree()
        path = tree / "Cargo.toml.orig"
        path.unlink()
        self.source_symlink(path, tree / "LICENSE")
        with self.assertRaises(standalone.StandaloneError):
            standalone.remove_generated_metadata(tree, files, parent)
        self.assertTrue((tree / ".cargo_vcs_info.json").exists())
        self.assertEqual((tree / "LICENSE").read_bytes(), b"Apache-2.0\n")

    def test_reserved_metadata_cleanup_restores_real_cargo_inventory(self):
        # A real minimal Cargo/Git regression, not Evidence or a compile/acceptance mock.
        manifest = (
            '[package]\nname = "wcode"\nversion = "0.9.0"\nedition = "2021"\n'
            'license = "Apache-2.0"\n'
        ).encode()
        lock = b'version = 3\n\n[[package]]\nname = "wcode"\nversion = "0.9.0"\n'
        sources = [
            (ROOT + "/Cargo.toml", manifest, tarfile.REGTYPE),
            (ROOT + "/Cargo.lock", lock, tarfile.REGTYPE),
            (ROOT + "/src/lib.rs", b"pub fn sample() -> bool { true }\n", tarfile.REGTYPE),
        ]
        generated = [
            (ROOT + "/Cargo.toml.orig", manifest, tarfile.REGTYPE),
            (ROOT + "/.cargo_vcs_info.json", b'{"git":{"sha1":"fixture"}}\n', tarfile.REGTYPE),
        ]
        env = standalone.controlled_env()
        hooks = self.root / "empty-hooks"
        hooks.mkdir()
        git = ["git", "-c", "core.hooksPath=" + str(hooks),
               "-c", "core.autocrlf=false", "-c", "commit.gpgSign=false",
               "-c", "user.name=OSS fixture", "-c", "user.email=fixture@invalid"]
        package = ["cargo", "package", "--list", "--locked", "--offline", "--allow-dirty"]
        with contextlib.redirect_stdout(io.StringIO()):
            control, _ = standalone.unpack_package(
                self.archive(sources), self.root / "control", ROOT)
            bad, files = standalone.unpack_package(
                self.archive(sources + generated), self.root / "bad", ROOT)
            for tree in (control, bad):
                for command in (["init"], ["add", "--force", "--all"],
                                ["commit", "--no-verify", "-m", "Temporary fixture"]):
                    standalone.run_stage("fixture_git", git + command, tree, env, 30)
            control_list = standalone.run_stage(
                "fixture_control", package, control, env, 30, capture_output=True)
            self.assertIn(b"src/lib.rs", control_list)
            with self.assertRaises(standalone.StandaloneError):
                standalone.run_stage(
                    "fixture_reserved", package, bad, env, 30, capture_output=True)
            before = {name: (bad / name).read_bytes() for name in files
                      if name not in standalone.GENERATED_FILES}
            self.assertEqual(standalone.remove_generated_metadata(
                bad, files, self.root / "bad"), 2)
            for command in (["add", "--all"],
                            ["commit", "--no-verify", "-m", "Remove reserved generated metadata"]):
                standalone.run_stage("fixture_git", git + command, bad, env, 30)
            recovered_list = standalone.run_stage(
                "fixture_recovered", package, bad, env, 30, capture_output=True)
            self.assertIn(b"src/lib.rs", recovered_list)
            for name, content in before.items():
                self.assertEqual((bad / name).read_bytes(), content)
            for name in standalone.GENERATED_FILES:
                self.assertFalse((bad / name).exists())

    def metadata(self, **changes):
        manifest = self.root / "Cargo.toml"
        manifest.write_text("not parsed as TOML", encoding="utf-8")
        package = {"name": "wcode", "version": "0.9.0", "license": "Apache-2.0",
                   "manifest_path": str(manifest)}
        package.update(changes)
        return manifest, {"version": 1, "packages": [package]}

    def test_cargo_metadata_uses_exact_manifest_and_typed_package(self):
        manifest, data = self.metadata()
        data["packages"].insert(0, {"name": "foreign", "version": "1.0.0",
                                   "manifest_path": str(self.root / "other.toml")})
        package = standalone.parse_cargo_package(json.dumps(data).encode(), manifest)
        self.assertEqual(standalone.validate_package(package), ROOT)

    def test_cargo_metadata_rejects_missing_duplicate_foreign_and_bad_license(self):
        manifest, valid = self.metadata()
        bad = [
            {"version": 1, "packages": []},
            {"version": 1, "packages": valid["packages"] * 2},
            {"version": 1, "packages": [{**valid["packages"][0],
                                        "manifest_path": str(self.root / "foreign.toml")}]},
            {"version": 1, "packages": [{**valid["packages"][0], "license": "LicenseRef-private"}]},
            {"version": 1, "packages": [{**valid["packages"][0], "name": "enterprise"}]},
            {"version": 1, "packages": [{**valid["packages"][0], "version": "../escape"}]},
            {"version": 2, "packages": valid["packages"]},
            {"version": True, "packages": valid["packages"]},
            {"version": 1, "packages": [{"manifest_path": "Cargo.toml"}]},
        ]
        for data in bad:
            with self.subTest(data=data), self.assertRaises(standalone.StandaloneError):
                standalone.parse_cargo_package(json.dumps(data).encode(), manifest)

    def test_cargo_metadata_rejects_truncation_encoding_and_budget(self):
        manifest, _ = self.metadata()
        for payload in (b"{", b"\xff", b"null", b"x" * (standalone.MAX_METADATA_BYTES + 1)):
            with self.assertRaises(standalone.StandaloneError):
                standalone.parse_cargo_package(payload, manifest)

    def test_json_capture_separates_stderr_and_does_not_publish_raw_content(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            payload = standalone.run_stage(
                "json_child", [sys.executable, "-c",
                               "import sys;print('{\"version\":1}');"
                               "print('PRIVATE_STDERR',file=sys.stderr)"],
                self.root, standalone.controlled_env(), 5,
                capture_output=True)
        self.assertEqual(json.loads(payload), {"version": 1})
        self.assertNotIn("PRIVATE_STDERR", output.getvalue())
        self.assertNotIn('"version":1', output.getvalue())

    def test_report_atomically_records_final_failure_without_sensitive_details(self):
        path = self.root / "report.json"
        report = standalone.StageReport(path)
        report.record("build", "running", {})
        report.record("build", "failed", {"exit_code": 7, "seconds": 0.1,
                                         "output_bytes": 99, "output": "PRIVATE",
                                         "argv": "PRIVATE", "environment": "PRIVATE"})
        report.record("oss_standalone", "failed", {"reason": "PRIVATE"})
        payload = path.read_bytes()
        data = json.loads(payload)
        self.assertEqual(data["status"], "failed")
        self.assertIn("not_evidence", data["kind"])
        self.assertEqual(data["stages"][1]["exit_code"], 7)
        self.assertNotIn(b"PRIVATE", payload)
        self.assertLessEqual(len(payload), standalone.MAX_REPORT_BYTES)
        self.assertFalse(list(self.root.glob(".oss-report-*")))

    def test_report_never_overwrites_an_existing_unknown_run(self):
        path = self.root / "report.json"
        standalone.StageReport(path)
        original = path.read_bytes()
        with self.assertRaises(standalone.StandaloneError):
            standalone.StageReport(path)
        self.assertEqual(path.read_bytes(), original)

    def test_report_write_failure_cannot_publish_success(self):
        path = self.root / "report.json"
        report = standalone.StageReport(path)
        original = path.read_bytes()
        with patch.object(standalone.os, "replace", side_effect=OSError("disk failure")):
            with self.assertRaises(standalone.StandaloneError):
                report.record("oss_standalone", "passed", {})
        self.assertEqual(path.read_bytes(), original)
        self.assertEqual(json.loads(path.read_bytes())["status"], "running")
        self.assertFalse(list(self.root.glob(".oss-report-*")))

    def test_report_external_change_and_event_bound_fail_closed(self):
        path = self.root / "report.json"
        report = standalone.StageReport(path)
        path.write_bytes(b'{"status":"other-run"}')
        with self.assertRaises(standalone.StandaloneError):
            report.record("build", "passed", {})
        self.assertEqual(path.read_bytes(), b'{"status":"other-run"}')
        bounded = standalone.StageReport(self.root / "bounded.json")
        with patch.object(standalone, "MAX_REPORT_EVENTS", 1):
            bounded.record("build", "running", {})
            with self.assertRaises(standalone.StandaloneError):
                bounded.record("build", "passed", {})
        self.assertEqual(json.loads((self.root / "bounded.json").read_bytes())["status"], "running")

    def test_input_failure_is_reported_without_running_cargo(self):
        path = self.root / "target" / "report.json"
        argv = ["standalone.py", "--workspace", str(self.root / "absent"),
                "--report", str(path)]
        with patch.object(sys, "argv", argv), patch.object(
                standalone, "__file__", str(self.root / "tests" / "standalone.py")):
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(standalone.main(), 1)
        data = json.loads(path.read_bytes())
        self.assertEqual(data["status"], "failed")
        self.assertEqual(data["stages"][-1]["stage"], "oss_standalone")
        self.assertFalse(list(self.root.glob(".oss-report-*")))

    def test_subprocess_failure_does_not_print_private_output_or_claim_success(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            with self.assertRaises(standalone.StandaloneError):
                standalone.run_stage(
                    "negative_child", [sys.executable, "-c",
                                       "print('PRIVATE_SENTINEL');raise SystemExit(7)"],
                    self.root, standalone.controlled_env(), 5)
        self.assertNotIn("PRIVATE_SENTINEL", output.getvalue())
        result = json.loads(output.getvalue().splitlines()[-1])
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["exit_code"], 7)

    def test_subprocess_timeout_and_output_overflow_are_failures(self):
        commands = [("import time;time.sleep(30)", 0.15, 1024),
                    ("import sys;sys.stdout.write('x'*1000000)", 5, 128)]
        for code, timeout, limit in commands:
            with self.subTest(timeout=timeout), contextlib.redirect_stdout(io.StringIO()):
                started = time.monotonic()
                with self.assertRaises(standalone.StandaloneError):
                    standalone.run_stage(
                        "bounded_child", [sys.executable, "-c", code], self.root,
                        standalone.controlled_env(), timeout, limit)
                self.assertLess(time.monotonic() - started, 15)

    def test_nested_package_inventory_allows_only_explicit_generated_lock(self):
        package = self.root / "core"
        (package / "src").mkdir(parents=True)
        (package / "Cargo.toml").write_bytes(b"[package]\nname='core'\nversion='1.0.0'\n")
        (package / "src/lib.rs").write_bytes(b"pub struct Fact;\n")
        payload = b"Cargo.toml\nCargo.lock\nCargo.toml.orig\nsrc/lib.rs\n"
        with self.assertRaises(standalone.StandaloneError):
            standalone.validate_package_sources(package, payload, "core-1.0.0")
        checked = standalone.validate_package_sources(
            package, payload, "core-1.0.0",
            standalone.GENERATED_FILES | {"Cargo.lock"})
        self.assertEqual(
            checked, {"Cargo.toml", "Cargo.lock", "Cargo.toml.orig", "src/lib.rs"})

    def test_workspace_archive_combines_packages_and_round_trips_exact_bytes(self):
        main = self.root / "main"
        core = self.root / "core"
        (main / "src").mkdir(parents=True)
        (core / "src").mkdir(parents=True)
        (main / "Cargo.toml").write_bytes(b"main manifest\n")
        (main / "src/lib.rs").write_bytes(b"pub fn root() {}\n")
        (core / "Cargo.toml").write_bytes(b"core manifest\n")
        (core / "src/lib.rs").write_bytes(b"pub struct Binding;\n")
        archive = self.root / "workspace.tar.gz"
        expected = standalone.write_workspace_archive(
            archive,
            "wcode-workspace-0.9.0",
            [
                (main, "", {"Cargo.toml", "src/lib.rs"}),
                (core, "crates/core-types", {"Cargo.toml", "src/lib.rs"}),
            ],
        )
        tree, files = standalone.unpack_package(
            archive, self.root / "workspace-out", "wcode-workspace-0.9.0")
        self.assertEqual(files, expected)
        self.assertEqual(
            (tree / "crates/core-types/src/lib.rs").read_bytes(),
            b"pub struct Binding;\n")
        self.assertFalse((tree / "commercial").exists())

    def test_workspace_archive_rejects_symlink_and_portable_overlap(self):
        main = self.root / "main"
        core = self.root / "core"
        main.mkdir()
        core.mkdir()
        (main / "a").write_bytes(b"a")
        (core / "A").write_bytes(b"b")
        with self.assertRaises(standalone.StandaloneError):
            standalone.write_workspace_archive(
                self.root / "dupe.tar.gz", "bundle",
                [(main, "", {"a"}), (core, "", {"A"})])
        with tempfile.TemporaryDirectory(prefix="wcode-external-fixture-") as outside:
            external = Path(outside) / "outside"
            external.write_bytes(b"outside")
            self.source_symlink(main / "link", external)
            with self.assertRaises(standalone.StandaloneError):
                standalone.write_workspace_archive(
                    self.root / "link.tar.gz", "bundle",
                    [(main, "", {"link"})])

    def test_validate_package_accepts_only_the_requested_oss_identity(self):
        core = {"name": "wcode-core-types", "version": "0.9.0", "license": "Apache-2.0"}
        self.assertEqual(
            standalone.validate_package(core, "wcode-core-types"),
            "wcode-core-types-0.9.0")
        with self.assertRaises(standalone.StandaloneError):
            standalone.validate_package(core, "wcode")


if __name__ == "__main__":
    unittest.main()
