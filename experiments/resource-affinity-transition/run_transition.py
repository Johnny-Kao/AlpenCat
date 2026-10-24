#!/usr/bin/env python3
"""Live process: actual Linux sched_setaffinity 1 -> 2 -> 1; paired CPU/Wall comparisons."""
import argparse
import csv
import json
import os
import random
import statistics
import subprocess
import time

MODES = ("fixed_one", "fixed_two", "direct_event", "alpencat")
SIZES = ("light", "heavy")
SEQUENCE = (1, 2, 1)
ROUNDS = 7

def single_run(binary, mode, size, cpu_ids):
    # Each subprocess starts with its normal inherited affinity. Linux affinity
    # is per thread: child MAIN thread is changed before each GO; workers are
    # newly created each phase and inherit that affinity (verified in Rust).
    child = subprocess.Popen(
        [binary, mode, size], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    rows = []
    try:
        ready = child.stdout.readline().strip()
        if not ready.startswith("READY pid="):
            raise RuntimeError(f"worker did not start: {ready}")
        assert int(ready.split("=")[1]) == child.pid
        for phase, count in enumerate(SEQUENCE):
            mask = set(cpu_ids[:count])
            transition_start = time.perf_counter_ns()
            os.sched_setaffinity(child.pid, mask)  # ACTUAL LINUX CPU AFFINITY
            transition_ns = time.perf_counter_ns() - transition_start
            confirmed = os.sched_getaffinity(child.pid)
            if confirmed != mask:
                raise AssertionError(f"affinity mismatch {confirmed} != {mask}")
            child.stdin.write("GO\n")
            child.stdin.flush()
            line = child.stdout.readline().strip()
            if not line.startswith("PHASE "):
                raise RuntimeError(f"missing phase response {mode} {size} {phase}: {line}")
            result = dict(part.split("=", 1) for part in line.split()[1:])
            if (int(result["phase"]) != phase or int(result["actual_allowed"]) != count
                    or result["mode"] != mode or result["size"] != size):
                raise AssertionError(f"live affinity not honored: {result}")
            if mode == "fixed_one" and int(result["workers"]) != 1:
                raise AssertionError(result)
            if mode == "fixed_two" and int(result["workers"]) != 2:
                raise AssertionError(result)
            if mode in ("direct_event", "alpencat") and int(result["workers"]) != count:
                raise AssertionError(f"non-adaptive worker decision: {result}")
            rows.append(dict(phase=phase, cpus=count, workers=int(result["workers"]),
                             checksum=result["checksum"],
                             wall_s=int(result["wall_ns"]) / 1e9,
                             cpu_s=int(result["cpu_ns"]) / 1e9,
                             observe_ms=int(result["observe_ns"]) / 1e6,
                             transition_ms=transition_ns / 1e6))
        child.stdin.close()
        rc = child.wait(timeout=30)
        if rc != 0:
            raise RuntimeError(f"{mode} failed rc={rc} stderr={child.stderr.read()}")
        return rows
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()

def median(items):
    return statistics.median(items)

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--binary", required=True)
    p.add_argument("--output", default="affinity-economics.json")
    args = p.parse_args()
    allowed = sorted(os.sched_getaffinity(0))
    if len(allowed) < 2:
        raise RuntimeError(f"need >=2 available CPU ids; got {allowed}")
    selected = allowed[:2]
    rng = random.Random(20261024)
    records = []
    for size in SIZES:
        for mode in MODES:
            single_run(args.binary, mode, size, selected)  # warmup outside trials
        for round_id in range(ROUNDS):
            order = list(MODES)
            rng.shuffle(order)
            per_round = {}
            for mode in order:
                phases = single_run(args.binary, mode, size, selected)
                if [r["cpus"] for r in phases] != list(SEQUENCE):
                    raise AssertionError("did not execute real 1->2->1 transitions")
                per_round[mode] = phases
                for r in phases:
                    records.append(dict(size=size, round=round_id, mode=mode, **r))
            for phase in range(3):
                expected = {per_round[m][phase]["checksum"] for m in MODES}
                if len(expected) != 1:
                    raise AssertionError(f"different computations {size} round={round_id} phase={phase}: {expected}")
    summary = {}
    for size in SIZES:
        trials = {}
        for mode in MODES:
            mode_rows = [x for x in records if x["size"] == size and x["mode"] == mode]
            trials[mode] = {
                field: [sum(r[field] for r in mode_rows if r["round"] == i)
                        for i in range(ROUNDS)]
                for field in ("wall_s", "cpu_s", "observe_ms")
            }
        comparisons = {}
        for baseline in ("fixed_one", "fixed_two", "direct_event", "best_fixed"):
            comparisons[baseline] = {}
            for metric in ("wall_s", "cpu_s"):
                b = ([min(a, c) for a, c in zip(trials["fixed_one"][metric], trials["fixed_two"][metric])]
                     if baseline == "best_fixed" else trials[baseline][metric])
                a = trials["alpencat"][metric]
                gains = [100 * (x - y) / x for x, y in zip(b, a)]
                comparisons[baseline][metric] = dict(
                    median_gain_pct=round(median(gains), 3),
                    positive_rounds_over_2pct=sum(g > 2 for g in gains),
                    paired_pct=[round(g, 2) for g in gains],
                    stable=sum(g > 2 for g in gains) >= 6)
        verdict = {
            "adaptive_vs_best_static": all(comparisons["best_fixed"][x]["stable"] for x in ("wall_s", "cpu_s")),
            "alpencat_vs_direct_event": all(comparisons["direct_event"][x]["stable"] for x in ("wall_s", "cpu_s"))
        }
        verdict["distinct_stable_compute_gain"] = all(verdict.values())
        summary[size] = dict(
            modes={m: {metric: round(median(v), 6)
                       for metric, v in trials[m].items()}
                   for m in MODES},
            comparisons=comparisons, verdict=verdict)
        print(f"SCENARIO {size} allowed_cpus=1->2->1")
        for m in MODES:
            print(f"MEDIAN size={size} mode={m} wall_s={summary[size]['modes'][m]['wall_s']:.6f} "
                  f"cpu_s={summary[size]['modes'][m]['cpu_s']:.6f} "
                  f"observe_ms={summary[size]['modes'][m]['observe_ms']:.4f}")
        for baseline, metrics in comparisons.items():
            w, c = metrics["wall_s"], metrics["cpu_s"]
            print(f"PAIRED_GAIN size={size} baseline={baseline} wall_pct={w['median_gain_pct']:.2f} "
                  f"cpu_pct={c['median_gain_pct']:.2f} wall_positive={w['positive_rounds_over_2pct']}/7 "
                  f"cpu_positive={c['positive_rounds_over_2pct']}/7")
        print(f"VERDICT size={size} distinct_stable_compute_gain={verdict['distinct_stable_compute_gain']}")
    overall = all(s["verdict"]["distinct_stable_compute_gain"] for s in summary.values())
    report = dict(
        experiment="real-cpu-affinity-transition-v1",
        cpu_mask=selected, runner=os.uname().release,
        github_run_id=os.environ.get("GITHUB_RUN_ID"), sequence=SEQUENCE,
        data=records, summary=summary,
        distinct_stable_compute_gain=overall,
        limitations=[
            "real Linux thread affinity, not cgroup cpu.max quota",
            "CPU-bound deterministic block-compute application, not third-party production workload",
            "two CPUs max; no claim about memory or heterogeneous CPU hardware",
            "direct_event has equal transition event timing and OS access as AlpenCat",
            "negative economic outcomes are valid scientific results",
        ]
    )
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
    with open(args.output.replace(".json", ".csv"), "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(records[0]))
        writer.writeheader()
        writer.writerows(records)
    print("OVERALL_DISTINCT_STABLE_COMPUTE_GAIN=" + str(overall).upper())
    print("A NO-GO is a valid economics finding; CI tests only experiment integrity.")

if __name__ == "__main__":
    main()
