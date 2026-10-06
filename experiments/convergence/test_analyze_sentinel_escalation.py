#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "sentinel", HERE / "analyze_sentinel_escalation.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SentinelEscalationTests(unittest.TestCase):
    def test_serial_direction_expands_only_to_observed_sentinel(self):
        self.assertEqual(
            MODULE.infer_one_sided_candidate(262_144, 3, "Serial"),
            1_048_576,
        )

    def test_serial_direction_caps_at_max(self):
        self.assertEqual(
            MODULE.infer_one_sided_candidate(262_144, 6, "Serial"),
            4_194_304,
        )

    def test_cpu_direction_contracts(self):
        self.assertEqual(
            MODULE.infer_one_sided_candidate(262_144, 3, "Cpu"),
            65_536,
        )


if __name__ == "__main__":
    unittest.main()
