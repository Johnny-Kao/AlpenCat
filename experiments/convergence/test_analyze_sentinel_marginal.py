#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "marginal", HERE / "analyze_sentinel_marginal.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SentinelMarginalTests(unittest.TestCase):
    def test_no_gain_step_has_no_break_even(self):
        rows = [
            {"regime":"x","max_points":3,"candidate_gain_per_call_ns":10.0,"probe_cost_ns":100.0},
            {"regime":"x","max_points":4,"candidate_gain_per_call_ns":10.0,"probe_cost_ns":150.0},
        ]
        row = MODULE.marginal(rows)[0]
        self.assertFalse(row["adds_value"])
        self.assertIsNone(row["marginal_break_even_calls"])

    def test_positive_increment_gets_break_even(self):
        rows = [
            {"regime":"x","max_points":4,"candidate_gain_per_call_ns":10.0,"probe_cost_ns":150.0},
            {"regime":"x","max_points":5,"candidate_gain_per_call_ns":30.0,"probe_cost_ns":250.0},
        ]
        row = MODULE.marginal(rows)[0]
        self.assertTrue(row["adds_value"])
        self.assertEqual(row["marginal_break_even_calls"], 5.0)


if __name__ == "__main__":
    unittest.main()
