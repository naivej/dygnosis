"""Regression checks for measurement evidence that must not count as a pass.

Run: python -m unittest discover -s scripts -p test_measure_project_lsp.py
"""
import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import measure_project_lsp
from measure_project_lsp import verify_include, verify_project, verify_sample, verify_stopped


class MeasurementEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.roots = ["file:///models/a.mod", "file:///models/b.mod"]
        self.include = "file:///models/shared.inc"
        self.status = {
            "complete": True, "cancelled": False, "enabled": True, "discovery": "complete",
            "discovery_failures": [], "pass_revision": 3,
            "counts": {"checked": 1, "incomplete": 1, "failed": 0, "excluded": 0,
                       "pending": 0, "checking": 0},
            "metrics": {"completed_jobs": 2},
            "roots": [{"root_uri": root, "state": state, "revision": f"old-{index}",
                       "dependency_candidates": [self.include]}
                      for index, (root, state) in enumerate(zip(self.roots, ("checked", "incomplete")))],
        }
        self.reports = [{"uri": root, "items": []} for root in self.roots]

    def test_incomplete_root_is_reported_without_being_omitted(self):
        result = verify_project(self.status, self.reports, self.roots)
        self.assertEqual(result["incomplete_roots"], [self.roots[1]])

    def test_complete_flag_does_not_hide_an_omitted_or_duplicate_root(self):
        for replacement in ([self.status["roots"][0]], [self.status["roots"][0]] * 2):
            status = copy.deepcopy(self.status)
            status["roots"] = replacement
            with self.subTest(roots=replacement), self.assertRaisesRegex(RuntimeError, "root set"):
                verify_project(status, self.reports, self.roots)

    def test_status_without_a_workspace_report_fails(self):
        with self.assertRaisesRegex(RuntimeError, "omitted"):
            verify_project(self.status, self.reports[:1], self.roots)

    def test_complete_flag_does_not_hide_discovery_or_analysis_failure(self):
        for field, value in (("discovery_failures", ["unreadable"]), ("counts", {"checked": 2})):
            status = copy.deepcopy(self.status)
            status[field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                verify_project(status, self.reports, self.roots)
        status = copy.deepcopy(self.status)
        status["roots"][1]["state"] = "failed"
        with self.assertRaisesRegex(RuntimeError, "terminal"):
            verify_project(status, self.reports, self.roots)

    def test_include_pass_cannot_leave_one_owner_at_an_old_revision(self):
        after = copy.deepcopy(self.status)
        after["pass_revision"] += 1
        after["roots"][0]["revision"] = "new-a"
        with self.assertRaisesRegex(RuntimeError, "obsolete owner"):
            verify_include(self.status, after, self.include)
        after["roots"][1]["revision"] = "new-b"
        self.assertEqual(verify_include(self.status, after, self.include), self.roots)

    def test_optional_or_unshared_include_cannot_make_a_release_pass(self):
        with self.assertRaisesRegex(RuntimeError, "at least two"):
            verify_include(self.status, self.status, "file:///models/unused.inc")

    def test_empty_withdrawal_is_not_a_diagnostic_latency_sample(self):
        initial = [{"code": "E001", "message": "syntax error", "range": {"start": 0}, "data": {"revision": "old"}}]
        hover = {"contents": "Endogenous y"}
        with self.assertRaisesRegex(RuntimeError, "differs from the current"):
            verify_sample({"version": 2, "diagnostics": []}, initial, initial, 2, hover, hover)
        with self.assertRaisesRegex(RuntimeError, "withdrew"):
            verify_sample({"version": 2, "diagnostics": []}, [], initial, 2, hover, hover)
        current = copy.deepcopy(initial)
        current[0]["data"]["revision"] = "new"
        verify_sample({"version": 2, "diagnostics": current}, current, initial, 2, hover, hover)

    def test_stale_version_or_empty_hover_cannot_make_a_latency_sample(self):
        hover = {"contents": "Endogenous y"}
        with self.assertRaisesRegex(RuntimeError, "obsolete version"):
            verify_sample({"version": 1, "diagnostics": []}, [], [], 2, hover, hover)
        with self.assertRaisesRegex(RuntimeError, "declaration information"):
            verify_sample({"version": 2, "diagnostics": []}, [], [], 2, None, hover)

    def test_cancel_acknowledgement_does_not_hide_a_late_result(self):
        stopped = copy.deepcopy(self.status)
        stopped.update(cancelled=True, complete=False)
        late = copy.deepcopy(stopped)
        late["metrics"]["completed_jobs"] += 1
        with self.assertRaisesRegex(RuntimeError, "later result"):
            verify_stopped(stopped, late, False)
        verify_stopped(stopped, stopped, False)

    def test_disabled_setting_does_not_hide_retained_roots_or_background_work(self):
        off = copy.deepcopy(self.status)
        off.update(enabled=False, discovery="disabled", roots=[])
        with self.assertRaisesRegex(RuntimeError, "continued background"):
            verify_stopped(off, off, True)
        off["metrics"]["completed_jobs"] = 0
        verify_stopped(off, off, True)

    def test_protocol_failure_replaces_an_earlier_pass_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / "a.mod").write_text("var y; model; y=0; end;")
            binary = folder / "binary.exe"
            binary.write_bytes(b"probe fixture")
            output = folder / "result.json"
            output.write_text('{"passed": true}')
            argv = ["measure_project_lsp.py", "--binary", str(binary), "--folder", str(folder),
                    "--include", str(folder / "shared.inc"), "--expected-roots", "1", "--output", str(output)]
            with patch("sys.argv", argv), patch.object(measure_project_lsp, "run", side_effect=RuntimeError("omitted root")):
                with self.assertRaisesRegex(RuntimeError, "omitted root"):
                    measure_project_lsp.main()
            failed = json.loads(output.read_text())
            self.assertFalse(failed["passed"])
            self.assertEqual(failed["failure"], "RuntimeError: omitted root")

    def test_preflight_failure_replaces_an_earlier_pass_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / "a.mod").write_text("var y; model; y=0; end;")
            binary = folder / "binary.exe"
            binary.write_bytes(b"probe fixture")
            output = folder / "result.json"
            base = ["measure_project_lsp.py", "--binary", str(binary), "--folder", str(folder),
                    "--include", str(folder / "shared.inc"), "--output", str(output)]
            cases = (
                (["--expected-roots", "2"], ValueError, "Expected 2 roots"),
                (["--expected-roots", "1", "--samples", "19"], ValueError, "at least 20"),
                (["--expected-roots", "1", "--active", str(folder / "absent.mod")], ValueError, "Active model"),
                (["--expected-roots", "1", "--binary", str(folder / "missing.exe")], FileNotFoundError, "missing.exe"),
            )
            for extra, error, message in cases:
                with self.subTest(arguments=extra):
                    output.write_text('{"passed": true}')
                    with patch("sys.argv", base + extra), patch.object(measure_project_lsp, "run") as run:
                        with self.assertRaisesRegex(error, message):
                            measure_project_lsp.main()
                    run.assert_not_called()
                    failed = json.loads(output.read_text())
                    self.assertFalse(failed["passed"])
                    self.assertEqual(failed["status"], "failed")
                    self.assertIn(message, failed["failure"])


if __name__ == "__main__":
    unittest.main()
