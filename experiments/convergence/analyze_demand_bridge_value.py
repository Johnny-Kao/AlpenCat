#!/usr/bin/env python3
import argparse
import importlib.util
import json
import math
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "v1", HERE / "analyze_observed_evsi_proxy.py"
)
V1 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(V1)

DEFAULT_HORIZONS = (100, 1_000, 10_000)


def regret_density_ns_per_item(sentinel):
    s = sentinel.get("serial_cost_ns")
    c = sentinel.get("cpu_cost_ns")
    n = int(sentinel.get("work_items", 0))
    if s is None or c is None or n <= 0:
        return None
    return abs(float(s) - float(c)) / n


def next_demand_target(demand_rows, direction, edge):
    candidates = []
    for row in demand_rows:
        n = int(row["work_items"])
        if direction == "Serial" and n > edge:
            candidates.append(row)
        elif direction == "Cpu" and n < edge:
            candidates.append(row)
    if not candidates:
        return None
    if direction == "Serial":
        return min(candidates, key=lambda row: int(row["work_items"]))
    return max(candidates, key=lambda row: int(row["work_items"]))


def geometric_bridge_steps(edge, target, direction):
    if edge <= 0 or target <= 0 or edge == target:
        return 0
    steps = 0
    current = edge
    if direction == "Serial":
        while current < target:
            current *= 2
            steps += 1
    elif direction == "Cpu":
        while current > target:
            current = max(1, current // 2)
            steps += 1
    return steps


def target_call_fraction(demand_rows, target_n):
    total = sum(float(row.get("weight", 1.0)) for row in demand_rows)
    if total <= 0.0:
        return 0.0
    weight = sum(
        float(row.get("weight", 1.0))
        for row in demand_rows
        if int(row["work_items"]) == int(target_n)
    )
    return weight / total


def bridge_value(revalidation, demand_rows, horizon):
    sentinels = list(revalidation.get("observed_sentinels") or [])
    consistency, direction = V1.direction_consistency(sentinels)
    if not sentinels or direction is None:
        return {
            "continue": False,
            "net_bridge_value_ns": 0.0,
            "reason": "no-observed-sentinels",
        }

    edge = (
        max(int(x["work_items"]) for x in sentinels)
        if direction == "Serial"
        else min(int(x["work_items"]) for x in sentinels)
    )
    target = next_demand_target(demand_rows, direction, edge)
    if target is None:
        return {
            "continue": False,
            "net_bridge_value_ns": 0.0,
            "reason": "no-unresolved-demand-target",
            "direction": direction,
            "observed_edge": edge,
        }

    target_n = int(target["work_items"])
    steps = geometric_bridge_steps(edge, target_n, direction)
    next_probe_cost = V1.estimate_next_probe_cost(revalidation)
    bridge_cost = next_probe_cost * steps

    densities = [
        value
        for value in (regret_density_ns_per_item(x) for x in sentinels)
        if value is not None and V1.preferred(x) == direction
    ]
    if not densities:
        return {
            "continue": False,
            "net_bridge_value_ns": -bridge_cost,
            "reason": "no-consistent-regret-density",
        }

    # Conservative lower-envelope extrapolation:
    # use the smallest directly observed wrong-route regret per work item.
    # This intentionally avoids fitting a CPU-specific performance curve.
    regret_density_lb = min(densities)
    target_regret_lb = regret_density_lb * target_n
    call_fraction = target_call_fraction(demand_rows, target_n)
    gross = (
        consistency
        * call_fraction
        * target_regret_lb
        * float(horizon)
    )
    net = gross - bridge_cost

    return {
        "continue": net > 0.0 and steps > 0,
        "net_bridge_value_ns": net,
        "gross_target_value_ns": gross,
        "estimated_bridge_cost_ns": bridge_cost,
        "estimated_probe_cost_ns": next_probe_cost,
        "bridge_steps": steps,
        "direction": direction,
        "direction_consistency": consistency,
        "observed_edge": edge,
        "target_work_items": target_n,
        "target_call_fraction": call_fraction,
        "regret_density_lower_bound_ns_per_item": regret_density_lb,
        "target_regret_lower_bound_ns": target_regret_lb,
        "reason": "positive-bridge-value" if net > 0.0 and steps > 0 else "nonpositive-bridge-value",
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sensitivity_root", type=pathlib.Path)
    parser.add_argument("main_root", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS))
    args = parser.parse_args()

    demand_records = V1.load_jsonl(args.main_root / "evidence.jsonl")
    demand_by_regime = {}
    for row in demand_records:
        if row.get("record_type") == "point":
            demand_by_regime.setdefault(row["regime"], []).append(row)

    output = {
        "schema_version": 1,
        "label": "observed-only-demand-aware-bridge-value-v1",
        "warning": (
            "Research-only estimator. Uses only already-observed sentinel measurements, "
            "known demand support/weights, and measured probe cost. The regret-density "
            "lower envelope is a generic extrapolation hypothesis and requires "
            "cross-workload validation."
        ),
        "cases": [],
    }

    for path in sorted(args.sensitivity_root.glob("*-p*.jsonl")):
        rows = V1.load_jsonl(path)
        reval = next((row for row in rows if row.get("record_type") == "revalidation"), None)
        if reval is None:
            continue
        regime = reval["regime"]
        case = {
            "regime": regime,
            "max_points": int(path.stem.split("-p")[-1]),
            "horizons": {},
        }
        for horizon in args.horizons:
            case["horizons"][str(horizon)] = bridge_value(
                reval, demand_by_regime.get(regime, []), horizon
            )
        output["cases"].append(case)

    target = args.main_root / "demand-aware-bridge-value.json"
    target.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
