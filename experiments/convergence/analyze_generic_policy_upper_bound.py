#!/usr/bin/env python3
import argparse
import json
import pathlib


DEFAULT_HORIZONS = (100, 1_000, 10_000)


def best_action(cases, horizon):
    actions = [{
        "action": "stay",
        "max_points": 0,
        "net_gain_ns": 0.0,
        "capture_fraction": 0.0,
    }]
    for row in cases:
        net = (
            float(row["candidate_gain_per_call_ns"]) * horizon
            - float(row["probe_cost_ns"])
        )
        actions.append({
            "action": f"p{row['max_points']}",
            "max_points": int(row["max_points"]),
            "net_gain_ns": net,
            "capture_fraction": float(row["capture_fraction"]),
        })
    return max(actions, key=lambda row: (row["net_gain_ns"], -row["max_points"]))


def analyze(sentinel, horizons):
    regimes = sorted({row["regime"] for row in sentinel["cases"]})
    out = {
        "schema_version": 1,
        "label": "hindsight-upper-bound-only",
        "warning": (
            "Uses realized post-hoc evidence to choose the best stopping point. "
            "This is a benchmark envelope, not a deployable runtime policy."
        ),
        "regimes": {},
    }
    for regime in regimes:
        cases = [row for row in sentinel["cases"] if row["regime"] == regime]
        out["regimes"][regime] = {}
        for horizon in horizons:
            out["regimes"][regime][str(horizon)] = best_action(cases, horizon)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sentinel_json", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS))
    args = parser.parse_args()

    sentinel = json.loads(args.sentinel_json.read_text())
    result = analyze(sentinel, tuple(args.horizons))
    target = args.sentinel_json.with_name("generic-policy-upper-bound.json")
    target.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
