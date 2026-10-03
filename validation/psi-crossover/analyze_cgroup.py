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
for path in sorted(glob.glob("psi-cgroup-results/sweeps/*.txt")):
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = RESULT.search(line)
            if not m:
                continue
            phase = m.group(1)
            sweep = int(m.group(2))
            start_ns = int(m.group(3))
            end_ns = int(m.group(4))
            n = int(m.group(5))
            serial = float(m.group(6))
            parallel = float(m.group(7))
            winner = m.group(8)
            rows[(phase, n)].append((sweep, start_ns, end_ns, serial, parallel, winner))

baseline = {}
for (phase, n), vals in rows.items():
    if phase != "idle_pre":
        continue
    serial = statistics.median(v[3] for v in vals)
    parallel = statistics.median(v[4] for v in vals)
    baseline[n] = "SERIAL" if serial <= parallel else "PARALLEL"

events = defaultdict(list)
for path in sorted(glob.glob("psi-cgroup-results/*.log")):
    name = os.path.basename(path)[:-4]
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = re.search(r"psi_event resource=(\w+) wall_ns=(\d+)", line)
            if m:
                events[name].append(int(m.group(2)))

windows = {}
for path in sorted(glob.glob("psi-cgroup-results/phases/*.window")):
    phase = os.path.basename(path)[:-7]
    start, end = map(int, open(path, encoding="utf-8").read().split())
    windows[phase] = (start, end)

sources = {
    "system": ("system-cpu", "system-memory", "system-io"),
    "cgroup": ("cgroup-cpu", "cgroup-memory", "cgroup-io"),
}

print("# System-wide PSI vs per-cgroup PSI")
print()
print("| phase | harmful_points | max_regret_pct | system_events | cgroup_events | system_first_lead_ms | cgroup_first_lead_ms |")
print("|---|---:|---:|---:|---:|---:|---:|")

summary = {
    "system": {"event_phases": 0, "harm_caught": 0, "false": 0, "lead": []},
    "cgroup": {"event_phases": 0, "harm_caught": 0, "false": 0, "lead": []},
}
harmful_phase_count = 0

for phase, (phase_start, phase_end) in windows.items():
    if phase == "idle_pre":
        continue

    harmful_samples = []
    max_regret = 0.0
    harmful_points = 0

    for (p, n), vals in rows.items():
        if p != phase or n not in baseline:
            continue

        median_serial = statistics.median(v[3] for v in vals)
        median_parallel = statistics.median(v[4] for v in vals)
        median_selected = median_serial if baseline[n] == "SERIAL" else median_parallel
        median_oracle = min(median_serial, median_parallel)
        median_regret = (
            100.0 * (median_selected - median_oracle) / median_oracle
            if median_oracle > 0 else 0.0
        )
        if median_regret >= 5.0:
            harmful_points += 1
        max_regret = max(max_regret, median_regret)

        for sweep, start_ns, end_ns, serial, parallel, winner in vals:
            selected = serial if baseline[n] == "SERIAL" else parallel
            oracle = min(serial, parallel)
            regret = 100.0 * (selected - oracle) / oracle if oracle > 0 else 0.0
            if regret >= 5.0:
                harmful_samples.append((start_ns, regret, n, sweep))

    phase_harmful = harmful_points > 0
    if phase_harmful:
        harmful_phase_count += 1

    first_harm_ns = min((x[0] for x in harmful_samples), default=None)
    counts = {}
    leads = {}
    for kind, source_names in sources.items():
        ev = sorted(
            ts
            for source in source_names
            for ts in events[source]
            if phase_start <= ts <= phase_end
        )
        counts[kind] = len(ev)
        if ev:
            summary[kind]["event_phases"] += 1
            if phase_harmful:
                summary[kind]["harm_caught"] += 1
            else:
                summary[kind]["false"] += 1
        lead = None
        if first_harm_ns is not None and ev:
            lead = (first_harm_ns - ev[0]) / 1e6
            summary[kind]["lead"].append(lead)
        leads[kind] = lead

    def fmt(v):
        return f"{v:.3f}" if v is not None else "n/a"

    print(
        f"| {phase} | {harmful_points} | {max_regret:.3f} | "
        f"{counts['system']} | {counts['cgroup']} | "
        f"{fmt(leads['system'])} | {fmt(leads['cgroup'])} |"
    )

print()
for kind in ("system", "cgroup"):
    s = summary[kind]
    lead = s["lead"]
    print(
        f"{kind}_psi_summary "
        f"harmful_phases={harmful_phase_count} "
        f"event_phases={s['event_phases']} "
        f"harmful_caught={s['harm_caught']} "
        f"event_without_material_harm={s['false']} "
        f"event_before_first_harm={sum(x > 0 for x in lead)}/{len(lead)} "
        f"median_lead_ms={statistics.median(lead):.3f}" if lead else
        f"{kind}_psi_summary harmful_phases={harmful_phase_count} no_harm_timing"
    )
