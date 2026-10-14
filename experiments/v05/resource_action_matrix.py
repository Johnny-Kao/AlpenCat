#!/usr/bin/env python3
"""Linux x86 resource/action opportunity screening using existing Rust workloads."""
import argparse
import json
import os
import pathlib
import statistics
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from experiments.convergence.run_universal_cross_machine import (
    WORKLOADS, build_examples, executable, hardware_manifest, total_memory_bytes,
)

def read(path):
    try:
        return pathlib.Path(path).read_text().strip()
    except OSError:
        return None

def env_snapshot():
    allowed = sorted(os.sched_getaffinity(0))
    cg = pathlib.Path("/sys/fs/cgroup")
    return dict(
        affinity_cpus=allowed, affinity_count=len(allowed),
        cpu_max=read(cg / "cpu.max"),
        cpuset_effective=read(cg / "cpuset.cpus.effective"),
        cpu_stat=read(cg / "cpu.stat"),
        memory_current=read(cg / "memory.current"),
        memory_max=read(cg / "memory.max"),
        cpu_psi=read("/proc/pressure/cpu"),
        memory_psi=read("/proc/pressure/memory"),
    )

def pct(values, frac):
    ordered = sorted(values)
    return ordered[min(len(ordered)-1, round((len(ordered)-1)*frac))]

def run_one(exe, affinity, size, label, repetitions):
    import resource
    proc_env = os.environ.copy()
    proc_env.update(ALPENCAT_REGIME=label, ALPENCAT_REPEATS=str(repetitions),
                    ALPENCAT_WARMUP_PAIRS="2", ALPENCAT_START_BOUNDARY=str(size),
                    ALPENCAT_BOOTSTRAP="1")
    # Each process is confined to a controllable subset of the runner's permitted CPUs.
    # No privilege escalation, taskset dependency, or persistent host modification.
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.perf_counter_ns()
    p = subprocess.run([str(exe)], capture_output=True, text=True, check=True,
                       env=proc_env, cwd=ROOT, preexec_fn=lambda: os.sched_setaffinity(0, affinity))
    wall = time.perf_counter_ns() - start
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    rows = [json.loads(x) for x in p.stdout.splitlines() if x.startswith("{")]
    points = [x for x in rows if x.get("record_type")=="point"]
    event = next(x for x in rows if x.get("record_type")=="revalidation")
    if not points:
        raise AssertionError("No workload measurements")
    if any(x.get("output_equivalent") is False for x in points):
        raise AssertionError("Backend result mismatch")
    fixed_serial = sum(statistics.median(x["serial_samples_ns"]) for x in points)
    cpu_valid = all(x.get("cpu_route_available") and x.get("cpu_samples_ns") for x in points)
    fixed_cpu = sum(statistics.median(x["cpu_samples_ns"]) for x in points) if cpu_valid else None
    auto = sum(statistics.median(x["auto_samples_ns"]) for x in points)
    best = min([fixed_serial] + ([fixed_cpu] if cpu_valid else []))
    oracle = sum(min([statistics.median(x["serial_samples_ns"])] +
                     ([statistics.median(x["cpu_samples_ns"])] if x.get("cpu_route_available") and x.get("cpu_samples_ns") else [])) for x in points)
    return dict(regime=label, affinity_count=len(affinity), affinity_cpus=affinity,
                fixed_serial_ns=fixed_serial, fixed_cpu_ns=fixed_cpu, fixed_best_ns=best,
                per_size_oracle_ns=oracle, auto_ns=auto, recoverable_opportunity_ns=best-oracle,
                auto_vs_fixed_best_ns=best-auto, auto_oracle_gap_ns=auto-oracle,
                estimated_bootstrap_ns=event["revalidation_elapsed_ns"],
                estimated_net_100_cycles_ns=100*(best-auto)-event["revalidation_elapsed_ns"],
                backend_by_size=[dict(size=x["work_items"], backend=x["auto_backend"],
                                      serial_p50_ns=statistics.median(x["serial_samples_ns"]),
                                      cpu_p50_ns=statistics.median(x["cpu_samples_ns"]) if x.get("cpu_samples_ns") else None,
                                      auto_p50_ns=statistics.median(x["auto_samples_ns"]),
                                      auto_p90_ns=pct(x["auto_samples_ns"], .9)) for x in points],
                event_status=event["status"], stale=event["boundary_stale"],
                wall_process_ns=wall, child_cpu_delta_s=(after.ru_utime-before.ru_utime)+(after.ru_stime-before.ru_stime),
                child_rss_peak_kib=after.ru_maxrss, output_equivalent=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if args.repeats < 3 or args.repeats % 2 == 0:
        parser.error("--repeats must be odd and >=3")
    if sys.platform != "linux":
        parser.error("Linux only: needs sched_setaffinity")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    cpus = sorted(os.sched_getaffinity(0))
    if not cpus:
        raise RuntimeError("Empty schedulable CPU set")
    envelopes = [(f"affinity-{min(n,len(cpus))}", cpus[:min(n,len(cpus))]) for n in (4, 2, 1)]
    unique = []
    for name, subset in envelopes:
        if subset not in [x[1] for x in unique]:
            unique.append((name, subset))
    baseline = dict(hardware=hardware_manifest(total_memory_bytes()), before=env_snapshot())
    build_examples(ROOT)
    results = []
    # The eligible action space is currently Serial/CPU; affinity is a controlled
    # resource-envelope intervention, not a claim that the SDK can set CPU affinity.
    for workload, example, boundary in WORKLOADS:
        for phase, (regime, subset) in enumerate(unique + list(reversed(unique[:-1]))):
            row = run_one(executable(ROOT, example), subset, boundary, regime, args.repeats)
            row["workload"] = workload
            row["example"] = example
            row["pass_kind"] = "recovery" if phase >= len(unique) else "transition"
            results.append(row)
            print(json.dumps(row), flush=True)
            time.sleep(.15)
    baseline["after"] = env_snapshot()
    baseline["note"] = "Affinities imposed per subprocess; fixed routes measured before Auto; separate processes do not exercise a single Runtime lifecycle."
    (out / "environment.json").write_text(json.dumps(baseline, indent=2)+"\n")
    (out / "matrix.json").write_text(json.dumps(results, indent=2)+"\n")
    assert all(x["output_equivalent"] for x in results)
    print("RESOURCE ACTION OPPORTUNITY SCREENING PASS; ECONOMIC GO UNDETERMINED")

if __name__ == "__main__":
    main()
