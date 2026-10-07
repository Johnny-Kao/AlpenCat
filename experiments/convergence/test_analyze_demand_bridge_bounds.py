#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "bounds", HERE / "analyze_demand_bridge_bounds.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DemandBridgeBoundsTests(unittest.TestCase):
    def demand(self):
        return [
            {"work_items": 4, "weight": 1.0},
            {"work_items": 8, "weight": 1.0},
            {"work_items": 16, "weight": 1.0},
            {"work_items": 64, "weight": 1.0},
        ]

    def reval(self, serial, cpu, elapsed=300):
        return {
            "status": "NoLocalCrossover",
            "measurement_count": 3,
            "revalidation_elapsed_ns": elapsed,
            "observed_sentinels": [
                {
                    "work_items": 16,
                    "serial_cost_ns": sorted(serial)[len(serial)//2],
                    "cpu_cost_ns": sorted(cpu)[len(cpu)//2],
                    "serial_samples_ns": serial,
                    "cpu_samples_ns": cpu,
                }
            ],
        }

    def test_strong_repeat_evidence_can_continue(self):
        row = MODULE.bridge_value_bounds(
            self.reval([40, 41, 42], [80, 82, 84], elapsed=30),
            self.demand(),
            100,
        )
        self.assertEqual(row["decision"], "continue")

    def test_sign_flips_produce_unresolved_near_break_even(self):
        row = MODULE.bridge_value_bounds(
            self.reval([40, 60, 40], [50, 50, 50], elapsed=30),
            self.demand(),
            10,
        )
        self.assertEqual(row["decision"], "unresolved")

    def test_published_candidate_is_not_applicable(self):
        reval = self.reval([40, 41, 42], [80, 82, 84])
        reval["status"] = "Published"
        row = MODULE.bridge_value_bounds(reval, self.demand(), 1000)
        self.assertEqual(row["decision"], "not-applicable")


if __name__ == "__main__":
    unittest.main()
