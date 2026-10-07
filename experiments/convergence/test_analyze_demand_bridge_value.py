#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "bridge", HERE / "analyze_demand_bridge_value.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DemandBridgeValueTests(unittest.TestCase):
    def reval(self):
        return {
            "status": "NoLocalCrossover",
            "measurement_count": 3,
            "revalidation_elapsed_ns": 300.0,
            "observed_sentinels": [
                {"work_items": 4, "serial_cost_ns": 10, "cpu_cost_ns": 14},
                {"work_items": 8, "serial_cost_ns": 20, "cpu_cost_ns": 28},
                {"work_items": 16, "serial_cost_ns": 40, "cpu_cost_ns": 56},
            ],
        }

    def demand(self):
        return [
            {"work_items": 4, "weight": 1.0},
            {"work_items": 8, "weight": 1.0},
            {"work_items": 16, "weight": 1.0},
            {"work_items": 64, "weight": 1.0},
        ]

    def test_bridge_counts_geometric_steps_to_next_real_demand(self):
        row = MODULE.bridge_value(self.reval(), self.demand(), 100)
        self.assertEqual(row["observed_edge"], 16)
        self.assertEqual(row["target_work_items"], 64)
        self.assertEqual(row["bridge_steps"], 2)

    def test_short_horizon_can_reject_bridge(self):
        row = MODULE.bridge_value(self.reval(), self.demand(), 1)
        self.assertFalse(row["continue"])

    def test_long_horizon_can_buy_bridge(self):
        row = MODULE.bridge_value(self.reval(), self.demand(), 100)
        self.assertTrue(row["continue"])

    def test_published_candidate_skips_bridge(self):
        reval = self.reval()
        reval["status"] = "Published"
        row = MODULE.bridge_value(reval, self.demand(), 1000)
        self.assertFalse(row["continue"])
        self.assertEqual(row["reason"], "bridge-not-applicable")

    def test_no_unresolved_demand_stops(self):
        demand = [
            {"work_items": 4, "weight": 1.0},
            {"work_items": 8, "weight": 1.0},
            {"work_items": 16, "weight": 1.0},
        ]
        row = MODULE.bridge_value(self.reval(), demand, 100)
        self.assertFalse(row["continue"])
        self.assertEqual(row["reason"], "no-unresolved-demand-target")


if __name__ == "__main__":
    unittest.main()
