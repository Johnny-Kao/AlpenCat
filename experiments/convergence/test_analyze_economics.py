#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "analyze_economics", HERE / "analyze_economics.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def point(regime, n, serial, cpu, auto, auto_backend):
    return {
        "record_type": "point",
        "schema_version": 1,
        "workload": "w1",
        "regime": regime,
        "work_items": n,
        "weight": 1.0,
        "parallelism": 4,
        "serial_samples_ns": [serial, serial, serial],
        "cpu_samples_ns": None if cpu is None else [cpu, cpu, cpu],
        "cpu_route_available": cpu is not None,
        "output_equivalent": None if cpu is None else True,
        "auto_samples_ns": [auto, auto, auto],
        "auto_backend": auto_backend,
        "boundary_stale": False,
    }


class EconomicsAnalyzerTests(unittest.TestCase):
    def test_reconstructs_four_policy_costs_from_same_evidence(self):
        records = [
            point("baseline-full", 10, 10, 30, 10, "Serial"),
            point("baseline-full", 20, 20, 10, 10, "Cpu"),
            {
                "record_type": "revalidation",
                "schema_version": 1,
                "workload": "w1",
                "regime": "baseline-full",
                "status": "Published",
                "measurement_count": 2,
                "revalidation_elapsed_ns": 5,
                "published_serial_max_items": 10,
                "boundary_stale": False,
            },
            point("half", 10, 10, 40, 10, "Serial"),
            point("half", 20, 20, 30, 20, "Serial"),
            {
                "record_type": "revalidation",
                "schema_version": 1,
                "workload": "w1",
                "regime": "half",
                "status": "Published",
                "measurement_count": 2,
                "revalidation_elapsed_ns": 5,
                "published_serial_max_items": 20,
                "boundary_stale": False,
            },
        ]

        summary = MODULE.analyze(records, calls_per_point=10)
        half = summary["workloads"]["w1"]["regimes"]["half"]

        self.assertEqual(summary["workloads"]["w1"]["static_boundary"], 10)
        self.assertEqual(half["periodic_boundary"], 20)
        self.assertEqual(half["total_ns"]["Static"], 400.0)
        self.assertEqual(half["execution_ns"]["Periodic"], 300.0)
        self.assertEqual(half["execution_ns"]["AlpenCat"], 300.0)
        self.assertEqual(half["execution_ns"]["Oracle"], 300.0)
        self.assertGreater(half["total_ns"]["Periodic"], half["execution_ns"]["Periodic"])
        self.assertEqual(half["calibration_ns"]["AlpenCat"], 5.0)

    def test_unavailable_cpu_keeps_oracle_on_serial(self):
        records = [
            point("baseline-full", 10, 10, None, 10, "Serial"),
            {
                "record_type": "revalidation",
                "schema_version": 1,
                "workload": "w1",
                "regime": "baseline-full",
                "status": "RouteUnavailable(Cpu)",
                "measurement_count": 1,
                "revalidation_elapsed_ns": 3,
                "published_serial_max_items": 32_768,
                "boundary_stale": True,
            },
        ]
        summary = MODULE.analyze(records, calls_per_point=1)
        row = summary["workloads"]["w1"]["regimes"]["baseline-full"]
        self.assertEqual(row["total_ns"]["Oracle"], 10.0)
        self.assertEqual(row["route_counts"]["Oracle"]["Serial"], 1)


if __name__ == "__main__":
    unittest.main()
