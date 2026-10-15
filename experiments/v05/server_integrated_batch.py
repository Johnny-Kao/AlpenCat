#!/usr/bin/env python3
"""One-shot Linux Server CPU research gate: inventory, native workload and evidence.
This is a falsification harness; absent cross-domain CPUs is INCONCLUSIVE, not PASS.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/"experiments"/"v05"))
from server_topology_probe import topology, selected_pairs, cpulist, read

def compile_native(directory):
    target=directory/"server_locality_native"
    cmd=["cc","-O3","-std=c11","-Wall","-Wextra","-Werror","-pthread",
         str(Path(__file__).with_name("server_locality_native.c")),"-o",str(target)]
    subprocess.run(cmd,check=True)
    return target.resolve()

def run(exe, cpus, elements, repetitions):
    p=subprocess.run([str(exe),str(elements),str(repetitions)],check=True,
                     capture_output=True,text=True,
                     preexec_fn=lambda:os.sched_setaffinity(0,cpus))
    r=json.loads(p.stdout)
    assert len(r["wall_ns"]) == repetitions and len(r["cpu_ns"]) == repetitions
    assert all(v>0 for v in r["wall_ns"]+r["cpu_ns"])
    return r

def median(values):
    return int(statistics.median(values))

def pct_change(a,b):
    return round(100*(a/b-1),3) if b else None

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--out",type=Path,required=True)
    ap.add_argument("--reps",type=int,default=15)
    args=ap.parse_args()
    if args.reps<7 or args.reps>99: ap.error("reps 7..99")
    out=args.out.resolve();out.mkdir(parents=True,exist_ok=True)
    allowed=sorted(os.sched_getaffinity(0))
    topo=topology(set(allowed))
    cases=selected_pairs(topo,allowed)
    # Preserve a one-core envelope as negative control. Two-core paths represent
    # available execution resources, not an optimization policy.
    if allowed: cases.append(("one-cpu-negative-control",[allowed[0]]))
    assert cases
    cg=Path("/sys/fs/cgroup")
    resource=dict(affinity=allowed,cpu_max=read(cg/"cpu.max"),
                  cpu_stat_before=read(cg/"cpu.stat"),
                  cpu_pressure=read(Path("/proc/pressure/cpu")),
                  memory_pressure=read(Path("/proc/pressure/memory")),
                  memory_max=read(cg/"memory.max"),
                  numa_distance={str(n):read(Path("/sys/devices/system/node")/("node"+str(n))/"distance")
                                 for n in topo["numa_nodes"]})
    (out/"manifest.json").write_text(json.dumps(dict(platform=platform.uname()._asdict(),
               topology=topo,resource=resource,cases=cases),indent=2)+"\n")
    exe=compile_native(out)
    measurements=[]
    # Three footprint classes; rotate case order in each pass. The original native
    # test still includes pthread startup, so only gross differences are interpretable.
    footprints=(16_384,262_144,4_194_304)
    for cycle in range(2):
        for size in footprints:
            plan=cases if cycle==0 else list(reversed(cases))
            for label,cpus in plan:
                result=run(exe,cpus,size,args.reps)
                record=dict(cycle=cycle,items=size,label=label,cpus=cpus,
                            wall_ns=result["wall_ns"],cpu_ns=result["cpu_ns"],
                            wall_p50_ns=median(result["wall_ns"]),
                            cpu_p50_ns=median(result["cpu_ns"]),
                            checksum=result["checksum"])
                measurements.append(record)
                print("CASE "+json.dumps({k:record[k] for k in ("cycle","items","label","cpus","wall_p50_ns","cpu_p50_ns")}),flush=True)
    # All affinity placements must produce identical results per fixed input.
    for n in footprints:
        assert len({m["checksum"] for m in measurements if m["items"]==n})==1
    comparisons=[]
    for n in footprints:
        for metric in ("wall_p50_ns","cpu_p50_ns"):
            a=[m[metric] for m in measurements if m["items"]==n and m["label"]=="same-l3"]
            b=[m[metric] for m in measurements if m["items"]==n and m["label"]=="cross-l3"]
            if len(a)==2 and len(b)==2:
                comparisons.append(dict(items=n,metric=metric,same_l3=a,cross_l3=b,
                                        cross_vs_same_pct=pct_change(median(b),median(a)),
                                        direction_consistent=all((b[i]>a[i])==(b[0]>a[0]) for i in range(2))))
    outcomes=dict(harness_correctness="PASS",
                  cross_l3_coverage="AVAILABLE" if comparisons else "BLOCKED_NO_DISTINCT_L3",
                  topology_available=bool(topo["l3_groups"]),
                  policy_performance="UNDECIDED",
                  cause="This benchmark has thread startup overhead and uncontrolled NUMA page placement; no policy tuning claim.")
    (out/"measurements.json").write_text(json.dumps(measurements,indent=2)+"\n")
    (out/"comparisons.json").write_text(json.dumps(comparisons,indent=2)+"\n")
    (out/"decision.json").write_text(json.dumps(outcomes,indent=2)+"\n")
    print("BATCH_DECISION "+json.dumps(outcomes),flush=True)

if __name__=="__main__":
    main()
