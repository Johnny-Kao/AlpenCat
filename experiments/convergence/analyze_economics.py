#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib
import statistics
from collections import defaultdict


def median(values):
    return float(statistics.median(values))


def derive_boundary(points):
    ordered = sorted(points, key=lambda row: row["work_items"])
    previous = 0
    for row in ordered:
        cpu = row.get("cpu_samples_ns")
        if cpu is None:
            previous = row["work_items"]
            continue
        if median(cpu) < median(row["serial_samples_ns"]):
            return previous
        previous = row["work_items"]
    return ordered[-1]["work_items"]


def selected_cost(row, boundary):
    serial = median(row["serial_samples_ns"])
    cpu_samples = row.get("cpu_samples_ns")
    if row["work_items"] <= boundary or cpu_samples is None:
        return serial, "Serial"
    return median(cpu_samples), "Cpu"


def oracle_cost(row):
    serial = median(row["serial_samples_ns"])
    cpu = row.get("cpu_samples_ns")
    if cpu is None:
        return serial, "Serial"
    cpu_cost = median(cpu)
    if serial <= cpu_cost:
        return serial, "Serial"
    return cpu_cost, "Cpu"


def load_records(path):
    records = []
    for source in sorted(path.glob("*.jsonl")):
        if source.name == "evidence.jsonl":
            continue
        for line in source.read_text().splitlines():
            line = line.strip()
            if line:
                records.append(json.loads(line))
    if not records:
        raise SystemExit("no JSONL evidence records")
    return records


def analyze(records, calls_per_point):
    points = [row for row in records if row.get("record_type") == "point"]
    revalidation = {
        (row["workload"], row["regime"]): row
        for row in records
        if row.get("record_type") == "revalidation"
    }

    grouped = defaultdict(list)
    for row in points:
        grouped[(row["workload"], row["regime"])].append(row)

    workloads = sorted({key[0] for key in grouped})
    regimes = sorted({key[1] for key in grouped})
    summary = {
        "schema_version": 1,
        "calls_per_point": calls_per_point,
        "workloads": {},
    }

    for workload in workloads:
        baseline_key = (workload, "baseline-full")
        if baseline_key not in grouped:
            raise SystemExit(f"missing baseline-full for {workload}")
        static_boundary = derive_boundary(grouped[baseline_key])
        workload_summary = {
            "static_boundary": static_boundary,
            "regimes": {},
        }

        for regime in regimes:
            key = (workload, regime)
            if key not in grouped:
                continue

            rows = sorted(grouped[key], key=lambda row: row["work_items"])
            periodic_boundary = derive_boundary(rows)

            execution = {
                "Static": 0.0,
                "Periodic": 0.0,
                "AlpenCat": 0.0,
                "Oracle": 0.0,
            }
            route_counts = {name: {"Serial": 0, "Cpu": 0} for name in execution}

            for row in rows:
                weight = float(row.get("weight", 1.0)) * calls_per_point

                static_cost, static_route = selected_cost(row, static_boundary)
                periodic_cost, periodic_route = selected_cost(row, periodic_boundary)
                oracle, oracle_route = oracle_cost(row)
                alpencat = median(row["auto_samples_ns"])
                alpencat_route = row["auto_backend"]

                for name, cost, route in (
                    ("Static", static_cost, static_route),
                    ("Periodic", periodic_cost, periodic_route),
                    ("AlpenCat", alpencat, alpencat_route),
                    ("Oracle", oracle, oracle_route),
                ):
                    execution[name] += cost * weight
                    route_counts[name][route] = route_counts[name].get(route, 0) + 1

            periodic_calibration = sum(
                median(row["serial_samples_ns"])
                + (median(row["cpu_samples_ns"]) if row.get("cpu_samples_ns") else 0.0)
                for row in rows
            )
            reval = revalidation.get(key, {})
            alpencat_calibration = float(reval.get("revalidation_elapsed_ns", 0.0))

            totals = {
                "Static": execution["Static"],
                "Periodic": execution["Periodic"] + periodic_calibration,
                "AlpenCat": execution["AlpenCat"] + alpencat_calibration,
                "Oracle": execution["Oracle"],
            }
            static_total = totals["Static"]
            oracle_total = totals["Oracle"]
            recoverable = max(0.0, static_total - oracle_total)
            recovered = max(0.0, static_total - totals["AlpenCat"])
            oracle_capture = 0.0 if recoverable <= 0 else recovered / recoverable

            workload_summary["regimes"][regime] = {
                "periodic_boundary": periodic_boundary,
                "alpencat_status": reval.get("status"),
                "alpencat_boundary": reval.get("published_serial_max_items"),
                "execution_ns": execution,
                "calibration_ns": {
                    "Static": 0.0,
                    "Periodic": periodic_calibration,
                    "AlpenCat": alpencat_calibration,
                    "Oracle": 0.0,
                },
                "total_ns": totals,
                "route_counts": route_counts,
                "recoverable_ns_vs_static": recoverable,
                "recovered_ns_by_alpencat": recovered,
                "oracle_capture_fraction": oracle_capture,
                "alpencat_savings_pct_vs_static": (
                    0.0
                    if static_total <= 0
                    else 100.0 * (static_total - totals["AlpenCat"]) / static_total
                ),
                "periodic_savings_pct_vs_static": (
                    0.0
                    if static_total <= 0
                    else 100.0 * (static_total - totals["Periodic"]) / static_total
                ),
            }

        summary["workloads"][workload] = workload_summary

    return summary


def write_csv(summary, path):
    fields = [
        "workload",
        "regime",
        "static_boundary",
        "periodic_boundary",
        "alpencat_status",
        "alpencat_boundary",
        "static_total_ns",
        "periodic_total_ns",
        "alpencat_total_ns",
        "oracle_total_ns",
        "recoverable_ns_vs_static",
        "recovered_ns_by_alpencat",
        "oracle_capture_fraction",
        "alpencat_savings_pct_vs_static",
        "periodic_savings_pct_vs_static",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for workload, data in summary["workloads"].items():
            for regime, row in data["regimes"].items():
                writer.writerow(
                    {
                        "workload": workload,
                        "regime": regime,
                        "static_boundary": data["static_boundary"],
                        "periodic_boundary": row["periodic_boundary"],
                        "alpencat_status": row["alpencat_status"],
                        "alpencat_boundary": row["alpencat_boundary"],
                        "static_total_ns": row["total_ns"]["Static"],
                        "periodic_total_ns": row["total_ns"]["Periodic"],
                        "alpencat_total_ns": row["total_ns"]["AlpenCat"],
                        "oracle_total_ns": row["total_ns"]["Oracle"],
                        "recoverable_ns_vs_static": row["recoverable_ns_vs_static"],
                        "recovered_ns_by_alpencat": row["recovered_ns_by_alpencat"],
                        "oracle_capture_fraction": row["oracle_capture_fraction"],
                        "alpencat_savings_pct_vs_static": row["alpencat_savings_pct_vs_static"],
                        "periodic_savings_pct_vs_static": row["periodic_savings_pct_vs_static"],
                    }
                )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=pathlib.Path)
    parser.add_argument(
        "--calls-per-point",
        type=int,
        default=1,
        help="Uniform demand multiplier used only for amortization smoke analysis.",
    )
    args = parser.parse_args()
    if args.calls_per_point < 1:
        raise SystemExit("--calls-per-point must be >= 1")

    records = load_records(args.root)
    summary = analyze(records, args.calls_per_point)
    (args.root / "economics-summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )
    write_csv(summary, args.root / "policy-summary.csv")

    print("# AlpenCat E0 policy economics")
    print()
    print(
        f"Demand assumption for this smoke: {args.calls_per_point} call(s) per measured grid point."
    )
    print("This is an experiment-harness check, not a fleet economics claim.")
    print()
    print("| Workload | Regime | Static | Periodic | AlpenCat | Oracle | AlpenCat vs static | Oracle capture |")
    print("|---|---|---:|---:|---:|---:|---:|---:|")
    for workload, data in summary["workloads"].items():
        for regime, row in data["regimes"].items():
            totals = row["total_ns"]
            print(
                f"| {workload} | {regime} | "
                f"{totals['Static'] / 1e6:.3f} ms | "
                f"{totals['Periodic'] / 1e6:.3f} ms | "
                f"{totals['AlpenCat'] / 1e6:.3f} ms | "
                f"{totals['Oracle'] / 1e6:.3f} ms | "
                f"{row['alpencat_savings_pct_vs_static']:.3f}% | "
                f"{100.0 * row['oracle_capture_fraction']:.1f}% |"
            )


if __name__ == "__main__":
    main()
