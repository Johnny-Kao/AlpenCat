#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "miss", HERE / "classify_candidate_miss.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CandidateMissTests(unittest.TestCase):
    def switching(self, consistency=1.0):
        return {
            "regimes": {
                "x": {
                    "candidate_miss": True,
                    "confidence_proxy": consistency,
                }
            }
        }

    def test_representation_limited_precedes_noise(self):
        profile = {
            "regimes": {
                "x": {
                    "preference_switch_count": 2,
                    "interval_capture_of_scalar_gap_fraction": 0.8,
                }
            }
        }
        row = MODULE.classify(self.switching(0.5), profile, [])[0]
        self.assertEqual(row["classification"], "representation-limited")

    def test_radius_limited_when_budget_changes_candidate(self):
        sensitivity = [
            {"regime": "x", "max_points": 3, "status": "Published", "published_boundary": 64},
            {"regime": "x", "max_points": 5, "status": "Published", "published_boundary": 256},
        ]
        row = MODULE.classify(self.switching(), {}, sensitivity)[0]
        self.assertEqual(row["classification"], "radius-limited")

    def test_noisy_when_consistency_low(self):
        row = MODULE.classify(self.switching(0.6), {}, [])[0]
        self.assertEqual(row["classification"], "noisy-local-evidence")

    def test_directional_miss_is_fallback(self):
        row = MODULE.classify(self.switching(1.0), {}, [])[0]
        self.assertEqual(row["classification"], "directional-miss")


if __name__ == "__main__":
    unittest.main()
