#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "opp", HERE / "analyze_opportunistic_evidence.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class OpportunisticEvidenceTests(unittest.TestCase):
    def point(self):
        return {
            "weight": 1.0,
            "serial_samples_ns": [10, 10, 10],
            "cpu_samples_ns": [20, 20, 20],
        }

    def regime(self):
        return [
            self.point(),
            {"weight": 1.0, "serial_samples_ns": [1], "cpu_samples_ns": [1]},
            {"weight": 1.0, "serial_samples_ns": [1], "cpu_samples_ns": [1]},
            {"weight": 1.0, "serial_samples_ns": [1], "cpu_samples_ns": [1]},
        ]

    def test_call_fraction_is_normalized(self):
        self.assertEqual(M.normalized_call_fraction(self.point(), self.regime()), 0.25)

    def test_short_horizon_may_not_collect_enough_samples(self):
        row = M.simulate_strategies(
            self.point(), self.regime(), 100, 0.01, 3, 100.0
        )
        self.assertFalse(row["evidence_acquired"])

    def test_long_horizon_can_collect_enough_samples(self):
        row = M.simulate_strategies(
            self.point(), self.regime(), 10_000, 0.01, 3, 100.0
        )
        self.assertTrue(row["evidence_acquired"])

    def test_opportunistic_cost_includes_alternate_execution(self):
        row = M.simulate_strategies(
            self.point(), self.regime(), 10_000, 0.01, 3, 100.0
        )
        self.assertEqual(row["alternate_route_cost_per_sample_ns"], 10.0)
        self.assertEqual(row["opportunistic_sampling_cost_ns"], 30.0)

    def test_dedicated_probe_can_be_worse_than_stay_on_short_horizon(self):
        row = M.simulate_strategies(
            self.point(), self.regime(), 100, 0.10, 1, 1000.0
        )
        self.assertLess(row["dedicated_net_vs_stay_ns"], 0.0)


if __name__ == "__main__":
    unittest.main()
