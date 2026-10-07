#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "batch", HERE / "analyze_universal_gate_batch.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class UniversalGateBatchTests(unittest.TestCase):
    def row(self, cpus=4, regime="cpu-pressure", recoverable=1):
        return {
            "logical_cpus": str(cpus),
            "regime": regime,
            "recoverable_ns": str(recoverable),
        }

    def test_v1_skips_single_cpu(self):
        self.assertFalse(M.v1(self.row(cpus=1)))

    def test_v1_skips_baseline(self):
        self.assertFalse(M.v1(self.row(regime="baseline-full")))

    def test_v1_keeps_multicore_transition(self):
        self.assertTrue(M.v1(self.row()))


if __name__ == "__main__":
    unittest.main()
