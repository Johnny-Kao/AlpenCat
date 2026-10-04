#!/usr/bin/env python3
import csv
import json
import math
import pathlib
import random
import statistics
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "results/x64-economics")
summary_path = root / "summary.json"
if not summary_path.exists():
    raise SystemExit("missing summary.json; run analyze.py first")

summary = json.loads(summary_path.read_text())
phases = {}
for path in sorted(root.glob("*.csv")):
    with path.open() as f:
        rows = []
        for row in csv.DictReader(f):
            row["n"] = int(row["n"])
            row["serial_ns"] = float(row["serial_ns"])
            row["parallel_ns"] = float(row["parallel_ns"])
            rows.append(row)
        if rows:
            phases[rows[0]["phase"]] = rows

if "full" not in phases:
    raise SystemExit("missing full phase")

baseline_boundary = summary["baseline_boundary"]
if baseline_boundary is None:
    raise SystemExit("full phase has no parallel crossover; robustness replay requires a baseline boundary")

sizes = [r["n"] for r in phases["full"]]
baseline_idx = min(range(len(sizes)), key=lambda i: abs(sizes[i] - baseline_boundary))

def route(boundary, n):
    if boundary is None:
        return "SERIAL"
    return "PARALLEL" if n >= boundary else "SERIAL"

def regret_ns(row, boundary):
    oracle = min(row["serial_ns"], row["parallel_ns"])
    chosen = row["parallel_ns"] if route(boundary, row["n"]) == "PARALLEL" else row["serial_ns"]
    return max(0.0, chosen - oracle)

def choose_index(rng, distribution):
    if distribution == "uniform":
        return rng.randrange(len(sizes))
    if distribution == "near":
        lo = max(0, baseline_idx - 2)
        hi = min(len(sizes), baseline_idx + 3)
        return rng.randrange(lo, hi)
    if distribution == "small":
        hi = max(1, len(sizes) // 2)
        return rng.randrange(hi)
    if distribution == "large":
        lo = len(sizes) // 2
        return rng.randrange(lo, len(sizes))
    if distribution == "bimodal":
        if rng.random() < 0.5:
            return rng.randrange(max(1, len(sizes) // 3))
        lo = max(0, (2 * len(sizes)) // 3)
        return rng.randrange(lo, len(sizes))
    raise ValueError(distribution)

distributions = ["uniform", "near", "small", "large", "bimodal"]
seeds = list(range(1, 33))
calls_per_episode = 4096
records = []
localized_records = []

for phase_name, rows in phases.items():
    bounded_boundary = summary["phases"][phase_name]["bounded_policy"]["boundary"]
    for distribution in distributions:
        ratios = []
        static_totals = []
        bounded_totals = []
        wins = 0
        ties = 0
        losses = 0

        for seed in seeds:
            rng = random.Random((seed << 16) ^ sum(map(ord, phase_name)) ^ sum(map(ord, distribution)))
            static_loss = 0.0
            bounded_loss = 0.0

            for _ in range(calls_per_episode):
                idx = choose_index(rng, distribution)
                row = rows[idx]
                static_loss += regret_ns(row, baseline_boundary)
                bounded_loss += regret_ns(row, bounded_boundary)

            static_totals.append(static_loss)
            bounded_totals.append(bounded_loss)

            if bounded_loss + 1e-9 < static_loss:
                wins += 1
            elif static_loss + 1e-9 < bounded_loss:
                losses += 1
            else:
                ties += 1

            if static_loss > 0:
                ratios.append(bounded_loss / static_loss)
            elif bounded_loss == 0:
                ratios.append(1.0)
            else:
                ratios.append(math.inf)

        finite = [x for x in ratios if math.isfinite(x)]
        record = {
            "phase": phase_name,
            "distribution": distribution,
            "episodes": len(seeds),
            "calls_per_episode": calls_per_episode,
            "wins": wins,
            "ties": ties,
            "losses": losses,
            "median_loss_ratio": statistics.median(finite) if finite else None,
            "max_loss_ratio": max(finite) if finite else None,
            "median_static_loss_ns": statistics.median(static_totals),
            "median_bounded_loss_ns": statistics.median(bounded_totals),
        }
        records.append(record)

        tested_points = set(summary["phases"][phase_name]["bounded_policy"]["tested_points"])
        local_wins = local_ties = local_losses = 0
        local_ratios = []
        for seed in seeds:
            rng = random.Random((seed << 16) ^ sum(map(ord, phase_name)) ^ sum(map(ord, distribution)))
            static_loss = 0.0
            localized_loss = 0.0
            for _ in range(calls_per_episode):
                idx = choose_index(rng, distribution)
                row = rows[idx]
                static = regret_ns(row, baseline_boundary)
                static_loss += static
                if row["n"] in tested_points:
                    localized = 0.0
                else:
                    localized = static
                localized_loss += localized

            if localized_loss + 1e-9 < static_loss:
                local_wins += 1
            elif static_loss + 1e-9 < localized_loss:
                local_losses += 1
            else:
                local_ties += 1

            if static_loss > 0:
                local_ratios.append(localized_loss / static_loss)
            elif localized_loss == 0:
                local_ratios.append(1.0)
            else:
                local_ratios.append(math.inf)

        local_finite = [x for x in local_ratios if math.isfinite(x)]
        localized_records.append({
            "phase": phase_name,
            "distribution": distribution,
            "episodes": len(seeds),
            "calls_per_episode": calls_per_episode,
            "wins": local_wins,
            "ties": local_ties,
            "losses": local_losses,
            "median_loss_ratio": statistics.median(local_finite) if local_finite else None,
            "tested_points": sorted(tested_points),
        })

(root / "robustness.json").write_text(json.dumps({
    "global_bounded": records,
    "localized_no_extrapolation": localized_records,
}, indent=2) + "\n")

print("# Randomized workload replay")
print()
print(f"Episodes: {len(seeds)} seeds x {calls_per_episode} calls x {len(distributions)} workload distributions")
print()
print("| Phase | Distribution | Win/Tie/Loss | Median bounded/static loss | Median static loss | Median bounded loss |")
print("|---|---|---:|---:|---:|---:|")
for r in records:
    ratio = "n/a" if r["median_loss_ratio"] is None else f"{r['median_loss_ratio']:.3f}x"
    print(
        f"| {r['phase']} | {r['distribution']} | "
        f"{r['wins']}/{r['ties']}/{r['losses']} | {ratio} | "
        f"{r['median_static_loss_ns']/1e6:.3f} ms | "
        f"{r['median_bounded_loss_ns']/1e6:.3f} ms |"
    )

loss_cases = [r for r in records if r["losses"] > 0]
local_loss_cases = [r for r in localized_records if r["losses"] > 0]
print()
print(f"Distributions with any global bounded-policy regression: {len(loss_cases)}/{len(records)}")
print(f"Distributions with any localized-policy regression: {len(local_loss_cases)}/{len(localized_records)}")
print()
print("## Localized evidence, no extrapolation")
print()
print("| Phase | Distribution | Win/Tie/Loss | Median localized/static loss |")
print("|---|---|---:|---:|")
for r in localized_records:
    ratio = "n/a" if r["median_loss_ratio"] is None else f"{r['median_loss_ratio']:.3f}x"
    print(
        f"| {r['phase']} | {r['distribution']} | "
        f"{r['wins']}/{r['ties']}/{r['losses']} | {ratio} |"
    )
