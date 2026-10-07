#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "upper", HERE / "analyze_generic_policy_upper_bound.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class GenericPolicyUpperBoundTests(unittest.TestCase):
    def cases(self):
        return [
            {"max_points": 3, "candidate_gain_per_call_ns": 10.0, "probe_cost_ns": 100.0, "capture_fraction": 0.4},
            {"max_points": 5, "candidate_gain_per_call_ns": 30.0, "probe_cost_ns": 500.0, "capture_fraction": 1.0},
        ]

    def test_short_horizon_stays(self):
        row = MODULE.best_action(self.cases(), 5)
        self.assertEqual(row["action"], "stay")

    def test_medium_horizon_can_stop_early(self):
        row = MODULE.best_action(self.cases(), 20)
        self.assertEqual(row["action"], "p3")

    def test_long_horizon_can_pay_for_deeper_probe(self):
        row = MODULE.best_action(self.cases(), 100)
        self.assertEqual(row["action"], "p5")


if __name__ == "__main__":
    unittest.main()
