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

def bench(pair, workers, n, repeat, binary):
    p = subprocess.run([str(binary), str(n), str(repeat)], capture_output=True, text=True,
                       check=True, preexec_fn=lambda: os.sched_setaffinity(0, pair))
    result = json.loads(p.stdout.strip())
    return dict(pair=pair, workers=workers, size=n, n=repeat,
                median_wall_ns=int(statistics.median(result["wall_ns"])),
                median_cpu_ns=int(statistics.median(result["cpu_ns"])),
                samples_wall_ns=result["wall_ns"], samples_cpu_ns=result["cpu_ns"],
                checksum=result["checksum"])

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
                    interpretation="Native pthread memory-read benchmark (wall and CPU time); still a feasibility smoke, not proven cache/NUMA advantage")
    (out/"topology.json").write_text(json.dumps(manifest, indent=2)+"\n")
    binary = out.resolve() / "server_locality_native"
    subprocess.run(["cc", "-O3", "-std=c11", "-Wall", "-Wextra", "-pthread",
                    str(Path(__file__).with_name("server_locality_native.c")),
                    "-o", str(binary)], check=True)
    results = []
    for label, pair in cases:
        val = bench(pair, 2, a.items, a.repeats, binary)
        val["label"] = label
        results.append(val)
        print(json.dumps(dict(label=label, pair=pair, median_wall_ns=val["median_wall_ns"], median_cpu_ns=val["median_cpu_ns"])), flush=True)
    (out/"results.json").write_text(json.dumps(results, indent=2)+"\n")
    print("TOPOLOGY_SMOKE_PASS", "CROSS_L3_AVAILABLE" if any(x["label"]=="cross-l3" for x in results) else "CROSS_L3_UNAVAILABLE")
if __name__=="__main__":
    main()
