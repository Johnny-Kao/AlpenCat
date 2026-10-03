#!/usr/bin/env python3
import glob
import os
import re
from collections import defaultdict

MARGIN = 0.20
REGRET_TRIGGER = 5.0

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

rows = {}

for path in sorted(glob.glob("campaign-results/quick/*.txt")):
    base = os.path.basename(path)
    stem = base[:-4]
    family = next((f for f in patterns if stem.startswith(f + "_")), None)
    if family is None:
        continue
    rest = stem[len(family)+1:]
    scenario = rest.rsplit("_", 1)[0]

    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = patterns[family].search(line)
            if not m:
                continue
            if family == "fir":
                key = (int(m.group(1)), int(m.group(2)))
                cpu, gpu, winner = float(m.group(4)), float(m.group(5)), m.group(7)
            elif family in ("fft", "reduction"):
                key = (int(m.group(1)),)
                cpu, gpu, winner = float(m.group(3)), float(m.group(4)), m.group(6)
            else:
                key = tuple(map(int, m.group(1, 2, 3, 4)))
                cpu, gpu, winner = float(m.group(6)), float(m.group(7)), m.group(9)
            rows[(family, scenario, key)] = (cpu, gpu, winner)

baseline = {
    (family, key): value
    for (family, scenario, key), value in rows.items()
    if scenario == "idle_pre"
}

families = ["fir", "fft", "reduction", "conv"]
scenarios = ["idle_pre", "mem_bw", "cpu_light", "game_like", "gpu_light", "idle_post"]

plan = []
for scenario in scenarios:
    for family in families:
        reasons = set()
        if scenario in ("idle_pre", "idle_post"):
            reasons.add("anchor")
        points = [
            (key, value)
            for (f, s, key), value in rows.items()
            if f == family and s == scenario
        ]
        for key, (cpu, gpu, winner) in points:
            base = baseline.get((family, key))
            if base is None:
                reasons.add("missing_idle_reference")
                continue
            base_cpu, base_gpu, base_winner = base
            oracle = min(cpu, gpu)
            selected = cpu if base_winner == "CPU" else gpu
            regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
            rel_gap = abs(cpu - gpu) / max(oracle, 1.0)

            if winner != base_winner:
                reasons.add("winner_flip")
            if regret >= REGRET_TRIGGER:
                reasons.add("regret_ge_5pct")
            if rel_gap <= MARGIN:
                reasons.add("near_crossover")

        if reasons:
            plan.append((scenario, family, ",".join(sorted(reasons))))

os.makedirs("campaign-results", exist_ok=True)
with open("campaign-results/deep-plan.tsv", "w", encoding="utf-8") as out:
    for row in plan:
        out.write("\t".join(row) + "\n")

print("# Deep validation plan")
print(f"margin={MARGIN}")
print(f"regret_trigger_pct={REGRET_TRIGGER}")
print("| scenario | family | reasons |")
print("|---|---|---|")
for scenario, family, reasons in plan:
    print(f"| {scenario} | {family} | {reasons} |")
