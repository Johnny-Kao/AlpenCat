#!/usr/bin/env python3
import argparse
import csv
import json
import math
import pathlib
import statistics
from collections import defaultdict


def median(values):
    return float(statistics.median(values))


def load_records(root):
    records = []
    for path in sorted(root.glob("*.jsonl")):
        if path.name == "evidence.jsonl":
            continue
        for line in path.read_text().splitlines():
            line = line.strip()
            if line:
                records.append(json.loads(line))
    if not records:
        raise SystemExit(f"no JSONL evidence found under {root}")
    return records


def route_costs(row):
    serial = median(row["serial_samples_ns"])
    cpu_samples = row.get("cpu_samples_ns")
    cpu = None if cpu_samples is None else median(cpu_samples)
    return serial, cpu


def oracle_route(row):
    serial, cpu = route_costs(row)
    if cpu is not None and cpu < serial:
        return "Cpu", cpu
    return "Serial", serial


def weighted_cost(points, chooser):
    total = 0.0
    for row in points:
        route = chooser(row)
        serial, cpu = route_costs(row)
        cost = cpu if route == "Cpu" and cpu is not None else serial
        total += cost * float(row.get("weight", 1.0))
    return total


def best_scalar(points):
    sizes = sorted(row["work_items"] for row in points)
    best = None
    for boundary in [0] + sizes:
        cost = weighted_cost(
            points,
            lambda row, b=boundary: (
                "Serial"
                if row["work_items"] <= b or row.get("cpu_samples_ns") is None
                else "Cpu"
            ),
        )
        candidate = (cost, boundary)
        if best is None or candidate < best:
            best = candidate
    return {"cost_ns": best[0], "serial_max_items": best[1]}


def best_interval(points):
    sizes = sorted(row["work_items"] for row in points)
    best = None
    for lower in [0] + sizes:
        for upper in sizes + [math.inf]:
            if upper < lower:
                continue
            cost = weighted_cost(
                points,
                lambda row, lo=lower, hi=upper: (
                    "Cpu"
                    if row.get("cpu_samples_ns") is not None
                    and lo < row["work_items"] <= hi
                    else "Serial"
                ),
            )
            candidate = (cost, lower, upper)
            if best is None or candidate < best:
                best = candidate
    return {
        "cost_ns": best[0],
        "cpu_min_exclusive": best[1],
        "cpu_max_inclusive": None if math.isinf(best[2]) else int(best[2]),
    }


def analyze(records):
    grouped = defaultdict(list)
    for row in records:
        if row.get("record_type") == "point":
            grouped[row["regime"]].append(row)

    result = {"schema_version": 1, "regimes": {}}
    for regime in sorted(grouped):
        points = sorted(grouped[regime], key=lambda row: row["work_items"])
        sequence = [oracle_route(row)[0] for row in points]
        switches = sum(a != b for a, b in zip(sequence, sequence[1:]))
        oracle_cost = weighted_cost(points, lambda row: oracle_route(row)[0])
        scalar = best_scalar(points)
        interval = best_interval(points)
        scalar_gap = 0.0 if oracle_cost <= 0 else 100.0 * (scalar["cost_ns"] - oracle_cost) / oracle_cost
        interval_gap = 0.0 if oracle_cost <= 0 else 100.0 * (interval["cost_ns"] - oracle_cost) / oracle_cost
        scalar_excess = max(0.0, scalar["cost_ns"] - oracle_cost)
        recovered = max(0.0, scalar["cost_ns"] - interval["cost_ns"])

        result["regimes"][regime] = {
            "preference_sequence": sequence,
            "preference_switch_count": switches,
            "oracle_cost_ns": oracle_cost,
            "best_scalar": scalar,
            "best_interval": interval,
            "scalar_gap_to_oracle_pct": scalar_gap,
            "interval_gap_to_oracle_pct": interval_gap,
            "interval_capture_of_scalar_gap_fraction": 0.0 if scalar_excess <= 0 else recovered / scalar_excess,
            "interval_exact_on_measured_grid": abs(interval["cost_ns"] - oracle_cost) < 0.5,
        }
    return result


def write_csv(summary, path):
    fields = [
        "regime", "preference_sequence", "preference_switch_count",
        "best_scalar_boundary", "best_interval_cpu_min_exclusive",
        "best_interval_cpu_max_inclusive", "scalar_gap_to_oracle_pct",
        "interval_gap_to_oracle_pct", "interval_capture_of_scalar_gap_fraction",
        "interval_exact_on_measured_grid",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for regime, row in summary["regimes"].items():
            writer.writerow({
                "regime": regime,
                "preference_sequence": ">".join(row["preference_sequence"]),
                "preference_switch_count": row["preference_switch_count"],
                "best_scalar_boundary": row["best_scalar"]["serial_max_items"],
                "best_interval_cpu_min_exclusive": row["best_interval"]["cpu_min_exclusive"],
                "best_interval_cpu_max_inclusive": row["best_interval"]["cpu_max_inclusive"],
                "scalar_gap_to_oracle_pct": row["scalar_gap_to_oracle_pct"],
                "interval_gap_to_oracle_pct": row["interval_gap_to_oracle_pct"],
                "interval_capture_of_scalar_gap_fraction": row["interval_capture_of_scalar_gap_fraction"],
                "interval_exact_on_measured_grid": row["interval_exact_on_measured_grid"],
            })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("evidence_root", type=pathlib.Path)
    args = parser.parse_args()
    summary = analyze(load_records(args.evidence_root))
    (args.evidence_root / "profile-shape.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    write_csv(summary, args.evidence_root / "profile-shape.csv")

    lines = [
        "# W2 profile-shape analysis", "",
        "| Regime | Oracle preference | Switches | Best scalar | Scalar gap | Best CPU interval | Interval gap | Exact grid fit |",
        "|---|---|---:|---:|---:|---|---:|---:|",
    ]
    for regime, row in summary["regimes"].items():
        interval = row["best_interval"]
        upper = "inf" if interval["cpu_max_inclusive"] is None else str(interval["cpu_max_inclusive"])
        lines.append(
            f"| {regime} | {'→'.join(row['preference_sequence'])} | "
            f"{row['preference_switch_count']} | {row['best_scalar']['serial_max_items']} | "
            f"{row['scalar_gap_to_oracle_pct']:.3f}% | "
            f"({interval['cpu_min_exclusive']}, {upper}] | "
            f"{row['interval_gap_to_oracle_pct']:.3f}% | "
            f"{'yes' if row['interval_exact_on_measured_grid'] else 'no'} |"
        )
    report = "\n".join(lines) + "\n"
    (args.evidence_root / "profile-shape.md").write_text(report)
    print(report)


if __name__ == "__main__":
    main()
