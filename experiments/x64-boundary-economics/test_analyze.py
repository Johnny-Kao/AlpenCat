#!/usr/bin/env python3
import csv
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ANALYZER = ROOT / "experiments/x64-boundary-economics/analyze.py"

FIELDS = ["phase", "n", "serial_ns", "parallel_ns", "winner", "rayon_threads"]


def write_phase(root, phase, rows):
    path = root / f"{phase}.csv"
    with path.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=FIELDS)
        w.writeheader()
        for n, serial_ns, parallel_ns in rows:
            w.writerow({
                "phase": phase,
                "n": n,
                "serial_ns": serial_ns,
                "parallel_ns": parallel_ns,
                "winner": "SERIAL" if serial_ns <= parallel_ns else "PARALLEL",
                "rayon_threads": 1,
            })


class AnalyzeRegressionTests(unittest.TestCase):
    def run_analyzer(self, phases):
        with tempfile.TemporaryDirectory() as td:
            root = pathlib.Path(td)
            for name, rows in phases.items():
                write_phase(root, name, rows)
            proc = subprocess.run(
                [sys.executable, str(ANALYZER), str(root)],
                text=True,
                capture_output=True,
                check=False,
            )
            summary = None
            summary_path = root / "summary.json"
            if summary_path.exists():
                summary = json.loads(summary_path.read_text())
            return proc, summary

    def test_no_parallel_baseline_renders_without_keyerror(self):
        proc, summary = self.run_analyzer({
            "full": [(8, 10, 20), (16, 20, 30), (32, 40, 50)],
            "one": [(8, 10, 25), (16, 20, 35), (32, 40, 60)],
        })
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("| full |", proc.stdout)
        self.assertIn("| n/a | no_baseline_boundary |", proc.stdout)
        self.assertIsNone(summary["baseline_boundary"])

    def test_break_even_uses_near_boundary_loss_not_grid_mean(self):
        proc, summary = self.run_analyzer({
            "full": [(8, 10, 20), (16, 20, 10), (32, 40, 20)],
            "half": [(8, 10, 20), (16, 20, 30), (32, 40, 20)],
        })
        self.assertEqual(proc.returncode, 0, proc.stderr)
        gate = summary["phases"]["half"]["economic_gate"]
        self.assertEqual(gate["near_boundary_stale_loss_ns_per_call"], 10.0)
        self.assertEqual(gate["near_boundary_post_revalidation_loss_ns_per_call"], 0.0)
        self.assertAlmostEqual(gate["break_even_calls_after_event"], 11.0)


if __name__ == "__main__":
    unittest.main()
