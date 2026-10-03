#!/usr/bin/env python3
import glob
import os
import re
import statistics
from collections import defaultdict

RESULT = re.compile(
    r"psi_result phase=(\S+) sweep=(\d+) start_wall_ns=(\d+) end_wall_ns=(\d+) n=(\d+) "
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
            start_wall_ns = int(m.group(3))
            end_wall_ns = int(m.group(4))
            n = int(m.group(5))
            serial_ns = float(m.group(6))
            parallel_ns = float(m.group(7))
            winner = m.group(8)
            rows[(phase, n)].append(
                (sweep, start_wall_ns, end_wall_ns, serial_ns, parallel_ns, winner)
            )

baseline = {}
for (phase, n), vals in rows.items():
    if phase != "idle_pre":
        continue
    serial = statistics.median(v[3] for v in vals)
    parallel = statistics.median(v[4] for v in vals)
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
        serial = statistics.median(v[3] for v in vals)
        parallel = statistics.median(v[4] for v in vals)
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

print()
print("## Event timing vs first materially harmful stale-route sample")
print()
print("| phase | first_harm_ms_from_phase_start | first_standard_event_ms | standard_lead_ms | first_fast_event_ms | fast_lead_ms |")
print("|---|---:|---:|---:|---:|---:|")

timing_rows = []
for phase, (start, end) in phase_windows.items():
    if phase == "idle_pre":
        continue

    harmful_samples = []
    for (p, n), vals in rows.items():
        if p != phase or n not in baseline:
            continue
        for sweep, sample_start, sample_end, serial, parallel, winner in vals:
            selected = serial if baseline[n] == "SERIAL" else parallel
            oracle = min(serial, parallel)
            regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
            if regret >= 5.0:
                harmful_samples.append((sample_start, sample_end, sweep, n, regret))

    if not harmful_samples:
        continue

    first_harm = min(harmful_samples, key=lambda x: x[0])
    first_harm_ns = first_harm[0]

    standard_events = sorted(
        ts
        for source in ("cpu", "memory", "io")
        for ts in events[source]
        if start <= ts <= end
    )
    fast_events = sorted(
        ts
        for source in ("cpu-fast", "memory-fast", "io-fast")
        for ts in events[source]
        if start <= ts <= end
    )

    standard_event = standard_events[0] if standard_events else None
    fast_event = fast_events[0] if fast_events else None
    standard_lead_ms = (
        (first_harm_ns - standard_event) / 1e6 if standard_event is not None else None
    )
    fast_lead_ms = (
        (first_harm_ns - fast_event) / 1e6 if fast_event is not None else None
    )

    timing_rows.append((phase, standard_lead_ms, fast_lead_ms))
    first_harm_ms = (first_harm_ns - start) / 1e6
    standard_event_ms = (
        f"{(standard_event - start) / 1e6:.3f}" if standard_event is not None else "n/a"
    )
    fast_event_ms = (
        f"{(fast_event - start) / 1e6:.3f}" if fast_event is not None else "n/a"
    )
    standard_lead = f"{standard_lead_ms:.3f}" if standard_lead_ms is not None else "n/a"
    fast_lead = f"{fast_lead_ms:.3f}" if fast_lead_ms is not None else "n/a"
    print(
        f"| {phase} | {first_harm_ms:.3f} | {standard_event_ms} | "
        f"{standard_lead} | {fast_event_ms} | {fast_lead} |"
    )

if timing_rows:
    standard_leads = [r[1] for r in timing_rows if r[1] is not None]
    fast_leads = [r[2] for r in timing_rows if r[2] is not None]
    print()
    print(
        "psi_timing_summary "
        f"harmful_phases={len(timing_rows)} "
        f"standard_event_seen={len(standard_leads)}/{len(timing_rows)} "
        f"standard_event_before_first_harm={sum(x > 0 for x in standard_leads)}/{len(standard_leads)} "
        f"fast_event_seen={len(fast_leads)}/{len(timing_rows)} "
        f"fast_event_before_first_harm={sum(x > 0 for x in fast_leads)}/{len(fast_leads)} "
        f"standard_median_lead_ms={statistics.median(standard_leads):.3f}"
        if standard_leads
        else "psi_timing_summary no_standard_events"
    )
