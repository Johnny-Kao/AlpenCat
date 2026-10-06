#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "switching", HERE / "analyze_switching_economics.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SwitchingEconomicsTests(unittest.TestCase):
    def base_row(self, **updates):
        row = {
            "start_boundary": 1024,
            "published_boundary": 2048,
            "oracle_boundary": 2048,
            "status": "Published",
            "realized_execution_gain_per_call_ns": 100.0,
            "revalidation_cost_ns": 1_000.0,
            "near_boundary_preference_consistency_min": 1.0,
        }
        row.update(updates)
        return row

    def test_switch_cost_delays_break_even(self):
        result = MODULE.simulate_regime(
            self.base_row(), (10, 20), (1.0,), verify_calls=0
        )
        horizons = result["switch_sensitivity"]["1.0"]["horizons"]
        self.assertFalse(horizons["10"]["break_even_action"])
        self.assertFalse(horizons["20"]["break_even_action"])

    def test_no_boundary_change_has_zero_switch_cost(self):
        row = self.base_row(published_boundary=1024)
        result = MODULE.simulate_regime(row, (20,), (2.0,), verify_calls=0)
        costs = result["switch_sensitivity"]["2.0"]["costs"]
        self.assertEqual(costs["switch_ns"], 0.0)

    def test_low_confidence_prices_expected_rollback(self):
        row = self.base_row(
            near_boundary_preference_consistency_min=0.5
        )
        result = MODULE.simulate_regime(row, (100,), (1.0,), verify_calls=3)
        costs = result["switch_sensitivity"]["1.0"]["costs"]
        self.assertEqual(costs["expected_rollback_ns"], 500.0)

    def test_negative_candidate_gain_never_verifies(self):
        row = self.base_row(realized_execution_gain_per_call_ns=-10.0)
        result = MODULE.simulate_regime(row, (10_000,), (0.0,), verify_calls=3)
        horizon = result["switch_sensitivity"]["0.0"]["horizons"]["10000"]
        self.assertFalse(horizon["verify_action"])
        self.assertFalse(horizon["confidence_verify_action"])


if __name__ == "__main__":
    unittest.main()
