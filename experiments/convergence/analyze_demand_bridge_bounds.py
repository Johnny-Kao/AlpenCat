#!/usr/bin/env python3
import argparse
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "bridge", HERE / "analyze_demand_bridge_value.py"
)
BRIDGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BRIDGE)


def directional_density_bounds(sentinels, direction):
    densities = []
    for sentinel in sentinels:
        n = int(sentinel.get("work_items", 0))
        serial = sentinel.get("serial_samples_ns")
        cpu = sentinel.get("cpu_samples_ns")
        if n <= 0 or not isinstance(serial, list) or not isinstance(cpu, list):
            continue
        for s, c in zip(serial, cpu):
            if direction == "Serial":
                densities.append((float(c) - float(s)) / n)
            elif direction == "Cpu":
                densities.append((float(s) - float(c)) / n)
    if not densities:
        return None, None
    return min(densities), max(densities)


def bridge_value_bounds(revalidation, demand_rows, horizon):
    if revalidation.get("status") != "NoLocalCrossover":
        return {
            "decision": "not-applicable",
            "continue": False,
            "reason": "bridge-not-applicable",
        }

    sentinels = list(revalidation.get("observed_sentinels") or [])
    consistency, direction = BRIDGE.V1.direction_consistency(sentinels)
    if not sentinels or direction is None:
        return {
            "decision": "unresolved",
            "continue": False,
            "reason": "no-observed-sentinels",
        }

    edge = (
        max(int(x["work_items"]) for x in sentinels)
        if direction == "Serial"
        else min(int(x["work_items"]) for x in sentinels)
    )
    target = BRIDGE.next_demand_target(demand_rows, direction, edge)
    if target is None:
        return {
            "decision": "stop",
            "continue": False,
            "reason": "no-unresolved-demand-target",
        }

    target_n = int(target["work_items"])
    steps = BRIDGE.geometric_bridge_steps(edge, target_n, direction)
    probe_cost = BRIDGE.V1.estimate_next_probe_cost(revalidation)
    bridge_cost = probe_cost * steps
    call_fraction = BRIDGE.target_call_fraction(demand_rows, target_n)

    density_lb, density_ub = directional_density_bounds(sentinels, direction)
    if density_lb is None or density_ub is None:
        return {
            "decision": "unresolved",
            "continue": False,
            "reason": "missing-repeat-samples",
            "estimated_bridge_cost_ns": bridge_cost,
        }

    target_regret_lb = density_lb * target_n
    target_regret_ub = density_ub * target_n
    gross_lb = consistency * call_fraction * target_regret_lb * float(horizon)
    gross_ub = consistency * call_fraction * target_regret_ub * float(horizon)
    net_lb = gross_lb - bridge_cost
    net_ub = gross_ub - bridge_cost

    if net_lb > 0.0 and steps > 0:
        decision = "continue"
    elif net_ub < 0.0 or steps == 0:
        decision = "stop"
    else:
        decision = "unresolved"

    return {
        "decision": decision,
        "continue": decision == "continue",
        "reason": f"empirical-bound-{decision}",
        "direction": direction,
        "direction_consistency": consistency,
        "observed_edge": edge,
        "target_work_items": target_n,
        "bridge_steps": steps,
        "target_call_fraction": call_fraction,
        "estimated_bridge_cost_ns": bridge_cost,
        "regret_density_lower_bound_ns_per_item": density_lb,
        "regret_density_upper_bound_ns_per_item": density_ub,
        "net_value_lower_bound_ns": net_lb,
        "net_value_upper_bound_ns": net_ub,
    }


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sensitivity_root", type=pathlib.Path)
    parser.add_argument("main_root", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=[100, 1_000, 10_000])
    args = parser.parse_args()

    evidence = load_jsonl(args.main_root / "evidence.jsonl")
    demand_by_regime = {}
    for row in evidence:
        if row.get("record_type") == "point":
            demand_by_regime.setdefault(row["regime"], []).append(row)

    output = {
        "schema_version": 1,
        "label": "demand-aware-bridge-empirical-bounds-v1",
        "warning": (
            "Bounds are empirical envelopes over already-observed paired repeats, "
            "not calibrated statistical confidence intervals."
        ),
        "cases": [],
    }

    for path in sorted(args.sensitivity_root.glob("*-p*.jsonl")):
        rows = load_jsonl(path)
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
            case["horizons"][str(horizon)] = bridge_value_bounds(
                reval, demand_by_regime.get(regime, []), horizon
            )
        output["cases"].append(case)

    target = args.main_root / "demand-aware-bridge-bounds.json"
    target.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
