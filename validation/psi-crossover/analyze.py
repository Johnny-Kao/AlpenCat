#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

RESULT = re.compile(
    r"psi_result phase=(\S+) sweep=(\d+) wall_ns=(\d+) n=(\d+) "
    r"serial_ns=([0-9.]+) parallel_ns=([0-9.]+) winner=(SERIAL|PARALLEL)"
)

rows = defaultdict(list)
for path in sorted(glob.glob("psi-results/sweeps/*.txt")):
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = RESULT.search(line)
            if not m:
                continue
            phase = m.group(1)
            sweep = int(m.group(2))
            wall_ns = int(m.group(3))
            n = int(m.group(4))
            serial_ns = float(m.group(5))
            parallel_ns = float(m.group(6))
            winner = m.group(7)
            rows[(phase, n)].append((sweep, wall_ns, serial_ns, parallel_ns, winner))

baseline = {}
for (phase, n), vals in rows.items():
    if phase != "idle_pre":
        continue
    serial = statistics.median(v[2] for v in vals)
    parallel = statistics.median(v[3] for v in vals)
    baseline[n] = "SERIAL" if serial <= parallel else "PARALLEL"

events = defaultdict(list)
for path in sorted(glob.glob("psi-results/psi-*.log")):
    source = os.path.basename(path)[4:-4]
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = re.search(r"psi_event resource=(\w+) wall_ns=(\d+)", line)
            if m:
                events[source].append(int(m.group(2)))

phase_windows = {}
for path in sorted(glob.glob("psi-results/phases/*.window")):
    phase = os.path.basename(path)[:-7]
    start, end = map(int, open(path, encoding="utf-8").read().strip().split())
    phase_windows[phase] = (start, end)

print("# PSI vs serial/parallel crossover correlation")
print()
print("| phase | cpu_events | cpu_fast_events | memory_events | memory_fast_events | io_events | io_fast_events | winner_flips | harmful_points | tested_points | mean_static_regret_pct | max_static_regret_pct |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")

phase_summary = {}
for phase in phase_windows:
    start, end = phase_windows[phase]
    counts = {
        r: sum(start <= ts <= end for ts in events[r])
        for r in ("cpu", "cpu-fast", "memory", "memory-fast", "io", "io-fast")
    }
    flips = 0
    harmful = 0
    tested = 0
    regrets = []
    for (p, n), vals in rows.items():
        if p != phase or n not in baseline:
            continue
        serial = statistics.median(v[2] for v in vals)
        parallel = statistics.median(v[3] for v in vals)
        winner = "SERIAL" if serial <= parallel else "PARALLEL"
        flips += int(winner != baseline[n])
        tested += 1
        selected = serial if baseline[n] == "SERIAL" else parallel
        oracle = min(serial, parallel)
        regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
        regrets.append(regret)
        harmful += int(regret >= 5.0)

    mean_regret = statistics.mean(regrets) if regrets else 0.0
    max_regret = max(regrets) if regrets else 0.0
    phase_summary[phase] = (counts, flips, harmful, tested, mean_regret, max_regret)
    print(
        f"| {phase} | {counts['cpu']} | {counts['cpu-fast']} | "
        f"{counts['memory']} | {counts['memory-fast']} | "
        f"{counts['io']} | {counts['io-fast']} | "
        f"{flips} | {harmful} | {tested} | {mean_regret:.3f} | {max_regret:.3f} |"
    )

print()
movement_phases = [
    p for p, (_, flips, _, _, _, _) in phase_summary.items()
    if p != "idle_pre" and flips > 0
]
harmful_phases = [
    p for p, (_, _, harmful, _, _, _) in phase_summary.items()
    if p != "idle_pre" and harmful > 0
]
normal_event_phases = [
    p for p, (counts, _, _, _, _, _) in phase_summary.items()
    if p != "idle_pre"
    and (counts["cpu"] + counts["memory"] + counts["io"]) > 0
]
fast_event_phases = [
    p for p, (counts, _, _, _, _, _) in phase_summary.items()
    if p != "idle_pre"
    and (counts["cpu-fast"] + counts["memory-fast"] + counts["io-fast"]) > 0
]
normal_caught = [p for p in harmful_phases if p in normal_event_phases]
fast_caught = [p for p in harmful_phases if p in fast_event_phases]
normal_false = [p for p in normal_event_phases if p not in harmful_phases]
fast_false = [p for p in fast_event_phases if p not in harmful_phases]
print(
    "psi_phase_summary "
    f"movement_phases={len(movement_phases)} "
    f"harmful_phases={len(harmful_phases)} "
    f"normal_event_phases={len(normal_event_phases)} "
    f"normal_harmful_caught={len(normal_caught)} "
    f"normal_event_without_harm={len(normal_false)} "
    f"fast_event_phases={len(fast_event_phases)} "
    f"fast_harmful_caught={len(fast_caught)} "
    f"fast_event_without_harm={len(fast_false)}"
)
print("movement_phase_names=" + ",".join(movement_phases))
print("harmful_phase_names=" + ",".join(harmful_phases))
print("normal_event_phase_names=" + ",".join(normal_event_phases))
print("fast_event_phase_names=" + ",".join(fast_event_phases))
