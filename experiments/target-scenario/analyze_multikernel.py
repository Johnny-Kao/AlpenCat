#!/usr/bin/env python3
import glob
import os
import random
import re
import statistics
from collections import defaultdict

random.seed(20261002)

patterns = {
    "fft": re.compile(
        r"fft_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
    "reduction": re.compile(
        r"reduction_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
}

rows = defaultdict(list)
for path in sorted(glob.glob("multikernel/*.txt")):
    base = os.path.basename(path)
    family = None
    for candidate in patterns:
        if base.startswith(candidate + "_"):
            family = candidate
            break
    if family is None:
        continue
    text_data = open(path, "r", encoding="utf-8").read()
    for line in text_data.splitlines():
        m = patterns[family].search(line)
        if not m:
            continue
        n = int(m.group(1))
        cpu = float(m.group(3))
        gpu = float(m.group(4))
        gpu_dev = float(m.group(5))
        winner = m.group(6)
        rows[(family, n)].append((cpu, gpu, gpu_dev, winner))

def med(xs):
    return statistics.median(xs)

def bootstrap_delta(vals, iters=10000):
    deltas = [c - g for c, g, _, _ in vals]
    n = len(deltas)
    boots = []
    for _ in range(iters):
        sample = [deltas[random.randrange(n)] for _ in range(n)]
        boots.append(statistics.mean(sample))
    boots.sort()
    lo = boots[int(0.025 * iters)]
    hi = boots[int(0.975 * iters)]
    p_gpu = sum(x > 0 for x in boots) / iters
    return statistics.mean(deltas), lo, hi, p_gpu

print("# Multi-kernel crossover confidence")
print()
print("Exploratory paired bootstrap over independent benchmark-process medians.")
print("p_gpu estimates P(CPU_time - GPU_time > 0); this is not a hardware-universal probability.")
print()
print("| family | n | reps | cpu_med_us | gpu_med_us | oracle_us | winner | stability | delta_mean_us | ci95_lo_us | ci95_hi_us | p_gpu |")
print("|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|")

summary = {}
for key in sorted(rows):
    family, n = key
    vals = rows[key]
    cpu = med([x[0] for x in vals])
    gpu = med([x[1] for x in vals])
    winner = "CPU" if cpu <= gpu else "GPU"
    stability = sum(x[3] == winner for x in vals) / len(vals)
    dmean, lo, hi, p_gpu = bootstrap_delta(vals)
    summary[key] = (cpu, gpu, winner, stability, dmean, lo, hi, p_gpu)
    print(
        f"| {family} | {n} | {len(vals)} | {cpu/1000:.3f} | {gpu/1000:.3f} | "
        f"{min(cpu,gpu)/1000:.3f} | {winner} | {stability:.3f} | "
        f"{dmean/1000:.3f} | {lo/1000:.3f} | {hi/1000:.3f} | {p_gpu:.4f} |"
    )

print()
print("# 100 us - 1 ms decision band")
print("| family | points_in_band | high_confidence_points | ambiguous_points |")
print("|---|---:|---:|---:|")
for family in sorted(patterns):
    pts = [v for (f,_),v in summary.items() if f == family and 100_000 <= min(v[0],v[1]) <= 1_000_000]
    high = sum(1 for v in pts if v[7] >= 0.95 or v[7] <= 0.05)
    print(f"| {family} | {len(pts)} | {high} | {len(pts)-high} |")

print()
print("# Monotonic single-threshold check")
print("| family | best_threshold_n | mean_regret_pct | max_regret_pct | wrong_points | points |")
print("|---|---:|---:|---:|---:|---:|")
for family in sorted(patterns):
    pts = [(n, *summary[(family,n)][:2]) for f,n in summary if f == family]
    pts.sort()
    if not pts:
        continue
    ns = [p[0] for p in pts]
    candidates = [0]
    for a,b in zip(ns, ns[1:]):
        candidates.append(a + (b-a)//2 + 1)
    candidates.append(ns[-1] + 1)
    best = None
    for t in candidates:
        regrets=[]
        wrong=0
        for n,cpu,gpu in pts:
            route_gpu = n >= t
            selected = gpu if route_gpu else cpu
            oracle=min(cpu,gpu)
            regrets.append(100*(selected-oracle)/oracle)
            actual_gpu = gpu < cpu
            wrong += route_gpu != actual_gpu
        score=sum(regrets)
        cand=(score,t,statistics.mean(regrets),max(regrets),wrong)
        if best is None or cand < best:
            best=cand
    _,t,mean_r,max_r,wrong=best
    print(f"| {family} | {t} | {mean_r:.3f} | {max_r:.3f} | {wrong} | {len(pts)} |")
