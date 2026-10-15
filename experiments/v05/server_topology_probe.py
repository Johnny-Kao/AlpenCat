#!/usr/bin/env python3
"""Read-only Linux CPU cache/NUMA topology + paired affinity-locality benchmark.

Only restricts child-process CPU affinity within its inherited cpuset; no root,
BIOS, NUMA policy, persistent host settings, or expensive data-centre simulation.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

SYS = Path("/sys/devices/system/cpu")

def cpulist(raw):
    out = set()
    for part in raw.strip().split(","):
        if not part:
            continue
        pair = part.split("-")
        if len(pair) == 1:
            out.add(int(pair[0]))
        elif len(pair) == 2:
            out.update(range(int(pair[0]), int(pair[1]) + 1))
        else:
            raise ValueError(part)
    return out

def read(path):
    try:
        return path.read_text().strip()
    except OSError:
        return None

def topology(allowed):
    cpus = {}
    for cpu in sorted(allowed):
        root = SYS / ("cpu" + str(cpu))
        cache = root / "cache"
        shared = []
        if cache.is_dir():
            for index in sorted(cache.glob("index*")):
                level = read(index / "level")
                typ = read(index / "type")
                group = read(index / "shared_cpu_list")
                if level == "3" and typ in ("Unified", "Data") and group:
                    shared.append(sorted(cpulist(group) & allowed))
        nodes = [int(x.name[4:]) for x in root.glob("node[0-9]*")]
        cpus[cpu] = dict(package_id=read(root / "topology/physical_package_id"),
                         core_id=read(root / "topology/core_id"),
                         l3_allowed_groups=shared, numa_nodes=nodes)
    l3 = sorted({tuple(g) for c in cpus.values() for g in c["l3_allowed_groups"] if g})
    numa = sorted({n for c in cpus.values() for n in c["numa_nodes"]})
    return dict(cpus=cpus, l3_groups=[list(g) for g in l3], numa_nodes=numa)

def selected_pairs(info, allowed):
    # Prefer distinct physical cores for a fair fixed two-worker action comparison.
    keys = lambda xs: {(info["cpus"][c]["package_id"], info["cpus"][c]["core_id"]) for c in xs}
    def valid(pair):
        return len(pair) == 2 and len(keys(pair)) == 2
    groups = [g for g in info["l3_groups"] if len(g) >= 2]
    same = next((g[:2] for g in groups if valid(g[:2])), None)
    distinct = None
    for i, a in enumerate(groups):
        for b in groups[i + 1:]:
            if a and b and a[0] != b[0] and valid([a[0], b[0]]):
                distinct = [a[0], b[0]]
                break
        if distinct:
            break
    if same is None:
        same = next(([a,b] for a in allowed for b in allowed if a < b and valid([a,b])), None)
    cases = []
    if same:
        cases.append(("same-l3" if any(set(same) <= set(g) for g in groups) else "generic-two-core", same))
    if distinct:
        cases.append(("cross-l3", distinct))
    return cases

def bench(pair, workers, n, repeat):
    # Pure Python benchmark measures memory access locality without CPU-bound
    # multiprocessing start method confusion; child captures process CPU and wall.
    program = """
import os, time, json, statistics
from concurrent.futures import ThreadPoolExecutor
from array import array
n=int(os.environ['N']); rounds=int(os.environ['ROUNDS'])
# Deliberate shared-memory strided reads. GIL means this is a harness smoke,
# NOT a multi-core C++/Rust speedup claim.
a=array('Q',range(n))
def task(start):
 s=0
 for i in range(start,n,64): s+=a[i]
 return s
samples=[]
for i in range(rounds+3):
 t=time.perf_counter_ns()
 with ThreadPoolExecutor(max_workers=2) as pool:
  result=list(pool.map(task,(0,32)))
 dt=time.perf_counter_ns()-t
 if i>=3: samples.append(dt)
 print(json.dumps({'checksum':result[0]+result[1],'ns':dt,'round':i}))
"""
    env = os.environ.copy()
    env.update(N=str(n), ROUNDS=str(repeat))
    p = subprocess.run(["python3", "-c", program], capture_output=True, text=True,
                       env=env, check=True, preexec_fn=lambda: os.sched_setaffinity(0, pair))
    rows = [json.loads(line) for line in p.stdout.splitlines()]
    checksum = set(x["checksum"] for x in rows)
    assert len(checksum) == 1
    data = [x["ns"] for x in rows if x["round"] >= 3]
    return dict(pair=pair, workers=workers, size=n, n=len(data),
                median_ns=int(statistics.median(data)), samples_ns=data,
                checksum=next(iter(checksum)))

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--repeats", type=int, default=11)
    parser.add_argument("--items", type=int, default=262144)
    a = parser.parse_args()
    out = Path(a.out); out.mkdir(parents=True, exist_ok=True)
    allowed = sorted(os.sched_getaffinity(0))
    info = topology(set(allowed))
    cases = selected_pairs(info, allowed)
    manifest = dict(uname=platform.uname()._asdict(), allowed=allowed, topology=info,
                    cases=[dict(label=x,pair=y) for x,y in cases],
                    interpretation="Two-worker Python/GIL test is *only* topology and execution harness smoke, not performance economics")
    (out/"topology.json").write_text(json.dumps(manifest, indent=2)+"\n")
    results = []
    for label, pair in cases:
        val = bench(pair, 2, a.items, a.repeats)
        val["label"] = label
        results.append(val)
        print(json.dumps(dict(label=label, pair=pair, median_ns=val["median_ns"])), flush=True)
    (out/"results.json").write_text(json.dumps(results, indent=2)+"\n")
    print("TOPOLOGY_SMOKE_PASS", "CROSS_L3_AVAILABLE" if any(x["label"]=="cross-l3" for x in results) else "CROSS_L3_UNAVAILABLE")
if __name__=="__main__":
    main()
