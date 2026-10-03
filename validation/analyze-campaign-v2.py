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
events = []

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

    seq = 0
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
            events.append((family, scenario, rep, seq, key, cpu, gpu, winner))
            seq += 1

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

print()
print("## Actual localized event-triggered probe replay")
print(
    "Prototype policy: normal calls use the published boundary. A probe is "
    "scheduled only when the alternative is already plausible from existing "
    "measurements: either the current backend has become >10% slower than the "
    "latest alternative observation, or a previously learned non-baseline route "
    "has changed enough that the original idle winner is again within 10%."
)
print(
    "This is a value-of-probe gate: do not explore a route that is still "
    "obviously expensive. The 10% margin and 0.2 steady alpha reuse existing "
    "M12 constants. A probe executes one backend only; no duplicate CPU+GPU "
    "execution."
)
print(
    "| family | scenario | calls | probes | probe_rate_pct | "
    "mean_regret_pct | p95_regret_pct | max_regret_pct | band_expansions |"
)
print("|---|---|---:|---:|---:|---:|---:|---:|---:|")

UNCERTAINTY_MARGIN = 0.10
STEADY_ALPHA = 0.20
scenario_order = [
    "cpu_light",
    "cpu_heavy",
    "mem_resident",
    "mem_bw",
    "gpu_light",
    "gpu_heavy",
    "app_like",
    "video_like",
    "game_like",
    "idle_post",
]
family_order = {"fir": 0, "fft": 1, "reduction": 2, "conv": 3}

def line_id(family, key):
    if family == "fir":
        return key[0]
    if family == "conv":
        return key[0]
    return 0

def axis_value(family, key):
    if family == "fir":
        return key[1]
    if family == "conv":
        return key[3]
    return key[0]

line_keys = {}
for family in families:
    ids = sorted({line_id(family, key) for (f, key) in baseline if f == family})
    for lid in ids:
        keys = [
            key
            for (f, key) in baseline
            if f == family and line_id(family, key) == lid
        ]
        line_keys[(family, lid)] = sorted(keys, key=lambda key: axis_value(family, key))

state = {}
for line, keys in line_keys.items():
    family, _ = line
    transition = None
    for i in range(len(keys) - 1):
        left = baseline[(family, keys[i])][2]
        right = baseline[(family, keys[i + 1])][2]
        if left != right:
            transition = (i, i + 1)
            break

    idle_reference = {
        key: {
            "CPU": baseline[(family, key)][0],
            "GPU": baseline[(family, key)][1],
        }
        for key in keys
    }
    state[line] = {
        "keys": keys,
        "index": {key: i for i, key in enumerate(keys)},
        "lo": None if transition is None else transition[0],
        "hi": None if transition is None else transition[1],
        "base_lo": None if transition is None else transition[0],
        "base_hi": None if transition is None else transition[1],
        "idle_reference": idle_reference,
        "reference": {
            key: dict(costs) for key, costs in idle_reference.items()
        },
        "learned_route": {
            key: baseline[(family, key)][2] for key in keys
        },
        "last_selected_cost": {
            key: baseline[(family, key)][0]
            if baseline[(family, key)][2] == "CPU"
            else baseline[(family, key)][1]
            for key in keys
        },
        "pending_probe": {key: None for key in keys},
    }

scenario_stats = defaultdict(lambda: {
    "regrets": [],
    "calls": 0,
    "probes": 0,
    "expansions": 0,
})

ordered_events = sorted(
    (event for event in events if event[1] in scenario_order),
    key=lambda event: (
        scenario_order.index(event[1]),
        event[2],
        family_order[event[0]],
        event[3],
    ),
)

for family, scenario, rep, seq, key, cpu, gpu, observed_winner in ordered_events:
    line = (family, line_id(family, key))
    st = state[line]
    idx = st["index"][key]
    lo = st["lo"]
    hi = st["hi"]
    inside = lo is not None and lo <= idx <= hi
    base_route = baseline[(family, key)][2]
    preferred_route = st["learned_route"][key] if inside else base_route

    probe_target = st["pending_probe"][key]
    should_probe = probe_target is not None
    route = probe_target if should_probe else preferred_route

    actual_cost = cpu if route == "CPU" else gpu
    oracle = min(cpu, gpu)
    regret = 100.0 * (actual_cost - oracle) / oracle if oracle > 0 else 0.0

    if should_probe:
        st["pending_probe"][key] = None
        previous_route = preferred_route
        previous_cost = st["last_selected_cost"][key]
        probe_wins = actual_cost < previous_cost * (1.0 - UNCERTAINTY_MARGIN)

        st["reference"][key][route] = actual_cost
        if probe_wins:
            st["learned_route"][key] = route
            st["last_selected_cost"][key] = actual_cost

            if lo is not None and idx < lo:
                st["lo"] = idx
                scenario_stats[(family, scenario)]["expansions"] += 1
            elif hi is not None and idx > hi:
                st["hi"] = idx
                scenario_stats[(family, scenario)]["expansions"] += 1
        else:
            # The drift was real but did not change the winner. Rebase both
            # known costs so the same stable environment does not retrigger.
            st["reference"][key][previous_route] = previous_cost

        # Hysteresis: once the original idle winner is re-confirmed at an
        # expanded outer edge, release that point from the uncertainty band.
        lo = st["lo"]
        hi = st["hi"]
        if (
            probe_wins
            and route == base_route
            and lo is not None
            and idx == lo
            and st["base_lo"] is not None
            and lo < st["base_lo"]
        ):
            st["lo"] = lo + 1
        elif (
            probe_wins
            and route == base_route
            and hi is not None
            and idx == hi
            and st["base_hi"] is not None
            and hi > st["base_hi"]
        ):
            st["hi"] = hi - 1
    else:
        reference = st["reference"][key][route]
        deviation = abs(actual_cost - reference) / max(reference, 1.0)
        st["last_selected_cost"][key] = actual_cost

        other = "GPU" if route == "CPU" else "CPU"
        other_latest = st["reference"][key][other]
        base_route = baseline[(family, key)][2]
        base_idle = st["idle_reference"][key][base_route]

        probe_target = None
        if actual_cost > other_latest * (1.0 + UNCERTAINTY_MARGIN):
            probe_target = other
        elif (
            route != base_route
            and deviation > UNCERTAINTY_MARGIN
            and base_idle <= actual_cost * (1.0 + UNCERTAINTY_MARGIN)
        ):
            probe_target = base_route

        if probe_target is not None:
            st["pending_probe"][key] = probe_target
            st["reference"][key][route] = actual_cost
        else:
            st["reference"][key][route] = (
                (1.0 - STEADY_ALPHA) * reference
                + STEADY_ALPHA * actual_cost
            )

    stats = scenario_stats[(family, scenario)]
    stats["calls"] += 1
    stats["probes"] += int(should_probe)
    stats["regrets"].append(regret)

for family in families:
    for scenario in scenario_order:
        stats = scenario_stats.get((family, scenario))
        if not stats or stats["calls"] == 0:
            continue
        regrets = stats["regrets"]
        print(
            f"| {family} | {scenario} | {stats['calls']} | {stats['probes']} | "
            f"{100.0 * stats['probes'] / stats['calls']:.2f} | "
            f"{statistics.mean(regrets):.3f} | {p95(regrets):.3f} | "
            f"{max(regrets):.3f} | {stats['expansions']} |"
        )

all_regrets = []
all_calls = all_probes = all_expansions = 0
for stats in scenario_stats.values():
    all_regrets.extend(stats["regrets"])
    all_calls += stats["calls"]
    all_probes += stats["probes"]
    all_expansions += stats["expansions"]

if all_calls:
    static_regrets = []
    for family, scenario, rep, seq, key, cpu, gpu, observed_winner in ordered_events:
        base_route = baseline[(family, key)][2]
        base_cost = cpu if base_route == "CPU" else gpu
        oracle = min(cpu, gpu)
        static_regrets.append(
            100.0 * (base_cost - oracle) / oracle if oracle > 0 else 0.0
        )

    print()
    print(
        "static_total "
        f"calls={len(static_regrets)} "
        f"mean_regret_pct={statistics.mean(static_regrets):.3f} "
        f"p95_regret_pct={p95(static_regrets):.3f} "
        f"max_regret_pct={max(static_regrets):.3f}"
    )
    print(
        "prototype_total "
        f"calls={all_calls} probes={all_probes} "
        f"probe_rate_pct={100.0 * all_probes / all_calls:.3f} "
        f"mean_regret_pct={statistics.mean(all_regrets):.3f} "
        f"p95_regret_pct={p95(all_regrets):.3f} "
        f"max_regret_pct={max(all_regrets):.3f} "
        f"band_expansions={all_expansions}"
    )
    print(
        "prototype_vs_static "
        f"mean_regret_reduction_pct="
        f"{100.0 * (statistics.mean(static_regrets) - statistics.mean(all_regrets)) / max(statistics.mean(static_regrets), 1e-12):.3f} "
        f"p95_regret_reduction_pct="
        f"{100.0 * (p95(static_regrets) - p95(all_regrets)) / max(p95(static_regrets), 1e-12):.3f}"
    )
