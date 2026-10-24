#!/usr/bin/env python3
"""Paired, order-balanced *real-process* CPU and wall economics.
No synthetic speedup assertions. Failed economic gates are recorded as NO_GO."""
import argparse
import csv
import json
import os
import random
import resource
import statistics
import subprocess
import time

MODES = ("static", "per_chunk", "periodic", "cached")
SCENARIOS = ("light", "moderate")
ROUNDS = 7

def run(binary, mode, scenario):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.perf_counter_ns()
    proc = subprocess.run([binary, mode, scenario], text=True, capture_output=True, check=True)
    external_wall_s = (time.perf_counter_ns() - start) / 1e9
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu_s = (after.ru_utime + after.ru_stime) - (before.ru_utime + before.ru_stime)
    lines = [line for line in proc.stdout.splitlines() if line.startswith("WORKLOAD_RESULT ")]
    if len(lines) != 1:
        raise RuntimeError(f"expected workload result: {mode} {scenario}: {proc.stdout} / {proc.stderr}")
    fields = dict(p.split("=", 1) for p in lines[0].split()[1:])
    if fields["mode"] != mode or fields["scenario"] != scenario:
        raise RuntimeError(f"unexpected result labels: {fields}")
    return dict(mode=mode, scenario=scenario,
                checksum=fields["checksum"], grain_sum=int(fields["policy_grain_sum"]),
                observations=int(fields["observations"]), events=int(fields["invalidations"]),
                wall_s=int(fields["wall_ns"]) / 1e9, # inside process, excluding startup
                external_wall_s=external_wall_s,
                cpu_s=cpu_s)

def median(xs):
    return statistics.median(xs)

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--binary", required=True)
    p.add_argument("--output", default="workload-economics.json")
    args = p.parse_args()
    rng = random.Random(20261024)
    rows = []
    for scenario in SCENARIOS:
        # Warmup each mode once, outside measurement.
        for mode in MODES:
            run(args.binary, mode, scenario)
        for round_id in range(ROUNDS):
            order = list(MODES)
            rng.shuffle(order)
            group = []
            for mode in order:
                row = run(args.binary, mode, scenario)
                row["round"] = round_id
                rows.append(row)
                group.append(row)
            if len({x["checksum"] for x in group}) != 1:
                raise AssertionError(f"non-equivalent workload outputs: {scenario} round={round_id}: {group}")
    report = {"experiment": "resource-lifecycle-workload-v1",
              "runner": {"os": os.uname().sysname, "release": os.uname().release,
                         "github_run_id": os.environ.get("GITHUB_RUN_ID", "unknown")},
              "rounds": ROUNDS,
              "scenarios": {},
              "raw": rows}
    for scenario in SCENARIOS:
        subset = [r for r in rows if r["scenario"] == scenario]
        by_mode = {m: [r for r in subset if r["mode"] == m] for m in MODES}
        medians = {m: {field: median([x[field] for x in by_mode[m]])
                       for field in ("wall_s", "cpu_s", "external_wall_s")} for m in MODES}
        comparisons = {}
        for baseline in ("per_chunk", "periodic", "static"):
            gains = {}
            for field in ("wall_s", "cpu_s"):
                pairs = []
                for i in range(ROUNDS):
                    a = next(x[field] for x in by_mode[baseline] if x["round"] == i)
                    b = next(x[field] for x in by_mode["cached"] if x["round"] == i)
                    pairs.append((a - b) / a * 100 if a > 0 else float("-inf"))
                gains[field] = {
                    "median_gain_pct": median(pairs),
                    "positive_rounds_over_2pct": sum(n > 2 for n in pairs),
                    "paired_gains_pct": [round(v, 2) for v in pairs],
                    "stable_positive": sum(n > 2 for n in pairs) >= 6
                }
            comparisons[baseline] = gains
        economics = {
            "gain_vs_every_chunk_observation": all(comparisons["per_chunk"][field]["stable_positive"] for field in ("wall_s","cpu_s")),
            "gain_vs_efficient_periodic_observation": all(comparisons["periodic"][field]["stable_positive"] for field in ("wall_s","cpu_s")),
            "gain_vs_static_policy": all(comparisons["static"][field]["stable_positive"] for field in ("wall_s","cpu_s")),
        }
        economics["robust_net_workload_gain"] = (
            economics["gain_vs_efficient_periodic_observation"]
            and economics["gain_vs_static_policy"]
        )
        report["scenarios"][scenario] = {
            "median_seconds": medians,
            "paired_comparisons_cached_vs": comparisons,
            "decision": economics,
            "checksum": by_mode["static"][0]["checksum"]
        }
        print(f"SCENARIO {scenario} median_wall_s " +
              " ".join(f"{mode}={medians[mode]['wall_s']:.6f}" for mode in MODES))
        print(f"SCENARIO {scenario} median_cpu_s " +
              " ".join(f"{mode}={medians[mode]['cpu_s']:.6f}" for mode in MODES))
        for b in comparisons:
            wall = comparisons[b]["wall_s"]
            cpu = comparisons[b]["cpu_s"]
            print(f"PAIRED_GAIN cached_vs={b} scenario={scenario} "
                  f"wall_median_pct={wall['median_gain_pct']:.2f} "
                  f"cpu_median_pct={cpu['median_gain_pct']:.2f} "
                  f"wall_positive={wall['positive_rounds_over_2pct']}/{ROUNDS} "
                  f"cpu_positive={cpu['positive_rounds_over_2pct']}/{ROUNDS}")
        print(f"VERDICT scenario={scenario} robust_net_gain={economics['robust_net_workload_gain']}")
    report["overall_stable_net_workload_gain"] = all(
        x["decision"]["robust_net_workload_gain"] for x in report["scenarios"].values()
    )
    print("OVERALL_STABLE_NET_WORKLOAD_GAIN=" + str(report["overall_stable_net_workload_gain"]).upper())
    print("NOTE: a failed economic gate is a NO_GO finding, not a CI infrastructure failure.")
    with open(args.output, "w", encoding="utf-8") as out:
        json.dump(report, out, indent=2, allow_nan=False)
    with open(args.output.replace(".json", ".csv"), "w", newline="", encoding="utf-8") as out:
        writer = csv.DictWriter(out, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)

if __name__ == "__main__":
    main()
