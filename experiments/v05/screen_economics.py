#!/usr/bin/env python3
"""v0.5 exploratory paired economics, reusing the frozen three workload examples.

These examples time fixed routes before Auto, so this is a screening experiment,
not a randomized end-to-end lifecycle proof. See research report.
"""
import argparse
import json
import pathlib
import platform
import statistics
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2]))
from experiments.convergence.run_universal_cross_machine import (
    WORKLOADS, build_examples, executable, hardware_manifest,
    start_pressure, stop_pressure, total_memory_bytes,
)

ROOT = pathlib.Path(__file__).resolve().parents[2]
REGIMES = (("baseline-full", "none"), ("cpu-pressure", "cpu"), ("recovery", "none"))


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * fraction))]


def run_case(binary, output, regime, boundary, bootstrap, repeats):
    import os
    env = os.environ.copy()
    env.update(ALPENCAT_REGIME=regime, ALPENCAT_START_BOUNDARY=str(boundary),
               ALPENCAT_BOOTSTRAP="1" if bootstrap else "0",
               ALPENCAT_REPEATS=str(repeats), ALPENCAT_WARMUP_PAIRS="2",
               ALPENCAT_REVALIDATION_POINTS="3")
    # GNU time returns CPU seconds and maximum *child-process* RSS, unlike
    # cumulative RUSAGE_CHILDREN maxrss.
    timing = output.with_suffix(".resource.txt")
    with output.open("w", encoding="utf-8") as handle:
        subprocess.run(["/usr/bin/time", "-f", "user_s=%U\\nsystem_s=%S\\nmaxrss_kib=%M",
                        "-o", str(timing), str(binary)],
                       cwd=ROOT, env=env, stdout=handle, check=True)
    rows = [json.loads(line) for line in output.read_text().splitlines()]
    event = next(row for row in rows if row["record_type"] == "revalidation")
    points = [row for row in rows if row["record_type"] == "point"]
    resources = dict(line.split("=", 1) for line in timing.read_text().splitlines())
    if any(p["output_equivalent"] is False for p in points):
        raise AssertionError("Serial/CPU output mismatch")
    def total(mode):
        values = []
        for point in points:
            samples = point.get(mode + "_samples_ns")
            if not samples:
                return None
            values.append(statistics.median(samples))
        return sum(values)
    fixed_serial, fixed_cpu, auto = (total(k) for k in ("serial", "cpu", "auto"))
    if auto is None:
        raise AssertionError("Missing Auto measurements")
    fixed_best = min(v for v in (fixed_serial, fixed_cpu) if v is not None)
    saving = fixed_best - auto
    overhead = event["revalidation_elapsed_ns"]
    return {
        "regime": regime, "status": event["status"],
        "boundary_stale": event["boundary_stale"],
        "points": len(points), "parallelism": points[0]["parallelism"],
        "fixed_serial_ns": fixed_serial, "fixed_cpu_ns": fixed_cpu,
        "auto_ns": auto, "fixed_best_ns": fixed_best,
        "per_cycle_savings_ns": saving, "adaptation_cost_ns": overhead,
        "break_even_cycles": (None if saving <= 0 else (overhead + saving - 1) // saving),
        "net_100_cycles_ns": 100 * saving - overhead,
        "auto_backend": [p["auto_backend"] for p in points],
        "auto_p50_ns": statistics.median([statistics.median(p["auto_samples_ns"]) for p in points]),
        "auto_p90_point_ns": percentile([statistics.median(p["auto_samples_ns"]) for p in points], .9),
        "user_cpu_s": float(resources["user_s"]),
        "system_cpu_s": float(resources["system_s"]),
        "peak_child_rss_kib": int(resources["maxrss_kib"]),
        "all_output_equivalent": all(p["output_equivalent"] is not False for p in points),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=pathlib.Path, required=True)
    ap.add_argument("--repeats", type=int, default=5)
    args = ap.parse_args()
    if args.repeats < 3 or args.repeats % 2 != 1:
        ap.error("--repeats must be odd and >= 3")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    build_examples(ROOT)
    manifest = hardware_manifest(total_memory_bytes())
    manifest["python"] = sys.version
    manifest["kernel"] = platform.release()
    results = []
    for workload, example, initial_boundary in WORKLOADS:
        boundary = initial_boundary
        for regime, pressure in REGIMES:
            processes = []
            try:
                processes, _ = start_pressure(pressure, total_memory_bytes())
                dest = out / f"{workload}-{regime}.jsonl"
                row = run_case(executable(ROOT, example), dest, regime, boundary,
                               regime == "baseline-full", args.repeats)
                row["workload"] = workload
                row["example"] = example
                results.append(row)
                for line in dest.read_text().splitlines():
                    event = json.loads(line)
                    if event["record_type"] == "revalidation":
                        boundary = event["published_serial_max_items"]
                        break
                print(json.dumps(row), flush=True)
            finally:
                stop_pressure(processes)
            time.sleep(.25)
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (out / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
    # Always preserve negative evidence. Fail CI only for missing/equiv-invalid results.
    assert len(results) == len(WORKLOADS) * len(REGIMES)
    assert all(row["all_output_equivalent"] for row in results)
    print("SCREENING COMPLETE: economic GO/STOP not implied by process exit status")


if __name__ == "__main__":
    main()
