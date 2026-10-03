#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

SCENARIO_ORDER = ["idle_pre", "mem_bw", "cpu_light", "game_like", "gpu_light", "idle_post"]
THRESHOLDS = [0.05, 0.10, 0.20, 0.30, 0.50]
HARMFUL_REGRET = 5.0

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

def parse_path(path):
    base = os.path.basename(path)
    stem = base[:-4] if base.endswith(".txt") else base
    family = next((f for f in patterns if stem.startswith(f + "_")), None)
    if family is None:
        return
    rest = stem[len(family) + 1:]
    if "_" not in rest:
        return
    scenario, rep_text = rest.rsplit("_", 1)
    try:
        rep = int(rep_text)
    except ValueError:
        return

    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = patterns[family].search(line)
            if not m:
                continue
            if family == "fir":
                key = (int(m.group(1)), int(m.group(2)))
                cpu, gpu = float(m.group(4)), float(m.group(5))
            elif family in ("fft", "reduction"):
                key = (int(m.group(1)),)
                cpu, gpu = float(m.group(3)), float(m.group(4))
            else:
                key = tuple(map(int, m.group(1, 2, 3, 4)))
                cpu, gpu = float(m.group(6)), float(m.group(7))
            rows[(family, scenario, key)].append((rep, cpu, gpu))

for pat in ("campaign-results/quick/*.txt", "campaign-results/deep/*.txt", "campaign-results/*.txt"):
    for path in sorted(glob.glob(pat)):
        parse_path(path)

# De-duplicate same rep from flattened / raw layouts.
for k, vals in list(rows.items()):
    unique = {}
    for rep, cpu, gpu in vals:
        unique[rep] = (rep, cpu, gpu)
    rows[k] = [unique[r] for r in sorted(unique)]

baseline = {}
for (family, scenario, key), vals in rows.items():
    if scenario != "idle_pre":
        continue
    cpu = statistics.median(v[1] for v in vals)
    gpu = statistics.median(v[2] for v in vals)
    route = "CPU" if cpu <= gpu else "GPU"
    baseline[(family, key)] = {
        "cpu": cpu,
        "gpu": gpu,
        "route": route,
        "selected": cpu if route == "CPU" else gpu,
    }

samples = []
for (family, scenario, key), vals in rows.items():
    if scenario == "idle_pre" or (family, key) not in baseline:
        continue
    b = baseline[(family, key)]
    for rep, cpu, gpu in vals:
        selected = cpu if b["route"] == "CPU" else gpu
        oracle = min(cpu, gpu)
        regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
        slowdown = (selected - b["selected"]) / max(b["selected"], 1.0)
        harmful = regret >= HARMFUL_REGRET
        winner_flip = ("CPU" if cpu <= gpu else "GPU") != b["route"]
        samples.append({
            "family": family,
            "scenario": scenario,
            "key": key,
            "rep": rep,
            "slowdown": slowdown,
            "regret": regret,
            "harmful": harmful,
            "winner_flip": winner_flip,
        })

def metrics(threshold, subset):
    tp = fp = tn = fn = 0
    for s in subset:
        signal = s["slowdown"] >= threshold
        if signal and s["harmful"]:
            tp += 1
        elif signal and not s["harmful"]:
            fp += 1
        elif not signal and s["harmful"]:
            fn += 1
        else:
            tn += 1
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    specificity = tn / (tn + fp) if tn + fp else 0.0
    return tp, fp, tn, fn, precision, recall, specificity

print("# Passive selected-route drift screen")
print()
print(
    "Signal uses only timing from the backend the frozen idle policy already selected. "
    "No alternate-backend execution is used to trigger the signal."
)
print(f"Harmful stale route label: regret >= {HARMFUL_REGRET:.1f}%.")
print()
print("| slowdown trigger | TP | FP | TN | FN | precision | recall | specificity |")
print("|---:|---:|---:|---:|---:|---:|---:|---:|")
for t in THRESHOLDS:
    tp, fp, tn, fn, p, r, sp = metrics(t, samples)
    print(f"| {100*t:.0f}% | {tp} | {fp} | {tn} | {fn} | {p:.3f} | {r:.3f} | {sp:.3f} |")

print()
print("## By family at the existing 10% uncertainty margin")
print("| family | samples | harmful | precision | recall | specificity | median harmful slowdown |")
print("|---|---:|---:|---:|---:|---:|---:|")
for family in sorted({s["family"] for s in samples}):
    subset = [s for s in samples if s["family"] == family]
    tp, fp, tn, fn, p, r, sp = metrics(0.10, subset)
    hs = [s["slowdown"] for s in subset if s["harmful"]]
    med = statistics.median(hs) if hs else 0.0
    print(
        f"| {family} | {len(subset)} | {len(hs)} | {p:.3f} | {r:.3f} | "
        f"{sp:.3f} | {100*med:.1f}% |"
    )

print()
print("## High-regret misses at 10%")
misses = [s for s in samples if s["harmful"] and s["slowdown"] < 0.10]
misses.sort(key=lambda s: s["regret"], reverse=True)
print("| family | scenario | key | rep | regret_pct | selected_route_slowdown_pct |")
print("|---|---|---|---:|---:|---:|")
for s in misses[:20]:
    print(
        f"| {s['family']} | {s['scenario']} | {s['key']} | {s['rep']} | "
        f"{s['regret']:.3f} | {100*s['slowdown']:.3f} |"
    )

print()
print("passive_screen_total", f"samples={len(samples)}", f"harmful={sum(s['harmful'] for s in samples)}")
