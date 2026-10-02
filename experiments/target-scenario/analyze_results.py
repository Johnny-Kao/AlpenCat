#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

pat = re.compile(
    r"fir_result taps=(\d+) n=(\d+) iters=(\d+) cpu_ns=([0-9.]+) "
    r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
)

rows = defaultdict(list)
for path in sorted(glob.glob("results/*.txt")):
    base = os.path.basename(path)
    scenario = base.rsplit("_", 1)[0]
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            m = pat.search(line)
            if not m:
                continue
            taps, n, iters = map(int, m.group(1, 2, 3))
            cpu, gpu, gpu_dev = map(float, m.group(4, 5, 6))
            winner = m.group(7)
            rows[(scenario, taps, n)].append((cpu, gpu, gpu_dev, winner))

def med(vals):
    return statistics.median(vals)

summary = {}
print("# Controlled-load FIR summary")
print("| scenario | taps | n | reps | cpu_med_ns | gpu_med_ns | winner | winner_stability |")
print("|---|---:|---:|---:|---:|---:|---|---:|")
for key in sorted(rows):
    scenario, taps, n = key
    vals = rows[key]
    cpu = med([v[0] for v in vals])
    gpu = med([v[1] for v in vals])
    winner = "CPU" if cpu <= gpu else "GPU"
    stability = sum(v[3] == winner for v in vals) / len(vals)
    summary[key] = (cpu, gpu, winner, stability)
    print(
        f"| {scenario} | {taps} | {n} | {len(vals)} | {cpu:.1f} | {gpu:.1f} | "
        f"{winner} | {stability:.2f} |"
    )

baseline = {
    (taps, n): (cpu, gpu, winner)
    for (scenario, taps, n), (cpu, gpu, winner, _) in summary.items()
    if scenario == "idle_pre"
}

print()
print("# Fixed idle-baseline policy regret under load")
print("| scenario | mean_regret_pct | max_regret_pct | changed_routes | points |")
print("|---|---:|---:|---:|---:|")
for scenario in sorted({k[0] for k in summary if k[0] != "idle_pre"}):
    regrets = []
    changed = 0
    points = 0
    for (taps, n), (_, _, base_winner) in baseline.items():
        key = (scenario, taps, n)
        if key not in summary:
            continue
        cpu, gpu, winner, _ = summary[key]
        selected = cpu if base_winner == "CPU" else gpu
        oracle = min(cpu, gpu)
        regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
        regrets.append(regret)
        changed += winner != base_winner
        points += 1
    if regrets:
        print(
            f"| {scenario} | {statistics.mean(regrets):.3f} | {max(regrets):.3f} | "
            f"{changed} | {points} |"
        )
