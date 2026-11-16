#!/usr/bin/env python3
"""Diagnostic bounding and redaction regressions for the OSS verifier."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

from standalone_case import StandaloneCase

SCRIPT = Path(__file__).resolve().parents[2] / "standalone.py"
SPEC = importlib.util.spec_from_file_location("oss_standalone", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
standalone = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(standalone)


class DiagnosticsSafety(StandaloneCase):
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

    def test_verification_turn_locations_are_fixed_and_never_echo_panic_bodies(self):
        parser = standalone.RustFailureDiagnostics()
        fixed = "tests/unit/runtime/harness/verification_turn_native.rs"
        for index, path in enumerate((fixed, "src/runtime/harness/../../../" +
                                      fixed.replace("/", "\\"))):
            parser.feed(f"thread 'phase_fixture::case_{index}' (17) panicked at "
                        f"{path}:142:5:\r\n".encode())
            parser.feed(b"PRIVATE_PANIC_BODY: fixture report and credentials\n")
        for path in ("/" + fixed, "C:/PRIVATE/" + fixed, "PRIVATE/" + fixed,
                     fixed.replace("verification_turn_native.rs", "PRIVATE.rs"),
                     fixed.replace("verification_turn_native.rs",
                                   "../harness/verification_turn_native.rs")):
            parser.feed(f"thread 'phase_fixture::hidden' panicked at {path}:1:1:\n".encode())
        parser.feed(f"thread 'phase_fixture::hidden' panicked at {fixed}:1:1: PRIVATE\n".encode())
        hints = parser.finish()
        self.assertEqual(hints["failure_native_locations"], [
            {"test": f"phase_fixture::case_{index}", "file": fixed, "line": 142, "column": 5}
            for index in range(2)
        ])
        self.assertNotIn("PRIVATE", json.dumps(hints))
        self.assertEqual(hints["failure_tests"], [])
        self.assertEqual(hints["failure_native_codes"], [])

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

    def test_store_and_watch_liveness_failures_keep_only_known_source_locations(self):
        parser = standalone.RustFailureDiagnostics()
        for index, path in enumerate(("tests/unit/evidence/failure_memory.rs",
                                      "tests/unit/integrations/git/watch.rs")):
            parser.feed(f"thread 'native::case_{index}' panicked at src/../../{path}:31:5:\n"
                        "PRIVATE assertion body containing arbitrary runtime data\n".encode())
        hints = parser.finish()
        self.assertEqual(len(hints["failure_native_locations"]), 2)
        self.assertNotIn("PRIVATE", json.dumps(hints))

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

    def test_fanout_call_locations_accept_exact_portable_paths_and_chunked_lines(self):
        cases = [
            ("tests/unit/integrations/mcp/calls.rs",
             "mcp::tool_tests::parallel_fanout_runs_writes_and_coalesces_same_file_edits"),
            ("tests/oss_payload.rs",
             "source_payload_excludes_python_bytecode_but_keeps_adjacent_sources"),
        ]
        for fixed, name in cases:
            paths = (fixed, fixed.replace("/", "\\"),
                     "src/integrations/../../" + fixed)
            for path in paths:
                for ending in ("\n", "\r\n"):
                    for chunk_size in (1, 7, 4096):
                        with self.subTest(path=path, ending=ending, chunk_size=chunk_size):
                            parser = standalone.RustFailureDiagnostics()
                            text = (f"thread '{name}' (17) panicked at {path}:552:5:"
                                    + ending + "PRIVATE_ASSERTION_BODY token/path" + ending).encode()
                            for offset in range(0, len(text), chunk_size):
                                parser.feed(text[offset:offset + chunk_size])
                            hints = parser.finish()
                            self.assertEqual(hints["failure_native_locations"], [
                                {"test": name, "file": fixed, "line": 552, "column": 5},
                            ])
                            self.assertNotIn("PRIVATE", json.dumps(hints))
                            self.assertEqual(hints["failure_native_codes"], [])
                            self.assertFalse(hints["failure_diagnostics_partial"])

    def test_fanout_call_locations_reject_unknown_paths_bodies_and_injected_headers(self):
        fixed = "tests/unit/integrations/mcp/calls.rs"
        paths = ("PRIVATE/" + fixed, "/" + fixed, "C:/PRIVATE/" + fixed,
                 "tests/unit/integrations/mcp/PRIVATE.rs", fixed + ".PRIVATE",
                 "tests/unit/integrations/mcp/../mcp/calls.rs",
                 "tests/PRIVATE.rs", "tests/subdir/oss_payload.rs",
                 "tests/oss_payload_PRIVATE.rs", "tests/../oss_payload.rs",
                 "src/../tests/PRIVATE.rs",
                 "src/" + "../" * 13 + fixed, "tests/unit/integrations/mcp/calls\x00.rs")
        lines = [f"thread 'native::case' panicked at {path}:552:5:" for path in paths]
        lines += [
            f"thread 'unsafe name' panicked at {fixed}:552:5:",
            f"thread 'PRIVATE/source.rs' panicked at {fixed}:552:5:",
            f"thread '{'x' * 257}' panicked at {fixed}:552:5:",
            f"thread 'native::case' panicked at {fixed}:0:5:",
            f"thread 'native::case' panicked at {fixed}:552:0:",
            f"thread 'native::case' panicked at {fixed}:1000000000:5:",
            f"thread 'native::case' panicked at {fixed}:552:5: PRIVATE",
            f"\x1b[31mthread 'native::case' panicked at {fixed}:552:5:",
            f"thread 'native::case' panicked at {fixed}:552:5:\x00",
            f"thread 'native::case' panicked at tests/unit/integrations/mcp/callé.rs:552:5:",
        ]
        parser = standalone.RustFailureDiagnostics()
        parser.feed(("\r\n".join(lines) + "\r\n").encode())
        hints = parser.finish()
        self.assertEqual(hints["failure_native_locations"], [])
        self.assertNotIn("PRIVATE", json.dumps(hints))

    def test_failed_fanout_child_emits_only_fixed_location_and_no_report_diagnostics(self):
        path = self.root / "fanout-report.json"
        report = standalone.StageReport(path)
        fixed = "tests/unit/integrations/mcp/calls.rs"
        name = "mcp::tool_tests::parallel_fanout_runs_writes_and_coalesces_same_file_edits"
        header = f"thread '{name}' (17) panicked at {fixed}:552:5:"
        code = (f"print({header!r});print('PRIVATE_ASSERTION_BODY credential/path');"
                f"print('test {name} ... FAILED');raise SystemExit(7)")
        output = io.StringIO()
        with patch.object(standalone, "REPORT", report), contextlib.redirect_stdout(output):
            with self.assertRaises(standalone.StandaloneError):
                standalone.run_stage(
                    "rust_child", [sys.executable, "-c", code], self.root,
                    standalone.controlled_env(), 5, diagnostics=True)
            standalone.emit("oss_standalone", "failed")
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        failed = next(row for row in records if row["stage"] == "rust_child"
                      and row["status"] == "failed")
        self.assertEqual(failed["failure_native_locations"], [
            {"test": name, "file": fixed, "line": 552, "column": 5},
        ])
        self.assertEqual(failed["failure_tests"], [name])
        self.assertEqual(failed["exit_code"], 7)
        self.assertNotIn("PRIVATE", output.getvalue())
        self.assertNotIn(b"failure_native_locations", path.read_bytes())
        self.assertNotIn(name.encode(), path.read_bytes())

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

    def test_panic_locations_accept_every_repo_relative_test_file(self):
        parser = standalone.RustFailureDiagnostics()
        parser.feed(b"thread 'suite::one' panicked at tests/architecture.rs:147:")
        parser.feed(b"5:\nPRIVATE panic body\n")
        parser.feed(b"thread 'suite::two' (1234) panicked at tests\\architecture.rs:9:17:\r\n")
        parser.feed(b"thread 'suite::three' panicked at tests/source_layout.rs:12:3:\n")
        hints = parser.finish()
        self.assertEqual(hints["failure_locations"], [
            {"test": "suite::one", "file": "architecture.rs", "line": 147, "column": 5},
            {"test": "suite::three", "file": "source_layout.rs", "line": 12, "column": 3},
            {"test": "suite::two", "file": "architecture.rs", "line": 9, "column": 17},
        ])
        self.assertNotIn("PRIVATE", json.dumps(hints))

    def test_panic_locations_reject_absolute_paths_and_unsafe_identifiers(self):
        parser = standalone.RustFailureDiagnostics()
        lines = [
            "thread 'suite::one' panicked at tests/unit/ui/PRIVATE.rs:1:1:",
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
                         [{"test": "suite::broken", "file": "architecture.rs",
                           "line": 147, "column": 5}])
        self.assertNotIn("PRIVATE_UNSTRUCTURED_OUTPUT", output.getvalue())
        self.assertNotIn(b"suite::broken", path.read_bytes())
        self.assertNotIn(b"failure_tests", path.read_bytes())
        self.assertNotIn(b"failure_locations", path.read_bytes())
        self.assertEqual(json.loads(path.read_bytes())["status"], "failed")

    def test_failed_child_retains_bounded_output_in_a_file_not_the_log(self):
        root = self.root / "failure-log"
        root.mkdir()
        log = root / "failure.log"
        path = self.root / "report.json"
        report = standalone.StageReport(path)
        code = (
            "print('test suite::broken ... FAILED');"
            "print('PRIVATE_BODY_MARKER');"
            "raise SystemExit(7)"
        )
        output = io.StringIO()
        with patch.object(standalone, "REPORT", report), \
                patch.object(standalone, "FAILURE_LOG", str(log)), \
                contextlib.redirect_stdout(output):
            with self.assertRaises(standalone.StandaloneError):
                standalone.run_stage(
                    "rust_child", [sys.executable, "-c", code], self.root,
                    standalone.controlled_env(), 5, capture_output=True,
                    diagnostics=True)
        retained = log.read_bytes()
        self.assertIn(b"PRIVATE_BODY_MARKER", retained)
        self.assertNotIn("PRIVATE_BODY_MARKER", output.getvalue())

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


if __name__ == "__main__":
    unittest.main()
