#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "baseline", HERE / "benchmark_next_sentinel_estimators.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NextSentinelEstimatorBaselineTests(unittest.TestCase):
    def w2_shape(self):
        return [
            {"max_points": 3, "candidate_gain_per_call_ns": 10.0, "probe_cost_ns": 100.0},
            {"max_points": 4, "candidate_gain_per_call_ns": 10.0, "probe_cost_ns": 150.0},
            {"max_points": 5, "candidate_gain_per_call_ns": 30.0, "probe_cost_ns": 300.0},
            {"max_points": 6, "candidate_gain_per_call_ns": 30.0, "probe_cost_ns": 400.0},
        ]

    def test_myopic_stops_before_later_jump(self):
        row = MODULE.myopic_last_marginal(self.w2_shape(), 100)
        self.assertEqual(row["action"], "p3")
        best = MODULE.hindsight_action(self.w2_shape(), 100)
        self.assertEqual(best["action"], "p5")
        self.assertGreater(best["net_gain_ns"], row["net_gain_ns"])

    def test_short_horizon_can_stay(self):
        row = MODULE.myopic_last_marginal(self.w2_shape(), 5)
        self.assertEqual(row["action"], "stay")


if __name__ == "__main__":
    unittest.main()
