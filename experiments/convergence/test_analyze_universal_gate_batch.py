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

    def test_economic_slowdown_requires_exposure_above_last_known_cost(self):
        rows = [
            {
                "machine_id": "m",
                "workload": "w",
                "logical_cpus": "4",
                "regime": "baseline-full",
                "revalidation_cost_ns": 100.0,
                "natural_slowdown_exposure_ns": 0.0,
            },
            {
                "machine_id": "m",
                "workload": "w",
                "logical_cpus": "4",
                "regime": "cpu-pressure",
                "revalidation_cost_ns": 120.0,
                "natural_slowdown_exposure_ns": 90.0,
            },
            {
                "machine_id": "m",
                "workload": "w",
                "logical_cpus": "4",
                "regime": "memory-light",
                "revalidation_cost_ns": 130.0,
                "natural_slowdown_exposure_ns": 110.0,
            },
        ]
        self.assertEqual(M.economic_slowdown_mask(rows), {2})


if __name__ == "__main__":
    unittest.main()
