#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

patterns = {
    "fir": re.compile(
        r"fir_result taps=(\d+) n=(\d+) iters=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
    "fft": re.compile(
        r"fft_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
    "reduction": re.compile(
        r"reduction_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
    "conv": re.compile(
        r"conv_result w=(\d+) h=(\d+) ks=(\d+) work=(\d+) reps=(\d+) "
        r"cpu_ns=([0-9.]+) gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
}

rows = defaultdict(list)

def parse_file(path):
    base = os.path.basename(path)
    stem = base[:-4] if base.endswith(".txt") else base
    family = next((f for f in patterns if stem.startswith(f + "_")), None)
    if family is None:
        return
    rest = stem[len(family)+1:]
    pos = rest.rfind("_")
    if pos < 0:
        return
    scenario = rest[:pos]
    try:
        rep = int(rest[pos+1:])
    except ValueError:
        return

    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = patterns[family].search(line)
            if not m:
                continue
            if family == "fir":
                taps, n = int(m.group(1)), int(m.group(2))
                cpu, gpu = float(m.group(4)), float(m.group(5))
                winner = m.group(7)
                key = (taps, n)
            elif family in ("fft", "reduction"):
                n = int(m.group(1))
                cpu, gpu = float(m.group(3)), float(m.group(4))
                winner = m.group(6)
                key = (n,)
            else:
                w, h, ks, work = map(int, m.group(1,2,3,4))
                cpu, gpu = float(m.group(6)), float(m.group(7))
                winner = m.group(9)
                key = (ks, w, h, work)
            rows[(family, scenario, key)].append((cpu, gpu, winner, rep))

for path in sorted(glob.glob("campaign-results/*.txt")):
    parse_file(path)

def med(xs):
    return statistics.median(xs)

summary = {}
for k, vals in rows.items():
    cpu = med([v[0] for v in vals])
    gpu = med([v[1] for v in vals])
    winner = "CPU" if cpu <= gpu else "GPU"
    stability = sum(v[2] == winner for v in vals) / len(vals)
    summary[k] = (cpu, gpu, winner, stability, len(vals))

print("# AlpenCat Validation Campaign v2")
print()
print("## Aggregated heterogeneous kernel results")
print("| family | scenario | key | reps | cpu_us | gpu_us | winner | winner_stability |")
print("|---|---|---|---:|---:|---:|---|---:|")
for (family, scenario, key), (cpu, gpu, winner, stability, reps) in sorted(summary.items()):
    print(f"| {family} | {scenario} | {key} | {reps} | {cpu/1000:.3f} | {gpu/1000:.3f} | {winner} | {stability:.2f} |")

# Build idle baseline.
baseline = {
    (family, key): value
    for (family, scenario, key), value in summary.items()
    if scenario == "idle_pre"
}

# Structural boundary band: for each 1-D family line, take the two adjacent
# measured points bracketing the first CPU->GPU transition in idle_pre.
boundary = set()

def add_boundary_for_group(family, group_items, x_index):
    pts = sorted(group_items, key=lambda item: item[0][x_index])
    for left, right in zip(pts, pts[1:]):
        lk, lv = left
        rk, rv = right
        if lv[2] != rv[2]:
            boundary.add((family, lk))
            boundary.add((family, rk))
            break

# FFT/reduction are one-dimensional in n.
for family in ("fft", "reduction"):
    items = [(key, val) for (f,key), val in baseline.items() if f == family]
    add_boundary_for_group(family, items, 0)

# FIR grouped by taps, sorted by n.
fir_taps = sorted({key[0] for (f,key) in baseline if f == "fir"})
for taps in fir_taps:
    items = [(key, val) for (f,key), val in baseline.items() if f == "fir" and key[0] == taps]
    add_boundary_for_group("fir", items, 1)

# Conv2D grouped by kernel size, sorted by work.
conv_ks = sorted({key[0] for (f,key) in baseline if f == "conv"})
for ks in conv_ks:
    items = [(key, val) for (f,key), val in baseline.items() if f == "conv" and key[0] == ks]
    add_boundary_for_group("conv", items, 3)

print()
print("## Frozen idle policy vs localized-boundary upper bound")
print("Localized-boundary upper bound recalibrates only the two measured points around each idle crossover.")
print("It is an offline upper bound, not an implementation claim.")
print("| family | scenario | static_mean_regret_pct | static_p95_regret_pct | static_max_regret_pct | localized_mean_regret_pct | localized_p95_regret_pct | localized_max_regret_pct | regret_recovered_pct | boundary_points | points |")
print("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")

def p95(xs):
    if not xs:
        return 0.0
    ys = sorted(xs)
    idx = max(0, min(len(ys)-1, int(round(0.95*(len(ys)-1)))))
    return ys[idx]

families = sorted({f for (f,_,_) in summary})
scenarios = sorted({s for (_,s,_) in summary if s != "idle_pre"})
for family in families:
    family_base = {key: val for (f,key),val in baseline.items() if f == family}
    for scenario in scenarios:
        static_regrets = []
        localized_regrets = []
        bcount = 0
        for key, base in family_base.items():
            current = summary.get((family, scenario, key))
            if current is None:
                continue
            cpu, gpu, winner, _, _ = current
            base_route = base[2]
            oracle = min(cpu, gpu)
            selected = cpu if base_route == "CPU" else gpu
            sr = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
            static_regrets.append(sr)

            if (family, key) in boundary:
                lr = 0.0
                bcount += 1
            else:
                lr = sr
            localized_regrets.append(lr)

        if not static_regrets:
            continue
        smean = statistics.mean(static_regrets)
        lmean = statistics.mean(localized_regrets)
        recovered = 0.0 if smean == 0 else 100.0 * (smean - lmean) / smean
        print(
            f"| {family} | {scenario} | {smean:.3f} | {p95(static_regrets):.3f} | {max(static_regrets):.3f} | "
            f"{lmean:.3f} | {p95(localized_regrets):.3f} | {max(localized_regrets):.3f} | "
            f"{recovered:.1f} | {bcount} | {len(static_regrets)} |"
        )

print()
print("## 100 us - 1 ms oracle-duration band")
print("| family | scenario | points | changed_vs_idle |")
print("|---|---|---:|---:|")
for family in families:
    family_base = {key: val for (f,key),val in baseline.items() if f == family}
    for scenario in sorted({s for (_,s,_) in summary}):
        count = changed = 0
        for key, base in family_base.items():
            current = summary.get((family, scenario, key))
            if current is None:
                continue
            cpu, gpu, winner, _, _ = current
            oracle = min(cpu, gpu)
            if 100_000 <= oracle <= 1_000_000:
                count += 1
                changed += int(winner != base[2])
        if count:
            print(f"| {family} | {scenario} | {count} | {changed} |")
