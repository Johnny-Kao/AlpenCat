#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib
import statistics


def load_json(path):
    return json.loads(path.read_text())


def main():
    p = argparse.ArgumentParser()
    p.add_argument("root", type=pathlib.Path)
    args = p.parse_args()

    rows = []
    machines = []
    for manifest_path in sorted(args.root.rglob("hardware-manifest.json")):
        machine_root = manifest_path.parent
        manifest = load_json(manifest_path)
        machine_id = machine_root.name
        machines.append({"machine_id": machine_id, **manifest})
        for workload in ("w1", "w2", "w3"):
            summary_path = machine_root / workload / "economics-summary.json"
            if not summary_path.exists():
                continue
            summary = load_json(summary_path)
            for workload_name, wdata in summary.get("workloads", {}).items():
                for regime, data in wdata.get("regimes", {}).items():
                    rows.append({
                        "machine_id": machine_id,
                        "runner_os": manifest.get("runner_os"),
                        "runner_arch": manifest.get("runner_arch"),
                        "logical_cpus": manifest.get("logical_cpus"),
                        "total_memory_gib": manifest.get("total_memory_gib"),
                        "workload": workload_name,
                        "regime": regime,
                        "savings_pct": float(data.get("alpencat_savings_pct_vs_static", 0.0)),
                        "oracle_capture_fraction": float(data.get("oracle_capture_fraction", 0.0)),
                        "recoverable_ns": float(data.get("recoverable_ns_vs_static", 0.0)),
                        "recovered_ns": float(data.get("recovered_ns_by_alpencat", 0.0)),
                    })

    if not rows:
        raise SystemExit("no cross-machine economics rows found")

    positive = [r for r in rows if r["savings_pct"] > 0.0]
    negative = [r for r in rows if r["savings_pct"] < 0.0]
    opportunity = [r for r in rows if r["recoverable_ns"] > 0.0]
    captures = [r["oracle_capture_fraction"] for r in opportunity]

    summary = {
        "schema_version": 1,
        "machine_count": len(machines),
        "case_count": len(rows),
        "positive_case_fraction": len(positive) / len(rows),
        "negative_case_fraction": len(negative) / len(rows),
        "median_savings_pct": statistics.median(r["savings_pct"] for r in rows),
        "worst_savings_pct": min(r["savings_pct"] for r in rows),
        "best_savings_pct": max(r["savings_pct"] for r in rows),
        "opportunity_case_count": len(opportunity),
        "median_oracle_capture_fraction_when_opportunity_exists": (
            statistics.median(captures) if captures else 0.0
        ),
        "machines": machines,
    }

    out = args.root / "cross-machine-summary.json"
    out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")

    csv_path = args.root / "cross-machine-cases.csv"
    with csv_path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)

    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
