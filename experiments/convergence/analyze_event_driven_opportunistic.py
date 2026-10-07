#!/usr/bin/env python3
import argparse
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "bounds", HERE / "analyze_demand_bridge_bounds.py"
)
BOUNDS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BOUNDS)


def load_json(path):
    return json.loads(path.read_text())


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def median(values):
    xs = sorted(float(x) for x in values)
    return xs[len(xs) // 2]


def route_costs(point):
    serial = point.get("serial_samples_ns")
    cpu = point.get("cpu_samples_ns")
    if not isinstance(serial, list) or not isinstance(cpu, list) or not serial or not cpu:
        return None, None
    return median(serial), median(cpu)


def one_sample_decision(bound_row, target_point, remaining_target_calls):
    direction = bound_row.get("direction")
    serial, cpu = route_costs(target_point)
    if serial is None or cpu is None:
        return {
            "sample": False,
            "reason": "missing-target-route-costs",
        }

    if direction == "Serial":
        stale = cpu
        alternate = serial
        realized_regret = max(0.0, cpu - serial)
    elif direction == "Cpu":
        stale = serial
        alternate = cpu
        realized_regret = max(0.0, serial - cpu)
    else:
        return {
            "sample": False,
            "reason": "missing-direction",
        }

    # Evaluation-only information-value ceiling:
    # if learning the correct route from one alternate execution could avoid
    # the realized regret over the remaining horizon, that is the maximum
    # value this sample could unlock. Runtime policy must replace this with an
    # observable bound before deployment.
    info_value_ceiling = realized_regret * max(0.0, float(remaining_target_calls))
    incremental_sample_cost = max(0.0, alternate)

    return {
        "sample": info_value_ceiling > incremental_sample_cost,
        "reason": (
            "positive-single-sample-value"
            if info_value_ceiling > incremental_sample_cost
            else "nonpositive-single-sample-value"
        ),
        "stale_route_cost_ns": stale,
        "alternate_route_cost_ns": alternate,
        "realized_regret_per_call_ns": realized_regret,
        "remaining_target_calls": float(remaining_target_calls),
        "information_value_ceiling_ns": info_value_ceiling,
        "incremental_sample_cost_ns": incremental_sample_cost,
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("evidence_root", type=pathlib.Path)
    p.add_argument("--horizons", type=int, nargs="+", default=[100, 1_000, 10_000])
    args = p.parse_args()

    bounds = load_json(args.evidence_root / "demand-aware-bridge-bounds.json")
    evidence = load_jsonl(args.evidence_root / "evidence.jsonl")

    points_by_regime = {}
    for row in evidence:
        if row.get("record_type") == "point":
            points_by_regime.setdefault(row["regime"], []).append(row)

    out = {
        "schema_version": 1,
        "label": "event-driven-opportunistic-evidence-v1",
        "warning": (
            "Offline oracle-style evaluation of whether one alternate execution could "
            "pay for itself. Realized target-route regret is used only as evaluation "
            "truth, not as a deployable runtime input."
        ),
        "cases": [],
    }

    for case in bounds.get("cases", []):
        regime = case["regime"]
        points = points_by_regime.get(regime, [])
        for horizon_text, bound_row in case.get("horizons", {}).items():
            if bound_row.get("decision") != "unresolved":
                continue
            target_n = bound_row.get("target_work_items")
            if target_n is None:
                continue
            point = next(
                (row for row in points if int(row["work_items"]) == int(target_n)),
                None,
            )
            if point is None:
                continue

            horizon = int(horizon_text)
            total_weight = sum(float(p.get("weight", 1.0)) for p in points)
            target_fraction = (
                float(point.get("weight", 1.0)) / total_weight
                if total_weight > 0.0
                else 0.0
            )
            remaining_target_calls = max(0.0, float(horizon) * target_fraction - 1.0)
            row = one_sample_decision(bound_row, point, remaining_target_calls)
            row.update(
                {
                    "regime": regime,
                    "max_points": case.get("max_points"),
                    "horizon": horizon,
                    "target_work_items": int(target_n),
                    "target_call_fraction": target_fraction,
                    "bound_net_lower_ns": bound_row.get("net_value_lower_bound_ns"),
                    "bound_net_upper_ns": bound_row.get("net_value_upper_bound_ns"),
                }
            )
            out["cases"].append(row)

    target = args.evidence_root / "event-driven-opportunistic-evidence.json"
    target.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
