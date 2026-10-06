#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "policy", HERE / "analyze_w2_cross_runner_policy.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def case(break_even=500, consistency=1.0, margin=10.0, capture=1.0):
    return {
        "replica": "r1",
        "cpu": "cpu",
        "regime": "half",
        "point_count": 2,
        "static_cycle_ns": 200.0,
        "candidate_cycle_ns": 100.0,
        "oracle_cycle_ns": 100.0,
        "total_probe_cost_ns": 25_000.0,
        "break_even_calls": break_even,
        "sentinel_winner": "Serial",
        "sentinel_consistency": consistency,
        "sentinel_margin_pct": margin,
        "candidate_capture_fraction": capture,
    }


class CrossRunnerPolicyTests(unittest.TestCase):
    def test_waits_before_break_even(self):
        result = MODULE.evaluate_case(case(break_even=500), 100, 0.85, 5.0)
        self.assertFalse(result["act"])
        self.assertEqual(result["net_saving_ns"], 0.0)

    def test_acts_after_break_even_with_sufficient_evidence(self):
        result = MODULE.evaluate_case(case(break_even=500), 1_000, 0.85, 5.0)
        self.assertTrue(result["act"])
        self.assertGreater(result["net_saving_ns"], 0.0)

    def test_confidence_gate_can_reject_action(self):
        result = MODULE.evaluate_case(
            case(break_even=100, consistency=0.7), 1_000, 0.85, 5.0
        )
        self.assertFalse(result["act"])

    def test_margin_gate_can_reject_action(self):
        result = MODULE.evaluate_case(
            case(break_even=100, margin=1.0), 1_000, 0.85, 5.0
        )
        self.assertFalse(result["act"])

    def test_useless_candidate_is_never_published(self):
        result = MODULE.evaluate_case(
            case(break_even=100, capture=0.0), 1_000, 0.70, 0.0
        )
        self.assertFalse(result["act"])


if __name__ == "__main__":
    unittest.main()
