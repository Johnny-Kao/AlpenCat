#!/usr/bin/env python3
import argparse
import json
import pathlib


DEFAULT_HORIZONS = (100, 1_000, 10_000)


def preferred(sentinel):
    s = sentinel.get("serial_cost_ns")
    c = sentinel.get("cpu_cost_ns")
    if s is None or c is None:
        return None
    return "Serial" if s <= c else "Cpu"


def regret_ns(sentinel):
    s = sentinel.get("serial_cost_ns")
    c = sentinel.get("cpu_cost_ns")
    if s is None or c is None:
        return 0.0
    return abs(float(s) - float(c))


def direction_consistency(sentinels):
    prefs = [preferred(x) for x in sentinels if preferred(x) is not None]
    if not prefs:
        return 0.0, None
    serial = sum(x == "Serial" for x in prefs)
    cpu = sum(x == "Cpu" for x in prefs)
    direction = "Serial" if serial >= cpu else "Cpu"
    return max(serial, cpu) / len(prefs), direction


def unresolved_mass(demand_rows, direction, observed_edge):
    total = sum(float(row.get("weight", 1.0)) for row in demand_rows)
    if total <= 0.0:
        return 0.0
    if direction == "Serial":
        unresolved = sum(
            float(row.get("weight", 1.0))
            for row in demand_rows
            if int(row["work_items"]) > observed_edge
        )
    elif direction == "Cpu":
        unresolved = sum(
            float(row.get("weight", 1.0))
            for row in demand_rows
            if int(row["work_items"]) < observed_edge
        )
    else:
        return 0.0
    return unresolved / total


def estimate_next_probe_cost(revalidation):
    count = max(1, int(revalidation.get("measurement_count", 1)))
    elapsed = float(revalidation.get("revalidation_elapsed_ns", 0.0))
    return elapsed / count


def evsi_proxy(revalidation, demand_rows, horizon):
    sentinels = list(revalidation.get("observed_sentinels") or [])
    consistency, direction = direction_consistency(sentinels)
    if not sentinels or direction is None:
        return {
            "continue": False,
            "evsi_proxy_ns": 0.0,
            "reason": "no-observed-sentinels",
        }

    edge = (
        max(int(x["work_items"]) for x in sentinels)
        if direction == "Serial"
        else min(int(x["work_items"]) for x in sentinels)
    )
    unresolved = unresolved_mass(demand_rows, direction, edge)
    observed_regret = max((regret_ns(x) for x in sentinels), default=0.0)
    next_probe_cost = estimate_next_probe_cost(revalidation)

    # Research-only online-safe proxy:
    # value = directional confidence * unresolved demand mass *
    #         largest directly observed route regret * remaining calls.
    gross_information_value = (
        consistency * unresolved * observed_regret * float(horizon)
    )
    net = gross_information_value - next_probe_cost
    return {
        "continue": net > 0.0 and unresolved > 0.0,
        "evsi_proxy_ns": net,
        "gross_information_value_ns": gross_information_value,
        "estimated_next_probe_cost_ns": next_probe_cost,
        "direction": direction,
        "direction_consistency": consistency,
        "observed_edge": edge,
        "unresolved_demand_fraction": unresolved,
        "max_observed_route_regret_ns": observed_regret,
        "reason": "positive-evsi" if net > 0.0 and unresolved > 0.0 else "nonpositive-evsi",
    }


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sensitivity_root", type=pathlib.Path)
    parser.add_argument("main_root", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS))
    args = parser.parse_args()

    demand_records = load_jsonl(args.main_root / "evidence.jsonl")
    demand_by_regime = {}
    for row in demand_records:
        if row.get("record_type") == "point":
            demand_by_regime.setdefault(row["regime"], []).append(row)

    output = {
        "schema_version": 1,
        "label": "observed-only-evsi-proxy-v1",
        "warning": (
            "Research-only proxy using only already-observed sentinel measurements, "
            "declared/observed demand weights, and measured probe cost. It is not a "
            "probabilistically calibrated EVSI model."
        ),
        "cases": [],
    }

    for path in sorted(args.sensitivity_root.glob("*-p*.jsonl")):
        rows = load_jsonl(path)
        reval = next(
            (row for row in rows if row.get("record_type") == "revalidation"),
            None,
        )
        if reval is None:
            continue
        regime = reval["regime"]
        demand_rows = demand_by_regime.get(regime, [])
        budget = int(path.stem.split("-p")[-1])
        case = {
            "regime": regime,
            "max_points": budget,
            "horizons": {},
        }
        for horizon in args.horizons:
            case["horizons"][str(horizon)] = evsi_proxy(
                reval, demand_rows, horizon
            )
        output["cases"].append(case)

    target = args.main_root / "observed-only-evsi-proxy.json"
    target.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
