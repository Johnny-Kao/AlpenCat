#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "evsi", HERE / "analyze_observed_evsi_proxy.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ObservedEvsiProxyTests(unittest.TestCase):
    def demand(self):
        return [
            {"work_items": 64, "weight": 1.0},
            {"work_items": 128, "weight": 1.0},
            {"work_items": 256, "weight": 1.0},
            {"work_items": 512, "weight": 1.0},
        ]

    def reval(self):
        return {
            "measurement_count": 3,
            "revalidation_elapsed_ns": 300.0,
            "observed_sentinels": [
                {"work_items": 64, "serial_cost_ns": 10, "cpu_cost_ns": 20},
                {"work_items": 128, "serial_cost_ns": 20, "cpu_cost_ns": 40},
                {"work_items": 256, "serial_cost_ns": 30, "cpu_cost_ns": 60},
            ],
        }

    def test_short_horizon_can_reject_next_probe(self):
        row = MODULE.evsi_proxy(self.reval(), self.demand(), 1)
        self.assertFalse(row["continue"])

    def test_long_horizon_can_buy_next_probe(self):
        row = MODULE.evsi_proxy(self.reval(), self.demand(), 100)
        self.assertTrue(row["continue"])
        self.assertEqual(row["direction"], "Serial")
        self.assertEqual(row["direction_consistency"], 1.0)

    def test_no_unresolved_demand_stops(self):
        demand = [
            {"work_items": 64, "weight": 1.0},
            {"work_items": 128, "weight": 1.0},
            {"work_items": 256, "weight": 1.0},
        ]
        row = MODULE.evsi_proxy(self.reval(), demand, 100)
        self.assertFalse(row["continue"])
        self.assertEqual(row["unresolved_demand_fraction"], 0.0)


if __name__ == "__main__":
    unittest.main()
