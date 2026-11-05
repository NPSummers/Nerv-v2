import importlib.util
from contextlib import redirect_stderr
import io
from pathlib import Path
import unittest
from unittest.mock import patch
import subprocess
import sys

spec = importlib.util.spec_from_file_location("compare", Path(__file__).with_name("compare-binaries.py"))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)


class CompareTests(unittest.TestCase):
    def test_summary_uses_median_and_keeps_samples(self):
        result = compare.stats([1.0, 100.0, 2.0])
        self.assertEqual(result["median_ms"], 2.0)
        self.assertEqual(result["samples_ms"], [1.0, 100.0, 2.0])

    def test_heavy_results_and_void_main_status(self):
        compare.validate_heavy("Nerv", "1049940\n0\n")
        compare.validate_heavy("Rust", "1059939\n")
        for label, output in [("Nerv", "1049940\n"), ("Nerv", "1049940\n1\n"),
                              ("Rust", "0\n"), ("Rust", "1059940\n"), ("Rust", "1049940\n0\n")]:
            with self.subTest(label=label, output=output), self.assertRaises(ValueError):
                compare.validate_heavy(label, output)

    def test_times_program_with_no_jit_or_source_arguments(self):
        fake = subprocess.CompletedProcess([], 0, b"1049940\n0\n", b"")
        with patch.object(compare.subprocess, "run", return_value=fake) as run:
            compare.run_once("Nerv", Path("standalone-app"), {}, heavy=True)
        self.assertEqual(run.call_args.args[0], ["standalone-app"])

    def test_process_failures_are_not_timing_results(self):
        with patch.object(compare.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, b"", b"failure")):
            with self.assertRaises(RuntimeError):
                compare.run_once("Nerv", Path("standalone-app"), {})

    def test_nonempty_stderr_is_not_silently_ignored(self):
        with patch.object(compare.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, b"42\n", b"warning")):
            with self.assertRaises(RuntimeError):
                compare.run_once("Nerv", Path("standalone-app"), {})

    def test_report_cannot_overwrite_the_timed_executable(self):
        with patch.object(sys, "argv", ["compare", "--nerv-exe", sys.executable, "--json-output", sys.executable]), redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                compare.main()
        self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
