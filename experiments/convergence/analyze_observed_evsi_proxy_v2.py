#!/usr/bin/env python3
import argparse
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "v1", HERE / "analyze_observed_evsi_proxy.py"
)
V1 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(V1)

DEFAULT_HORIZONS = (100, 1_000, 10_000)


def unresolved_work_mass(demand_rows, direction, observed_edge):
    total = sum(float(row.get("weight", 1.0)) * int(row["work_items"]) for row in demand_rows)
    if total <= 0.0:
        return 0.0
    if direction == "Serial":
        unresolved = sum(
            float(row.get("weight", 1.0)) * int(row["work_items"])
            for row in demand_rows
            if int(row["work_items"]) > observed_edge
        )
    elif direction == "Cpu":
        unresolved = sum(
            float(row.get("weight", 1.0)) * int(row["work_items"])
            for row in demand_rows
            if int(row["work_items"]) < observed_edge
        )
    else:
        return 0.0
    return unresolved / total


def evsi_proxy_v2(revalidation, demand_rows, horizon):
    sentinels = list(revalidation.get("observed_sentinels") or [])
    consistency, direction = V1.direction_consistency(sentinels)
    if not sentinels or direction is None:
        return {"continue": False, "evsi_proxy_ns": 0.0, "reason": "no-observed-sentinels"}

    edge = (
        max(int(x["work_items"]) for x in sentinels)
        if direction == "Serial"
        else min(int(x["work_items"]) for x in sentinels)
    )
    exposure = unresolved_work_mass(demand_rows, direction, edge)
    observed_regret = max((V1.regret_ns(x) for x in sentinels), default=0.0)
    next_probe_cost = V1.estimate_next_probe_cost(revalidation)

    gross = consistency * exposure * observed_regret * float(horizon)
    net = gross - next_probe_cost
    return {
        "continue": net > 0.0 and exposure > 0.0,
        "evsi_proxy_ns": net,
        "gross_information_value_ns": gross,
        "estimated_next_probe_cost_ns": next_probe_cost,
        "direction": direction,
        "direction_consistency": consistency,
        "observed_edge": edge,
        "unresolved_work_fraction": exposure,
        "max_observed_route_regret_ns": observed_regret,
        "reason": "positive-evsi" if net > 0.0 and exposure > 0.0 else "nonpositive-evsi",
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
        "label": "observed-only-evsi-proxy-v2-work-exposure",
        "warning": (
            "Research-only proxy. Uses only already-observed sentinel measurements, "
            "known demand sizes/weights, and measured probe cost. Work-item weighting "
            "assumes unresolved execution exposure grows with request size and must be "
            "validated across workload families."
        ),
        "cases": [],
    }

    for path in sorted(args.sensitivity_root.glob("*-p*.jsonl")):
        rows = V1.load_jsonl(path)
        reval = next((row for row in rows if row.get("record_type") == "revalidation"), None)
        if reval is None:
            continue
        regime = reval["regime"]
        case = {"regime": regime, "max_points": int(path.stem.split("-p")[-1]), "horizons": {}}
        for horizon in args.horizons:
            case["horizons"][str(horizon)] = evsi_proxy_v2(
                reval, demand_by_regime.get(regime, []), horizon
            )
        output["cases"].append(case)

    target = args.main_root / "observed-only-evsi-proxy-v2.json"
    target.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
