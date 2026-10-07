#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "gate", HERE / "analyze_universal_eligibility_gate.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class UniversalEligibilityGateTests(unittest.TestCase):
    def row(self, cpus=4, regime="cpu-pressure", recoverable=1):
        return {
            "logical_cpus": str(cpus),
            "regime": regime,
            "recoverable_ns": str(recoverable),
        }

    def test_single_cpu_is_ineligible(self):
        fn = lambda row: M.as_int(row, "logical_cpus") > 1 and row["regime"] != "baseline-full"
        self.assertFalse(fn(self.row(cpus=1)))

    def test_baseline_is_ineligible(self):
        fn = lambda row: M.as_int(row, "logical_cpus") > 1 and row["regime"] != "baseline-full"
        self.assertFalse(fn(self.row(regime="baseline-full")))

    def test_multicore_transition_is_eligible(self):
        fn = lambda row: M.as_int(row, "logical_cpus") > 1 and row["regime"] != "baseline-full"
        self.assertTrue(fn(self.row()))


if __name__ == "__main__":
    unittest.main()
