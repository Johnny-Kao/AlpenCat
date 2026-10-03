#!/usr/bin/env python3
import csv, json, math, pathlib, statistics, sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "results/x64-economics")
files = sorted(root.glob("*.csv"))
if not files:
    raise SystemExit("no csv files")

phases = {}
for path in files:
    rows = []
    with path.open() as f:
        for row in csv.DictReader(f):
            row["n"] = int(row["n"])
            row["serial_ns"] = float(row["serial_ns"])
            row["parallel_ns"] = float(row["parallel_ns"])
            row["rayon_threads"] = int(row["rayon_threads"])
            rows.append(row)
    if rows:
        phases[rows[0]["phase"]] = rows

if "full" not in phases:
    raise SystemExit("missing full baseline")

def first_parallel(rows):
    for r in rows:
        if r["parallel_ns"] < r["serial_ns"]:
            return r["n"]
    return math.inf

def decide(boundary, n):
    return "PARALLEL" if n >= boundary else "SERIAL"

def route_time(r, route):
    return r["parallel_ns"] if route == "PARALLEL" else r["serial_ns"]

def summary_for(rows, boundary):
    regrets, extra = [], []
    for r in rows:
        oracle = min(r["serial_ns"], r["parallel_ns"])
        selected = route_time(r, decide(boundary, r["n"]))
        regrets.append(max(0.0, (selected / oracle - 1.0) * 100.0))
        extra.append(max(0.0, selected - oracle))
    ordered = sorted(regrets)
    return {
        "mean_regret_pct": statistics.mean(regrets),
        "p95_regret_pct": ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)],
        "max_regret_pct": max(regrets),
        "mean_extra_ns_per_call": statistics.mean(extra),
        "total_extra_ns_one_call_each_size": sum(extra),
    }



def bounded_policy(rows, baseline_boundary, max_steps=3, collapse_ratio=1.8):
    if math.isinf(baseline_boundary):
        start = len(rows) - 1
    else:
        start = min(range(len(rows)), key=lambda i: abs(rows[i]["n"] - baseline_boundary))

    tested = []
    def inspect(i):
        if i not in tested:
            tested.append(i)
        r = rows[i]
        return r["parallel_ns"] < r["serial_ns"]

    # First inspect the old boundary.
    start_parallel = inspect(start)
    r0 = rows[start]
    ratio = r0["parallel_ns"] / max(r0["serial_ns"], 1.0)

    # Severe collapse: old parallel route is much slower than serial.
    # Do not chase the new crossover; fail safe to SERIAL.
    if (not start_parallel) and ratio >= collapse_ratio:
        recovered_boundary = math.inf
        mode = "collapse_fallback"
    elif start_parallel:
        i = start - 1
        steps = 1
        while i >= 0 and steps < max_steps and inspect(i):
            i -= 1
            steps += 1
        recovered_boundary = rows[i + 1]["n"]
        mode = "bounded_down"
    else:
        i = start + 1
        steps = 1
        while i < len(rows) and steps < max_steps and not inspect(i):
            i += 1
            steps += 1
        if i < len(rows) and rows[i]["parallel_ns"] < rows[i]["serial_ns"]:
            recovered_boundary = rows[i]["n"]
            mode = "bounded_up_found"
        else:
            # No crossover found inside the bounded search. Prefer serial
            # conservatively rather than paying for an unbounded search.
            recovered_boundary = math.inf
            mode = "bounded_fallback"

    local_rows = [rows[i] for i in tested]
    local_cost = sum(r["serial_ns"] + r["parallel_ns"] for r in local_rows)
    return recovered_boundary, local_rows, local_cost, mode, ratio

baseline_boundary = first_parallel(phases["full"])
results = {"baseline_boundary": None if math.isinf(baseline_boundary) else baseline_boundary, "phases": {}}

for name, rows in phases.items():
    actual_boundary = first_parallel(rows)
    static = summary_for(rows, baseline_boundary)

    # Simulate an actual localized boundary search. Start at the old crossover.
    # If the old point is now SERIAL, walk upward until PARALLEL is found; if it
    # is still PARALLEL, walk downward until the nearest SERIAL point is found.
    # Every inspected point prices one serial + one parallel measurement.
    if math.isinf(baseline_boundary):
        start = len(rows) - 1
    else:
        start = min(range(len(rows)), key=lambda i: abs(rows[i]["n"] - baseline_boundary))

    tested = []
    def test(i):
        if i not in tested:
            tested.append(i)
        return rows[i]["parallel_ns"] < rows[i]["serial_ns"]

    start_parallel = test(start)
    if start_parallel:
        i = start - 1
        while i >= 0 and test(i):
            i -= 1
        recovered_boundary = rows[i + 1]["n"]
    else:
        i = start + 1
        while i < len(rows) and not test(i):
            i += 1
        recovered_boundary = math.inf if i == len(rows) else rows[i]["n"]

    local_rows = [rows[i] for i in tested]
    local_cost = sum(r["serial_ns"] + r["parallel_ns"] for r in local_rows)
    recovered = summary_for(rows, recovered_boundary)
    saved = max(0.0, static["mean_extra_ns_per_call"] - recovered["mean_extra_ns_per_call"])
    break_even = None if saved <= 0 else local_cost / saved

    bounded_boundary, bounded_rows, bounded_cost, bounded_mode, collapse_ratio_observed = bounded_policy(
        rows, baseline_boundary
    )
    bounded = summary_for(rows, bounded_boundary)
    bounded_saved = max(0.0, static["mean_extra_ns_per_call"] - bounded["mean_extra_ns_per_call"])
    bounded_break_even = None if bounded_saved <= 0 else bounded_cost / bounded_saved

    results["phases"][name] = {
        "rayon_threads": rows[0]["rayon_threads"],
        "actual_boundary": None if math.isinf(actual_boundary) else actual_boundary,
        "static_old_boundary": static,
        "recovered_boundary_value": None if math.isinf(recovered_boundary) else recovered_boundary,
        "recovered_boundary": recovered,
        "localized_revalidation_points": [r["n"] for r in local_rows],
        "localized_revalidation_cost_ns": local_cost,
        "saved_ns_per_call_estimate": saved,
        "break_even_calls": break_even,
        "bounded_policy": {
            "mode": bounded_mode,
            "collapse_ratio_at_old_boundary": collapse_ratio_observed,
            "boundary": None if math.isinf(bounded_boundary) else bounded_boundary,
            "tested_points": [r["n"] for r in bounded_rows],
            "cost_ns": bounded_cost,
            "summary": bounded,
            "saved_ns_per_call_estimate": bounded_saved,
            "break_even_calls": bounded_break_even,
        },
    }

(root / "summary.json").write_text(json.dumps(results, indent=2) + "\n")

print("# AlpenCat x64 boundary economics")
print()
print(f"Baseline full-budget crossover: **{results['baseline_boundary']}**")
print()
print("| Phase | Threads | Actual crossover | Static mean regret | Full-search cost | Full-search BE | Bounded mode | Bounded cost | Bounded BE | Bounded mean regret |")
print("|---|---:|---:|---:|---:|---:|---|---:|---:|---:|")
for name, d in results["phases"].items():
    be = "n/a" if d["break_even_calls"] is None else f"{d['break_even_calls']:.2f}"
    ab = "none" if d["actual_boundary"] is None else str(d["actual_boundary"])
    s = d["static_old_boundary"]
    b = d["bounded_policy"]
    bbe = "n/a" if b["break_even_calls"] is None else f"{b['break_even_calls']:.2f}"
    print(f"| {name} | {d['rayon_threads']} | {ab} | {s['mean_regret_pct']:.3f}% | {d['localized_revalidation_cost_ns']/1e6:.3f} ms | {be} | {b['mode']} | {b['cost_ns']/1e6:.3f} ms | {bbe} | {b['summary']['mean_regret_pct']:.3f}% |")
