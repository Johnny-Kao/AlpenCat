#!/usr/bin/env python3
import argparse
import json
import pathlib


DEFAULT_HORIZONS = (100, 1_000, 10_000)


def net_value(row, horizon):
    return (
        float(row["candidate_gain_per_call_ns"]) * horizon
        - float(row["probe_cost_ns"])
    )


def by_points(cases):
    return {int(row["max_points"]): row for row in cases}


def fixed_action(cases, points, horizon):
    row = by_points(cases).get(points)
    if row is None:
        return {"action": "stay", "net_gain_ns": 0.0}
    value = net_value(row, horizon)
    if value <= 0.0:
        return {"action": "stay", "net_gain_ns": 0.0}
    return {"action": f"p{points}", "net_gain_ns": value}


def hindsight_action(cases, horizon):
    best = {"action": "stay", "net_gain_ns": 0.0, "points": 0}
    for row in cases:
        value = net_value(row, horizon)
        points = int(row["max_points"])
        candidate = {"action": f"p{points}", "net_gain_ns": value, "points": points}
        if (candidate["net_gain_ns"], -candidate["points"]) > (
            best["net_gain_ns"], -best["points"]
        ):
            best = candidate
    return {"action": best["action"], "net_gain_ns": best["net_gain_ns"]}


def myopic_last_marginal(cases, horizon):
    ordered = sorted(cases, key=lambda row: int(row["max_points"]))
    if not ordered:
        return {"action": "stay", "net_gain_ns": 0.0}

    first = ordered[0]
    if net_value(first, horizon) <= 0.0:
        return {"action": "stay", "net_gain_ns": 0.0}

    chosen = first
    previous = first
    for row in ordered[1:]:
        delta_gain = (
            float(row["candidate_gain_per_call_ns"])
            - float(previous["candidate_gain_per_call_ns"])
        )
        delta_cost = float(row["probe_cost_ns"]) - float(previous["probe_cost_ns"])
        # Deliberately myopic baseline: continue only if the immediately
        # preceding step pays for itself at the declared horizon.
        if delta_gain * horizon <= delta_cost:
            break
        chosen = row
        previous = row

    return {
        "action": f"p{int(chosen['max_points'])}",
        "net_gain_ns": net_value(chosen, horizon),
    }


def analyze(data, horizons):
    regimes = sorted({row["regime"] for row in data["cases"]})
    result = {
        "schema_version": 1,
        "warning": (
            "Myopic last-marginal is an intentionally weak baseline. "
            "Hindsight is a post-hoc upper bound, not a runtime policy."
        ),
        "regimes": {},
    }
    for regime in regimes:
        cases = [row for row in data["cases"] if row["regime"] == regime]
        result["regimes"][regime] = {}
        for horizon in horizons:
            result["regimes"][regime][str(horizon)] = {
                "fixed_p3": fixed_action(cases, 3, horizon),
                "fixed_p5": fixed_action(cases, 5, horizon),
                "myopic_last_marginal": myopic_last_marginal(cases, horizon),
                "hindsight_upper_bound": hindsight_action(cases, horizon),
            }
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sentinel_json", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS))
    args = parser.parse_args()

    data = json.loads(args.sentinel_json.read_text())
    result = analyze(data, tuple(args.horizons))
    target = args.sentinel_json.with_name("next-sentinel-estimator-baselines.json")
    target.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
