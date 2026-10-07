#!/usr/bin/env python3
import argparse
import json
import pathlib


def load_json(path):
    return json.loads(path.read_text())


def index_bridge(bounds):
    out = {}
    for case in bounds.get("cases", []):
        regime = case["regime"]
        points = int(case.get("max_points", 0))
        if points != 3:
            continue
        out[regime] = case.get("horizons", {})
    return out


def index_opportunistic(opportunistic):
    out = {}
    for row in opportunistic.get("cases", []):
        key = (row["regime"], int(row["horizon"]))
        out.setdefault(key, []).append(row)
    return out


def choose_unresolved_strategy(rows):
    if not rows:
        return "defer"
    best = min(
        rows,
        key=lambda row: min(
            row["stay_loss_ns"],
            row["dedicated_probe_cost_ns"],
            row["opportunistic_loss_ns"],
        ),
    )
    return best["best_strategy"]


def decide_path(switching_regime, bridge_horizon=None, opportunistic_rows=None):
    if not switching_regime.get("opportunity_exists", False):
        return "stay:no-economic-opportunity"

    if switching_regime.get("candidate_exists", False):
        return "candidate:switching-economics"

    if not switching_regime.get("candidate_miss", False):
        return "stay:no-actionable-candidate"

    if bridge_horizon is None:
        return "defer:missing-bridge-evidence"

    decision = bridge_horizon.get("decision")
    if decision == "continue":
        return "probe:dedicated-bridge"
    if decision == "stop":
        return "stay:bridge-not-worth-cost"
    if decision == "unresolved":
        strategy = choose_unresolved_strategy(opportunistic_rows or [])
        return f"unresolved:{strategy}"
    if decision == "not-applicable":
        return "defer:bridge-not-applicable"
    return "defer:unknown-bridge-state"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("evidence_root", type=pathlib.Path)
    p.add_argument("--horizons", type=int, nargs="+", default=[100, 1_000, 10_000])
    args = p.parse_args()

    switching = load_json(args.evidence_root / "switching-economics.json")
    bounds = load_json(args.evidence_root / "demand-aware-bridge-bounds.json")
    opportunistic_path = args.evidence_root / "unresolved-opportunistic-economics.json"
    opportunistic = load_json(opportunistic_path) if opportunistic_path.exists() else {"cases": []}

    bridge_by_regime = index_bridge(bounds)
    opp_by_key = index_opportunistic(opportunistic)

    out = {
        "schema_version": 1,
        "label": "generic-policy-state-machine-v1",
        "policy_order": [
            "economic-opportunity",
            "candidate-exists",
            "candidate-miss",
            "bridge-bounds",
            "unresolved-opportunistic",
        ],
        "regimes": {},
    }

    for regime, switching_regime in switching.get("regimes", {}).items():
        out["regimes"][regime] = {}
        for horizon in args.horizons:
            bridge_horizon = bridge_by_regime.get(regime, {}).get(str(horizon))
            opp_rows = opp_by_key.get((regime, horizon), [])
            out["regimes"][regime][str(horizon)] = {
                "path": decide_path(
                    switching_regime,
                    bridge_horizon=bridge_horizon,
                    opportunistic_rows=opp_rows,
                ),
                "opportunity_exists": switching_regime.get("opportunity_exists", False),
                "candidate_exists": switching_regime.get("candidate_exists", False),
                "candidate_miss": switching_regime.get("candidate_miss", False),
                "bridge_decision": (
                    bridge_horizon.get("decision") if bridge_horizon else None
                ),
            }

    target = args.evidence_root / "generic-policy-state-machine.json"
    target.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
