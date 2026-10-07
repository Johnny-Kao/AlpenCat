#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "event", HERE / "analyze_event_driven_opportunistic.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class EventDrivenOpportunisticTests(unittest.TestCase):
    def point(self, serial, cpu):
        return {
            "serial_samples_ns": [serial, serial, serial],
            "cpu_samples_ns": [cpu, cpu, cpu],
        }

    def test_short_remaining_horizon_can_reject_sample(self):
        row = M.one_sample_decision(
            {"direction": "Serial"},
            self.point(100, 110),
            5,
        )
        self.assertFalse(row["sample"])

    def test_long_remaining_horizon_can_accept_sample(self):
        row = M.one_sample_decision(
            {"direction": "Serial"},
            self.point(100, 110),
            20,
        )
        self.assertTrue(row["sample"])

    def test_missing_direction_rejects_sample(self):
        row = M.one_sample_decision({}, self.point(100, 110), 100)
        self.assertFalse(row["sample"])


if __name__ == "__main__":
    unittest.main()
