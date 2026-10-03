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



def economic_gate(rows, baseline_rows, baseline_boundary,
                  demand_threshold=32, slowdown_threshold=1.50):
    if math.isinf(baseline_boundary):
        return {"trigger": False, "reason": "no_baseline_boundary"}

    idx = min(range(len(rows)), key=lambda i: abs(rows[i]["n"] - baseline_boundary))
    base = baseline_rows[idx]
    cur = rows[idx]

    baseline_route = "PARALLEL" if base["parallel_ns"] < base["serial_ns"] else "SERIAL"
    baseline_route_ns = route_time(base, baseline_route)
    current_route_ns = route_time(cur, baseline_route)
    slowdown = current_route_ns / max(baseline_route_ns, 1.0)

    # A severe slowdown can trigger immediately from the route that would
    # already have been executed. Otherwise we wait for repeated near-boundary
    # demand before paying for any alternate-path measurement.
    immediate = slowdown >= slowdown_threshold
    return {
        "trigger": immediate,
        "reason": "route_slowdown" if immediate else "demand_threshold",
        "slowdown_ratio": slowdown,
        "baseline_route": baseline_route,
        "demand_threshold": demand_threshold,
    }


def gated_economics(rows, baseline_rows, baseline_boundary,
                    demand_threshold=32, slowdown_threshold=1.50):
    gate = economic_gate(rows, baseline_rows, baseline_boundary,
                         demand_threshold=demand_threshold,
                         slowdown_threshold=slowdown_threshold)

    static = summary_for(rows, baseline_boundary)
    bounded_boundary, bounded_rows, bounded_cost, bounded_mode, _ = bounded_policy(
        rows, baseline_boundary
    )
    bounded = summary_for(rows, bounded_boundary)

    # Estimate economics for a stream of near-boundary calls. Immediate severe
    # slowdown triggers before accumulating demand; otherwise wait N calls.
    wait_calls = 0 if gate["trigger"] else demand_threshold
    stale_loss_per_call = static["mean_extra_ns_per_call"]
    pre_trigger_loss = stale_loss_per_call * wait_calls
    post_trigger_saved_per_call = max(
        0.0, static["mean_extra_ns_per_call"] - bounded["mean_extra_ns_per_call"]
    )

    total_activation_cost = pre_trigger_loss + bounded_cost
    break_even_after_event = None
    if post_trigger_saved_per_call > 0:
        break_even_after_event = wait_calls + bounded_cost / post_trigger_saved_per_call

    return {
        "gate": gate,
        "wait_calls": wait_calls,
        "pre_trigger_stale_loss_ns": pre_trigger_loss,
        "bounded_mode": bounded_mode,
        "bounded_cost_ns": bounded_cost,
        "bounded_summary": bounded,
        "activation_cost_ns": total_activation_cost,
        "break_even_calls_after_event": break_even_after_event,
    }

baseline_boundary = first_parallel(phases["full"])
results = {"baseline_boundary": None if math.isinf(baseline_boundary) else baseline_boundary, "phases": {}}
baseline_rows = phases["full"]

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
        "economic_gate": gated_economics(
            rows, baseline_rows, baseline_boundary,
            demand_threshold=32, slowdown_threshold=1.50
        ),
    }

(root / "summary.json").write_text(json.dumps(results, indent=2) + "\n")

print("# AlpenCat x64 boundary economics")
print()
print(f"Baseline full-budget crossover: **{results['baseline_boundary']}**")
print()
print("| Phase | Static mean regret | Route slowdown | Gate | Wait calls | Bounded cost | Event break-even |")
print("|---|---:|---:|---|---:|---:|---:|")
for name, d in results["phases"].items():
    s = d["static_old_boundary"]
    g = d["economic_gate"]
    gate = g["gate"]
    be = "n/a" if g["break_even_calls_after_event"] is None else f"{g['break_even_calls_after_event']:.2f}"
    print(f"| {name} | {s['mean_regret_pct']:.3f}% | {gate['slowdown_ratio']:.3f}x | {gate['reason']} | {g['wait_calls']} | {g['bounded_cost_ns']/1e6:.3f} ms | {be} |")


print()
print("## Demand-threshold sensitivity (slowdown threshold = 1.50x)")
print()
print("| Phase | 32 | 128 | 512 | 1024 |")
print("|---|---:|---:|---:|---:|")
for name, rows in phases.items():
    vals = []
    for threshold in (32, 128, 512, 1024):
        g = gated_economics(
            rows, baseline_rows, baseline_boundary,
            demand_threshold=threshold, slowdown_threshold=1.50
        )
        be = g["break_even_calls_after_event"]
        if g["gate"]["trigger"]:
            vals.append("immediate")
        elif be is None:
            vals.append(f"wait {threshold}")
        else:
            vals.append(f"{be:.1f}")
    print(f"| {name} | " + " | ".join(vals) + " |")
