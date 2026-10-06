#!/usr/bin/env python3
import importlib.util
import math
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "adaptation", HERE / "analyze_adaptation_economics.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def point(regime, n, serial, cpu, auto_backend, weight=1.0):
    return {
        "record_type": "point",
        "schema_version": 1,
        "workload": "w2",
        "regime": regime,
        "work_items": n,
        "weight": weight,
        "serial_samples_ns": [serial, serial, serial],
        "cpu_samples_ns": None if cpu is None else [cpu, cpu, cpu],
        "auto_backend": auto_backend,
    }


def reval(regime, start, published, cost, status="Published"):
    return {
        "record_type": "revalidation",
        "schema_version": 1,
        "workload": "w2",
        "regime": regime,
        "status": status,
        "revalidation_elapsed_ns": cost,
        "published_serial_max_items": published,
        "start_boundary": start,
    }


class AdaptationEconomicsTests(unittest.TestCase):
    def test_break_even_calls_are_measured_from_recoverable_regret(self):
        records = [
            point("baseline-full", 10, 10, 30, "Serial"),
            point("baseline-full", 20, 20, 10, "Cpu"),
            reval("baseline-full", 20, 10, 0, "Bootstrap"),
            point("contention", 10, 10, 30, "Serial"),
            point("contention", 20, 40, 10, "Cpu"),
            reval("contention", 10, 10, 30),
        ]

        summary = MODULE.analyze_main(records)
        row = summary["regimes"]["contention"]

        self.assertEqual(summary["static_boundary"], 10)
        self.assertEqual(row["oracle_boundary"], 10)
        self.assertEqual(row["recoverable_regret_per_cycle_ns"], 0.0)
        self.assertTrue(math.isinf(row["break_even_calls_oracle"]))

    def test_wrong_static_boundary_has_finite_payback(self):
        records = [
            point("baseline-full", 10, 10, 30, "Serial"),
            point("baseline-full", 20, 20, 10, "Cpu"),
            reval("baseline-full", 10, 10, 0, "Bootstrap"),
            point("half", 10, 10, 30, "Serial"),
            point("half", 20, 20, 40, "Serial"),
            reval("half", 10, 20, 50),
        ]

        summary = MODULE.analyze_main(records)
        row = summary["regimes"]["half"]

        self.assertEqual(row["oracle_boundary"], 20)
        self.assertEqual(row["wrong_route_points"], 1)
        self.assertGreater(row["recoverable_regret_per_call_ns"], 0)
        self.assertTrue(math.isfinite(row["break_even_calls_oracle"]))

    def test_non_monotonic_route_preference_is_detected(self):
        rows = [
            point("x", 10, 10, 30, "Serial"),
            point("x", 20, 30, 10, "Cpu"),
            point("x", 40, 10, 30, "Serial"),
        ]
        switches, sequence = MODULE.preference_switches(rows)
        self.assertEqual(sequence, ["Serial", "Cpu", "Serial"])
        self.assertEqual(switches, 2)

    def test_preference_consistency_uses_paired_samples(self):
        row = {
            "record_type": "point",
            "workload": "w2",
            "regime": "x",
            "work_items": 10,
            "weight": 1.0,
            "serial_samples_ns": [10, 10, 30],
            "cpu_samples_ns": [20, 20, 20],
            "auto_backend": "Serial",
        }
        self.assertAlmostEqual(
            MODULE.point_preference_consistency(row),
            2 / 3,
        )


if __name__ == "__main__":
    unittest.main()
