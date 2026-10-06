#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("profile_shape", HERE / "analyze_profile_shape.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def point(regime, n, serial, cpu):
    return {
        "record_type": "point",
        "schema_version": 1,
        "workload": "w2",
        "regime": regime,
        "work_items": n,
        "weight": 1.0,
        "serial_samples_ns": [serial, serial, serial],
        "cpu_samples_ns": [cpu, cpu, cpu],
    }


class ProfileShapeTests(unittest.TestCase):
    def test_interval_represents_serial_cpu_serial_shape(self):
        records = [
            point("half", 10, 10, 30),
            point("half", 20, 30, 10),
            point("half", 40, 10, 30),
        ]
        row = MODULE.analyze(records)["regimes"]["half"]
        self.assertEqual(row["preference_sequence"], ["Serial", "Cpu", "Serial"])
        self.assertGreater(row["scalar_gap_to_oracle_pct"], 0)
        self.assertEqual(row["interval_gap_to_oracle_pct"], 0.0)
        self.assertTrue(row["interval_exact_on_measured_grid"])

    def test_scalar_is_sufficient_for_monotonic_shape(self):
        records = [
            point("baseline-full", 10, 10, 30),
            point("baseline-full", 20, 30, 10),
            point("baseline-full", 40, 60, 20),
        ]
        row = MODULE.analyze(records)["regimes"]["baseline-full"]
        self.assertEqual(row["preference_switch_count"], 1)
        self.assertEqual(row["scalar_gap_to_oracle_pct"], 0.0)
        self.assertEqual(row["interval_gap_to_oracle_pct"], 0.0)


if __name__ == "__main__":
    unittest.main()
