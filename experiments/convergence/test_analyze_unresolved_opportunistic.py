#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "uoe", HERE / "analyze_unresolved_opportunistic.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class UnresolvedOpportunisticTests(unittest.TestCase):
    def test_choose_strategy_prefers_minimum_loss(self):
        row = {
            "stay_loss_ns": 100.0,
            "dedicated_probe_cost_ns": 80.0,
            "opportunistic_loss_ns": 60.0,
        }
        self.assertEqual(M.choose_strategy(row), "opportunistic")

    def test_choose_strategy_can_prefer_stay(self):
        row = {
            "stay_loss_ns": 10.0,
            "dedicated_probe_cost_ns": 80.0,
            "opportunistic_loss_ns": 20.0,
        }
        self.assertEqual(M.choose_strategy(row), "stay")


if __name__ == "__main__":
    unittest.main()
