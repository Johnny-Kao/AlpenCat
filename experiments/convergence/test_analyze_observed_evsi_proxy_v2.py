#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("v2", HERE / "analyze_observed_evsi_proxy_v2.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ObservedEvsiV2Tests(unittest.TestCase):
    def test_large_unresolved_work_can_dominate_call_fraction(self):
        demand = [
            {"work_items": 1, "weight": 1.0},
            {"work_items": 2, "weight": 1.0},
            {"work_items": 100, "weight": 1.0},
        ]
        fraction = MODULE.unresolved_work_mass(demand, "Serial", 2)
        self.assertGreater(fraction, 0.9)

    def test_longer_horizon_can_cross_probe_cost(self):
        reval = {
            "measurement_count": 2,
            "revalidation_elapsed_ns": 200.0,
            "observed_sentinels": [
                {"work_items": 1, "serial_cost_ns": 10, "cpu_cost_ns": 20},
                {"work_items": 2, "serial_cost_ns": 20, "cpu_cost_ns": 40},
            ],
        }
        demand = [
            {"work_items": 1, "weight": 1.0},
            {"work_items": 2, "weight": 1.0},
            {"work_items": 100, "weight": 1.0},
        ]
        self.assertFalse(MODULE.evsi_proxy_v2(reval, demand, 1)["continue"])
        self.assertTrue(MODULE.evsi_proxy_v2(reval, demand, 100)["continue"])


if __name__ == "__main__":
    unittest.main()
