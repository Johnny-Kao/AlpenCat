#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

PATTERNS = {
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

SCENARIOS = [
    "cpu_light", "cpu_heavy", "mem_resident", "mem_bw",
    "gpu_light", "gpu_heavy", "app_like", "video_like",
    "game_like", "idle_post",
]

def line_id(family, key):
    return key[0] if family in ("fir", "conv") else 0

def axis(family, key):
    if family == "fir":
        return key[1]
    if family == "conv":
        return key[3]
    return key[0]

rows = defaultdict(list)
for pattern in ("campaign-results/quick/*.txt", "campaign-results/deep/*.txt"):
    for path in sorted(glob.glob(pattern)):
        stem = os.path.basename(path)[:-4]
        family = next((f for f in PATTERNS if stem.startswith(f + "_")), None)
        if family is None:
            continue
        rest = stem[len(family) + 1:]
        scenario, rep_text = rest.rsplit("_", 1)
        rep = int(rep_text)
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                m = PATTERNS[family].search(line)
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

# Median process-level evidence avoids treating one noisy repetition as a
# recalibration target.
med = {}
for k, vals in rows.items():
    med[k] = (
        statistics.median(v[1] for v in vals),
        statistics.median(v[2] for v in vals),
    )

lines = defaultdict(list)
for family, scenario, key in med:
    if scenario == "idle_pre":
        lines[(family, line_id(family, key))].append(key)
for line in lines:
    family, _ = line
    lines[line] = sorted(set(lines[line]), key=lambda key: axis(family, key))

baseline = {}
for line, keys in lines.items():
    family, _ = line
    winners = []
    for key in keys:
        cpu, gpu = med[(family, "idle_pre", key)]
        winners.append("CPU" if cpu <= gpu else "GPU")
    transition = next(
        ((i, i + 1) for i in range(len(keys) - 1) if winners[i] != winners[i + 1]),
        None,
    )
    baseline[line] = (keys, winners, transition)

print("# Localized recalibration cost screen")
print()
print(
    "Conservative replay: start from the idle crossover bracket, execute both "
    "backends once at the two bracket points, and expand outward one measured "
    "bucket at a time only if the bracket no longer contains both winners."
)
print(
    "Cost is CPU+GPU host time at each checked point. This is deliberately more "
    "expensive than a selected-path implementation and excludes hot FastRoute cost."
)
print()
print("| scenario | family | checked_points | calibration_cost_us | stale_loss_per_sweep_us | break_even_sweeps |")
print("|---|---|---:|---:|---:|---:|")

totals = defaultdict(lambda: [0, 0.0, 0.0])
meaningful = []

for scenario in SCENARIOS:
    by_family = defaultdict(lambda: [0, 0.0, 0.0])

    for line, (keys, idle_winners, transition) in baseline.items():
        if transition is None:
            continue
        family, _ = line

        scenario_winners = []
        for key in keys:
            pair = med.get((family, scenario, key))
            if pair is None:
                scenario_winners.append(None)
            else:
                scenario_winners.append("CPU" if pair[0] <= pair[1] else "GPU")

        lo, hi = transition
        checked = {i for i in (lo, hi) if scenario_winners[i] is not None}
        if not checked:
            continue

        while True:
            observed = {scenario_winners[i] for i in checked}
            if "CPU" in observed and "GPU" in observed:
                break
            if len(observed) != 1:
                break

            winner = next(iter(observed))
            if winner == "CPU":
                nxt = max(checked) + 1
                if nxt >= len(keys) or scenario_winners[nxt] is None:
                    break
            else:
                nxt = min(checked) - 1
                if nxt < 0 or scenario_winners[nxt] is None:
                    break
            checked.add(nxt)

        cost = 0.0
        for i in checked:
            cpu, gpu = med[(family, scenario, keys[i])]
            cost += cpu + gpu

        stale_loss = 0.0
        for i, key in enumerate(keys):
            pair = med.get((family, scenario, key))
            if pair is None:
                continue
            cpu, gpu = pair
            selected = cpu if idle_winners[i] == "CPU" else gpu
            stale_loss += selected - min(cpu, gpu)

        row = by_family[family]
        row[0] += len(checked)
        row[1] += cost
        row[2] += stale_loss

    for family in sorted(by_family):
        points, cost, loss = by_family[family]
        if points == 0:
            continue
        break_even = cost / loss if loss > 0 else None
        print(
            f"| {scenario} | {family} | {points} | {cost/1000:.1f} | "
            f"{loss/1000:.1f} | "
            f"{break_even:.1f} |" if break_even is not None else
            f"| {scenario} | {family} | {points} | {cost/1000:.1f} | "
            f"{loss/1000:.1f} | n/a |"
        )
        totals[scenario][0] += points
        totals[scenario][1] += cost
        totals[scenario][2] += loss
        if loss > 0:
            meaningful.append((scenario, family, break_even, cost, loss))

print()
print("## Scenario totals")
print("| scenario | checked_points | calibration_cost_us | stale_loss_per_sweep_us | break_even_sweeps |")
print("|---|---:|---:|---:|---:|")
for scenario in SCENARIOS:
    points, cost, loss = totals[scenario]
    if points == 0:
        continue
    be = cost / loss if loss > 0 else None
    if be is None:
        print(f"| {scenario} | {points} | {cost/1000:.1f} | {loss/1000:.1f} | n/a |")
    else:
        print(f"| {scenario} | {points} | {cost/1000:.1f} | {loss/1000:.1f} | {be:.1f} |")

if meaningful:
    bes = [x[2] for x in meaningful]
    print()
    print(
        "recalibration_total "
        f"meaningful_family_scenarios={len(meaningful)} "
        f"median_break_even_sweeps={statistics.median(bes):.2f} "
        f"p75_break_even_sweeps={statistics.quantiles(bes, n=4, method='inclusive')[2]:.2f} "
        f"max_break_even_sweeps={max(bes):.2f}"
    )
