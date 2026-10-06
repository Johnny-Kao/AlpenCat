#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "holdout", HERE / "analyze_w2_holdout.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def case(candidate_cycle=100.0, break_even=500):
    return {
        "replica": "r",
        "cpu": "cpu",
        "regime": "half",
        "static_boundary": 10,
        "candidate_boundary": 20,
        "train_break_even_calls": break_even,
        "train_sentinel_winner": "Serial",
        "train_sentinel_consistency": 1.0,
        "train_sentinel_margin_pct": 10.0,
        "train_candidate_capture_fraction": 1.0,
        "probe_cost_ns": 25_000.0,
        "holdout_weight": 2.0,
        "holdout_static_cycle_ns": 200.0,
        "holdout_candidate_cycle_ns": candidate_cycle,
        "holdout_oracle_cycle_ns": 100.0,
    }


class HoldoutPolicyTests(unittest.TestCase):
    def test_positive_transfer_can_pay(self):
        result = MODULE.evaluate(case(), 1_000, 0.85, 5.0)
        self.assertTrue(result["act"])
        self.assertGreater(result["net_saving_ns"], 0.0)

    def test_bad_transfer_can_be_negative(self):
        result = MODULE.evaluate(
            case(candidate_cycle=300.0), 1_000, 0.85, 5.0
        )
        self.assertTrue(result["act"])
        self.assertLess(result["net_saving_ns"], 0.0)

    def test_short_horizon_waits(self):
        result = MODULE.evaluate(case(), 100, 0.85, 5.0)
        self.assertFalse(result["act"])
        self.assertEqual(result["net_saving_ns"], 0.0)


if __name__ == "__main__":
    unittest.main()
