#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

CPU_PROC_PCT = 80.0
MAX_RSS_MB = 400.0
GPU_DELTA_NS = 50_000_000
HARMFUL_REGRET = 5.0

RESULT_PATTERNS = {
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

SCENARIOS = ["idle_pre", "mem_bw", "cpu_light", "game_like", "gpu_light", "idle_post"]

def parse_snapshot(path):
    text = open(path, encoding="utf-8").read()

    process_rows = []
    in_processes = False
    for line in text.splitlines():
        if line.startswith("--- TOP PROCESSES ---"):
            in_processes = True
            continue
        if line.startswith("--- LOAD ---"):
            in_processes = False
        if in_processes:
            m = re.match(
                r"\s*\d+\s+\d+\s+([0-9.]+)\s+([0-9.]+)\s+(\d+)\s+",
                line,
            )
            if m:
                process_rows.append((float(m.group(1)), int(m.group(3))))

    gpu_total = sum(
        int(x) for x in re.findall(r'accumulatedGPUTime"=(\d+)', text)
    )
    return {
        "max_cpu": max((v[0] for v in process_rows), default=0.0),
        "max_rss_mb": max((v[1] for v in process_rows), default=0) / 1024.0,
        "gpu_total": gpu_total,
    }

def epoch_key(before, active):
    gpu_delta = max(0, active["gpu_total"] - before["gpu_total"])
    return (
        int(active["max_cpu"] >= CPU_PROC_PCT),
        int(active["max_rss_mb"] >= MAX_RSS_MB),
        int(gpu_delta >= GPU_DELTA_NS),
    ), gpu_delta

def parse_results():
    rows = defaultdict(list)
    for pattern in ("campaign-results/quick/*.txt", "campaign-results/deep/*.txt"):
        for path in sorted(glob.glob(pattern)):
            base = os.path.basename(path)
            stem = base[:-4]
            family = next(
                (f for f in RESULT_PATTERNS if stem.startswith(f + "_")), None
            )
            if family is None:
                continue
            rest = stem[len(family) + 1:]
            scenario, rep_text = rest.rsplit("_", 1)
            rep = int(rep_text)

            with open(path, encoding="utf-8") as fh:
                for line in fh:
                    m = RESULT_PATTERNS[family].search(line)
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
    return rows

snapshots = {}
for scenario in SCENARIOS:
    before_path = f"campaign-results/snapshots/quick_{scenario}_before.txt"
    active_path = f"campaign-results/snapshots/quick_{scenario}_active.txt"
    if not os.path.exists(before_path) or not os.path.exists(active_path):
        continue
    before = parse_snapshot(before_path)
    active = parse_snapshot(active_path)
    key, gpu_delta = epoch_key(before, active)
    snapshots[scenario] = {
        "key": key,
        "max_cpu": active["max_cpu"],
        "max_rss_mb": active["max_rss_mb"],
        "gpu_delta_ns": gpu_delta,
    }

rows = parse_results()
baseline = {}
for (family, scenario, key), vals in rows.items():
    if scenario != "idle_pre":
        continue
    cpu = statistics.median(v[1] for v in vals)
    gpu = statistics.median(v[2] for v in vals)
    route = "CPU" if cpu <= gpu else "GPU"
    baseline[(family, key)] = (route, cpu if route == "CPU" else gpu)

scenario_stats = defaultdict(lambda: {"samples": 0, "harmful": 0, "regrets": []})
for (family, scenario, key), vals in rows.items():
    if scenario == "idle_pre" or (family, key) not in baseline:
        continue
    route, _ = baseline[(family, key)]
    for rep, cpu, gpu in vals:
        selected = cpu if route == "CPU" else gpu
        oracle = min(cpu, gpu)
        regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
        st = scenario_stats[scenario]
        st["samples"] += 1
        st["harmful"] += int(regret >= HARMFUL_REGRET)
        st["regrets"].append(regret)

idle_key = snapshots.get("idle_pre", {}).get("key")

print("# Coarse resource epoch screen")
print()
print(
    "Observable epoch bits are: max process CPU >=80%, max process RSS >=400 MiB, "
    "and accelerator accumulated-GPU-time delta >=50 ms between pre-load and active snapshots."
)
print("These bits only invalidate a boundary; they do not choose CPU or GPU.")
print()
print("| scenario | epoch(cpu,rss,gpu) | changed_vs_idle | max_cpu_pct | max_rss_mib | gpu_delta_ms | harmful/total | mean_static_regret_pct |")
print("|---|---|---:|---:|---:|---:|---:|---:|")
for scenario in SCENARIOS:
    snap = snapshots.get(scenario)
    st = scenario_stats.get(scenario, {"samples": 0, "harmful": 0, "regrets": []})
    if snap is None:
        continue
    mean_regret = statistics.mean(st["regrets"]) if st["regrets"] else 0.0
    changed = snap["key"] != idle_key
    print(
        f"| {scenario} | {snap['key']} | {int(changed)} | "
        f"{snap['max_cpu']:.1f} | {snap['max_rss_mb']:.1f} | "
        f"{snap['gpu_delta_ns']/1e6:.1f} | {st['harmful']}/{st['samples']} | "
        f"{mean_regret:.3f} |"
    )

harmful_in_changed = 0
harmful_total = 0
all_in_changed = 0
all_total = 0
for scenario, st in scenario_stats.items():
    if scenario not in snapshots:
        continue
    changed = snapshots[scenario]["key"] != idle_key
    harmful_total += st["harmful"]
    all_total += st["samples"]
    if changed:
        harmful_in_changed += st["harmful"]
        all_in_changed += st["samples"]

print()
print(
    "epoch_screen_total "
    f"harmful_caught={harmful_in_changed}/{harmful_total} "
    f"harmful_recall={(harmful_in_changed/harmful_total if harmful_total else 0):.3f} "
    f"calls_in_changed_epochs={all_in_changed}/{all_total} "
    f"changed_epoch_share={(all_in_changed/all_total if all_total else 0):.3f}"
)
