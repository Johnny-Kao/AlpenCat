#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "policy", HERE / "analyze_generic_policy_state_machine.py"
)
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class GenericPolicyStateMachineTests(unittest.TestCase):
    def test_no_opportunity_stays_before_any_bridge_logic(self):
        switching = {
            "opportunity_exists": False,
            "candidate_exists": False,
            "candidate_miss": False,
        }
        self.assertEqual(
            M.decide_path(switching, {"decision": "continue"}, []),
            "stay:no-economic-opportunity",
        )

    def test_existing_candidate_skips_bridge(self):
        switching = {
            "opportunity_exists": True,
            "candidate_exists": True,
            "candidate_miss": False,
        }
        self.assertEqual(
            M.decide_path(switching, {"decision": "continue"}, []),
            "candidate:switching-economics",
        )

    def test_candidate_miss_can_use_dedicated_bridge(self):
        switching = {
            "opportunity_exists": True,
            "candidate_exists": False,
            "candidate_miss": True,
        }
        self.assertEqual(
            M.decide_path(switching, {"decision": "continue"}, []),
            "probe:dedicated-bridge",
        )

    def test_unresolved_can_defer_to_opportunistic(self):
        switching = {
            "opportunity_exists": True,
            "candidate_exists": False,
            "candidate_miss": True,
        }
        rows = [{
            "best_strategy": "opportunistic",
            "stay_loss_ns": 100.0,
            "dedicated_probe_cost_ns": 80.0,
            "opportunistic_loss_ns": 20.0,
        }]
        self.assertEqual(
            M.decide_path(switching, {"decision": "unresolved"}, rows),
            "unresolved:opportunistic",
        )


if __name__ == "__main__":
    unittest.main()
