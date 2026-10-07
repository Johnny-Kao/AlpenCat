#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib
import statistics


def median(values):
    xs = sorted(float(x) for x in values)
    return xs[len(xs) // 2]


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def baseline_features(machine_root, workload_dir):
    baseline = machine_root / workload_dir / "baseline-full.jsonl"
    rows = load_jsonl(baseline)
    points = [r for r in rows if r.get("record_type") == "point"]
    cpu_available = [r for r in points if r.get("cpu_samples_ns") is not None]
    winners = []
    for r in cpu_available:
        serial = median(r["serial_samples_ns"])
        cpu = median(r["cpu_samples_ns"])
        winners.append("Cpu" if cpu < serial else "Serial")
    cpu_wins = sum(w == "Cpu" for w in winners)
    serial_wins = sum(w == "Serial" for w in winners)
    return {
        "baseline_cpu_route_available": bool(cpu_available),
        "baseline_cpu_ever_wins": cpu_wins > 0,
        "baseline_serial_ever_wins": serial_wins > 0,
        "baseline_mixed_preference": cpu_wins > 0 and serial_wins > 0,
        "baseline_cpu_win_points": cpu_wins,
        "baseline_serial_win_points": serial_wins,
    }


def load_cases(root):
    csv_path = root / "cross-machine-cases.csv"
    with csv_path.open(newline="") as f:
        rows = list(csv.DictReader(f))

    feature_map = {}
    workload_map = {
        "compute-mix-128": "w1",
        "memory-column-transform-u64": "w2",
        "mixed-gather-mix-u64": "w3",
    }
    machine_roots = {}
    for manifest in root.rglob("hardware-manifest.json"):
        machine_roots[manifest.parent.name] = manifest.parent

    for row in rows:
        machine = row["machine_id"]
        workload = row["workload"]
        key = (machine, workload)
        if key not in feature_map:
            mroot = machine_roots.get(machine)
            wdir = workload_map.get(workload)
            if mroot is None or wdir is None:
                feature_map[key] = {}
            else:
                feature_map[key] = baseline_features(mroot, wdir)
        row.update(feature_map[key])
    return rows


def opp(row):
    return float(row["recoverable_ns"]) > 0.0


def cpus(row):
    return int(float(row["logical_cpus"]))


def v1(row):
    return cpus(row) > 1 and row["regime"] != "baseline-full"


def evaluate(rows, name, family, eligible_fn):
    eligible = [r for r in rows if eligible_fn(r)]
    skipped = [r for r in rows if not eligible_fn(r)]
    total_opp = sum(opp(r) for r in rows)
    retained = sum(opp(r) for r in eligible)
    no_opp_total = len(rows) - total_opp
    skipped_no_opp = sum(not opp(r) for r in skipped)
    eligible_no_opp = sum(not opp(r) for r in eligible)
    return {
        "gate": name,
        "family": family,
        "eligible_case_count": len(eligible),
        "skipped_case_count": len(skipped),
        "skip_fraction": len(skipped) / len(rows) if rows else 0.0,
        "total_opportunity_cases": total_opp,
        "retained_opportunity_cases": retained,
        "missed_opportunity_cases": total_opp - retained,
        "opportunity_recall": retained / total_opp if total_opp else 1.0,
        "total_no_opportunity_cases": no_opp_total,
        "skipped_no_opportunity_cases": skipped_no_opp,
        "no_opportunity_prune_fraction": (
            skipped_no_opp / no_opp_total if no_opp_total else 0.0
        ),
        "eligible_no_opportunity_cases": eligible_no_opp,
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("root", type=pathlib.Path)
    p.add_argument("--out", type=pathlib.Path, required=True)
    args = p.parse_args()

    rows = load_cases(args.root)

    candidates = [
        ("v1", "generic", lambda r: v1(r)),
        (
            "v1+baseline-cpu-ever-wins",
            "generic-history",
            lambda r: v1(r) and bool(r.get("baseline_cpu_ever_wins")),
        ),
        (
            "v1+baseline-mixed-preference",
            "generic-history",
            lambda r: v1(r) and bool(r.get("baseline_mixed_preference")),
        ),
        (
            "v1+baseline-cpu-route-available",
            "generic-history",
            lambda r: v1(r) and bool(r.get("baseline_cpu_route_available")),
        ),
        (
            "v1+exclude-recovery",
            "diagnostic-transition",
            lambda r: v1(r) and r["regime"] != "recovery",
        ),
        (
            "v1+cpu-or-combined",
            "diagnostic-transition",
            lambda r: v1(r) and r["regime"] in {"cpu-pressure", "combined"},
        ),
        (
            "v1+memory-or-combined",
            "diagnostic-transition",
            lambda r: v1(r) and r["regime"] in {"memory-light", "memory-heavy", "combined"},
        ),
        (
            "v1+baseline-cpu-ever-wins+exclude-recovery",
            "diagnostic-combination",
            lambda r: (
                v1(r)
                and bool(r.get("baseline_cpu_ever_wins"))
                and r["regime"] != "recovery"
            ),
        ),
        (
            "v1+baseline-mixed+exclude-recovery",
            "diagnostic-combination",
            lambda r: (
                v1(r)
                and bool(r.get("baseline_mixed_preference"))
                and r["regime"] != "recovery"
            ),
        ),
    ]

    results = [evaluate(rows, name, family, fn) for name, family, fn in candidates]
    results.sort(
        key=lambda x: (
            -x["opportunity_recall"],
            -x["no_opportunity_prune_fraction"],
            x["eligible_case_count"],
        )
    )

    output = {
        "schema_version": 1,
        "purpose": "Batch-evaluate multiple eligibility layers on one evidence sweep.",
        "guardrail": (
            "Generic-history candidates use only pre-existing baseline execution history. "
            "Diagnostic transition candidates are for falsification/understanding and must "
            "not be promoted solely because they score well on this dataset."
        ),
        "results": results,
    }
    args.out.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")

    print("| Gate | Family | Recall | No-op prune | Eligible | Missed opp |")
    print("|---|---|---:|---:|---:|---:|")
    for r in results:
        print(
            f"| {r['gate']} | {r['family']} | "
            f"{100*r['opportunity_recall']:.1f}% | "
            f"{100*r['no_opportunity_prune_fraction']:.1f}% | "
            f"{r['eligible_case_count']} | {r['missed_opportunity_cases']} |"
        )


if __name__ == "__main__":
    main()
