#!/usr/bin/env python3
import argparse
import csv
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "adaptation", HERE / "analyze_adaptation_economics.py"
)
ADAPT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ADAPT)

MAX_ITEMS = 4_194_304


def infer_one_sided_candidate(start_boundary, measurement_count, direction):
    if measurement_count <= 0:
        return start_boundary
    if direction == "Serial":
        return min(MAX_ITEMS, start_boundary * (2 ** (measurement_count - 1)))
    if direction == "Cpu":
        value = start_boundary
        for _ in range(measurement_count - 1):
            value = max(4_096, value // 2)
        return value
    return start_boundary


def winner_at_boundary(points, n):
    nearest = min(points, key=lambda row: abs(int(row["work_items"]) - int(n)))
    return ADAPT.oracle_cost(nearest)[1]


def cycle_cost(points, boundary):
    total = 0.0
    for row in points:
        weight = float(row.get("weight", 1.0))
        cost, _ = ADAPT.route_cost(row, boundary)
        total += cost * weight
    return total


def analyze(main_root, sensitivity_root, horizons):
    records = ADAPT.load_directory(main_root)
    points = [row for row in records if row.get("record_type") == "point"]
    grouped = {}
    for row in points:
        grouped.setdefault(row["regime"], []).append(row)

    static_boundary, _ = ADAPT.derive_boundary(grouped["baseline-full"])
    out = {"schema_version": 1, "static_boundary": static_boundary, "cases": []}

    for path in sorted(sensitivity_root.glob("half-p*.jsonl")) + sorted(
        sensitivity_root.glob("contention-p*.jsonl")
    ):
        rows = ADAPT.load_jsonl(path)
        reval = next(row for row in rows if row.get("record_type") == "revalidation")
        regime = reval["regime"]
        budget = int(path.stem.split("-p")[-1])
        main_points = grouped[regime]

        if reval.get("status") != "NoLocalCrossover":
            continue

        direction = winner_at_boundary(main_points, int(reval["start_boundary"]))
        candidate = infer_one_sided_candidate(
            int(reval["start_boundary"]),
            int(reval["measurement_count"]),
            direction,
        )
        static_cycle = cycle_cost(main_points, static_boundary)
        candidate_cycle = cycle_cost(main_points, candidate)
        oracle_cycle = sum(
            ADAPT.oracle_cost(row)[0] * float(row.get("weight", 1.0))
            for row in main_points
        )
        total_weight = sum(float(row.get("weight", 1.0)) for row in main_points)
        gain_per_call = max(0.0, static_cycle - candidate_cycle) / total_weight
        opportunity_per_call = max(0.0, static_cycle - oracle_cycle) / total_weight
        probe_cost = float(reval.get("revalidation_elapsed_ns", 0.0))

        horizon_rows = {}
        for horizon in horizons:
            net = gain_per_call * horizon - probe_cost
            horizon_rows[str(horizon)] = {
                "net_gain_ns": net,
                "profitable": net > 0.0,
            }

        out["cases"].append({
            "regime": regime,
            "max_points": budget,
            "status": reval.get("status"),
            "direction": direction,
            "start_boundary": int(reval["start_boundary"]),
            "sentinel_candidate_boundary": candidate,
            "probe_cost_ns": probe_cost,
            "candidate_gain_per_call_ns": gain_per_call,
            "opportunity_gain_per_call_ns": opportunity_per_call,
            "capture_fraction": (
                0.0 if opportunity_per_call <= 0.0
                else min(1.0, gain_per_call / opportunity_per_call)
            ),
            "horizons": horizon_rows,
        })
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("main_root", type=pathlib.Path)
    parser.add_argument("sensitivity_root", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=[100, 1_000, 10_000])
    args = parser.parse_args()

    result = analyze(args.main_root, args.sensitivity_root, tuple(args.horizons))
    target = args.main_root / "sentinel-escalation.json"
    target.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")

    fields = [
        "regime", "max_points", "direction", "start_boundary",
        "sentinel_candidate_boundary", "probe_cost_ns",
        "candidate_gain_per_call_ns", "opportunity_gain_per_call_ns",
        "capture_fraction",
    ]
    with (args.main_root / "sentinel-escalation.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in result["cases"]:
            writer.writerow({key: row[key] for key in fields})

    print(target)


if __name__ == "__main__":
    main()
