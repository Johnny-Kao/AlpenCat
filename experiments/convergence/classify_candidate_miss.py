#!/usr/bin/env python3
import argparse
import json
import pathlib


def load_json(path):
    return json.loads(path.read_text())


def classify(switching, profile=None, sensitivity=None):
    rows = []
    profile_regimes = {} if profile is None else profile.get("regimes", {})
    sensitivity_rows = [] if sensitivity is None else sensitivity
    sensitivity_by_regime = {}
    for row in sensitivity_rows:
        sensitivity_by_regime.setdefault(row["regime"], []).append(row)

    for regime, row in switching["regimes"].items():
        if not row.get("candidate_miss"):
            rows.append({
                "regime": regime,
                "candidate_miss": False,
                "classification": "none",
                "reason": "no candidate miss",
            })
            continue

        shape = profile_regimes.get(regime, {})
        non_monotonic = shape.get("preference_switch_count", 0) > 1
        interval_capture = float(
            shape.get("interval_capture_of_scalar_gap_fraction", 0.0) or 0.0
        )

        sens = sorted(
            sensitivity_by_regime.get(regime, []),
            key=lambda item: int(item.get("max_points", 0)),
        )
        published = [
            item.get("published_boundary")
            for item in sens
            if item.get("status") == "Published"
        ]
        larger_budget_finds_candidate = len(set(published)) > 1

        consistency = float(row.get("confidence_proxy", 0.0) or 0.0)

        if non_monotonic and interval_capture >= 0.5:
            kind = "representation-limited"
            reason = "oracle preference is non-monotonic and a bounded interval recovers material scalar regret"
        elif larger_budget_finds_candidate:
            kind = "radius-limited"
            reason = "larger bounded-search budgets change the published candidate"
        elif consistency < 0.7:
            kind = "noisy-local-evidence"
            reason = "near-boundary paired preference consistency is below 70%"
        else:
            kind = "directional-miss"
            reason = "economic opportunity exists but current local search does not expose a candidate"

        rows.append({
            "regime": regime,
            "candidate_miss": True,
            "classification": kind,
            "reason": reason,
            "confidence_proxy": consistency,
            "non_monotonic": non_monotonic,
            "interval_capture_of_scalar_gap_fraction": interval_capture,
            "larger_budget_finds_candidate": larger_budget_finds_candidate,
        })
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("switching_json", type=pathlib.Path)
    parser.add_argument("--profile-json", type=pathlib.Path)
    parser.add_argument("--sensitivity-json", type=pathlib.Path)
    args = parser.parse_args()

    switching = load_json(args.switching_json)
    profile = load_json(args.profile_json) if args.profile_json else None
    sensitivity = load_json(args.sensitivity_json) if args.sensitivity_json else None

    rows = classify(switching, profile, sensitivity)
    print(json.dumps(rows, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
