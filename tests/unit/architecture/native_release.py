#!/usr/bin/env python3
"""Release-canary protocol and completeness regressions; no imported PASS proof."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
SPEC = importlib.util.spec_from_file_location("native_release", ROOT / "native_release.py")
assert SPEC is not None and SPEC.loader is not None
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class ReleaseCanaryContract(unittest.TestCase):
    def report(self):
        return {
            "passed": True, "level": "full", "checks_run": 6,
            "checks_failed": 0, "checks_reused": 0, "skipped_checks": [],
            "checks": [dict(id=name, command=command, success=True,
                            exit_code=0, reused=False)
                       for name, command in runner.REQUIRED_CHECKS.items()],
        }

    def test_complete_native_full_report_is_accepted_without_fabricating_acceptance(self):
        report = self.report()
        result = runner.require_full_report(report)
        self.assertEqual(result, report)
        self.assertNotIn("current_acceptance", result)

    def test_partial_reused_wrong_command_or_missing_gates_are_rejected(self):
        changes = [
            ("passed", False), ("level", "quick"), ("checks_run", 5),
            ("checks_failed", 1), ("checks_reused", 1),
            ("skipped_checks", ["rust-release-build"]),
        ]
        for key, value in changes:
            with self.subTest(key=key), self.assertRaises(runner.CanaryError):
                report = self.report()
                report[key] = value
                runner.require_full_report(report)
        for key, value in [("command", "cargo test one_test"), ("exit_code", None),
                           ("success", False), ("reused", True), ("exit_code", False)]:
            with self.subTest(key=key), self.assertRaises(runner.CanaryError):
                report = self.report()
                report["checks"][3][key] = value
                runner.require_full_report(report)
        for rows in [[], self.report()["checks"][:-1], self.report()["checks"] * 2]:
            with self.assertRaises(runner.CanaryError):
                report = self.report()
                report["checks"] = rows
                runner.require_full_report(report)

    def test_protocol_errors_and_text_only_output_never_become_a_result(self):
        for response in [None, [], {}, {"error": {"message": "remote diagnostic"}},
                         {"result": {"content": [{"text": "PASS"}]}},
                         {"result": {"isError": True, "structuredContent": {"passed": True}}}]:
            with self.subTest(response=response), self.assertRaises(runner.CanaryError):
                runner.structured(response)
        self.assertEqual(runner.structured({"result": {
            "isError": False, "structuredContent": {"kind": "verification_task"}}}),
            {"kind": "verification_task"})

    def test_completed_failed_or_wrong_task_receipts_do_not_pass(self):
        receipt = {"kind": "verification_task", "task_id": "TASK-fixture",
                   "tool": "verify_project", "workspace": "repo", "status": "completed",
                   "terminal": True, "result_available": True,
                   "result": {"isError": False, "structuredContent": self.report()}}
        self.assertEqual(runner.completed_report(receipt, "TASK-fixture", "repo"), self.report())
        for key, value in [("task_id", "TASK-other"), ("workspace", "other"),
                           ("tool", "run_command"), ("kind", "command_task"),
                           ("status", "failed"), ("terminal", False),
                           ("result_available", False)]:
            with self.subTest(key=key), self.assertRaises(runner.CanaryError):
                runner.completed_report({**receipt, key: value}, "TASK-fixture", "repo")
        with self.assertRaises(runner.CanaryError):
            runner.completed_report({**receipt, "result": {
                "isError": True, "structuredContent": self.report()}}, "TASK-fixture", "repo")


if __name__ == "__main__":
    unittest.main()
