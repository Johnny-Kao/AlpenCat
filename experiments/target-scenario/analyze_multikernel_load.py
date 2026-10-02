#!/usr/bin/env python3
import glob, os, random, re, statistics
from collections import defaultdict

random.seed(20261002)

patterns = {
    "fft": re.compile(
        r"fft_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
    "reduction": re.compile(
        r"reduction_result n=(\d+) reps=(\d+) cpu_ns=([0-9.]+) "
        r"gpu_host_ns=([0-9.]+) gpu_device_ns=([0-9.]+) winner=(CPU|GPU)"
    ),
}

rows = defaultdict(list)
for path in sorted(glob.glob("multikernel_load/*.txt")):
    base = os.path.basename(path)
    if base.startswith("runner_") or base.startswith("load_") or base.startswith("vm_"):
        continue
    family = next((f for f in patterns if base.startswith(f + "_")), None)
    if family is None:
        continue
    stem = base[:-4]
    rest = stem[len(family)+1:]
    pos = rest.rfind("_")
    if pos < 0:
        continue
    scenario = rest[:pos]
    try:
        rep = int(rest[pos+1:])
    except ValueError:
        continue
    text = open(path, encoding="utf-8").read()
    for line in text.splitlines():
        m = patterns[family].search(line)
        if not m:
            continue
        n = int(m.group(1))
        cpu = float(m.group(3))
        gpu = float(m.group(4))
        winner = m.group(6)
        rows[(family, scenario, n)].append((cpu, gpu, winner, rep))

def median(xs):
    return statistics.median(xs)

def boot_prob(vals, iters=6000):
    ds=[c-g for c,g,_,_ in vals]
    n=len(ds)
    boots=[]
    for _ in range(iters):
        boots.append(statistics.mean(ds[random.randrange(n)] for _ in range(n)))
    boots.sort()
    lo=boots[int(.025*iters)]
    hi=boots[int(.975*iters)]
    p=sum(x>0 for x in boots)/iters
    return lo,hi,p

summary={}
for key, vals in rows.items():
    cpu=median([v[0] for v in vals])
    gpu=median([v[1] for v in vals])
    winner="CPU" if cpu<=gpu else "GPU"
    lo,hi,p=boot_prob(vals)
    summary[key]=(cpu,gpu,winner,lo,hi,p)

print("# Multi-kernel controlled-load crossover")
print("| family | scenario | n | cpu_us | gpu_us | winner | ci95_lo_us | ci95_hi_us | p_gpu |")
print("|---|---|---:|---:|---:|---|---:|---:|---:|")
for (family,scenario,n) in sorted(summary):
    cpu,gpu,winner,lo,hi,p=summary[(family,scenario,n)]
    print(f"| {family} | {scenario} | {n} | {cpu/1000:.3f} | {gpu/1000:.3f} | {winner} | {lo/1000:.3f} | {hi/1000:.3f} | {p:.4f} |")

print()
print("# Idle-pre policy regret under load")
print("| family | scenario | mean_regret_pct | max_regret_pct | changed_routes | points | band_points | band_changed |")
print("|---|---|---:|---:|---:|---:|---:|---:|")
families=sorted({k[0] for k in summary})
scenarios=sorted({k[1] for k in summary if k[1]!="idle_pre"})
for family in families:
    baseline={n:v for (f,s,n),v in summary.items() if f==family and s=="idle_pre"}
    for scenario in scenarios:
        regrets=[]; changed=0; points=0; band=0; band_changed=0
        for n,base in baseline.items():
            key=(family,scenario,n)
            if key not in summary: continue
            cpu,gpu,winner,*_=summary[key]
            base_winner=base[2]
            selected=cpu if base_winner=="CPU" else gpu
            oracle=min(cpu,gpu)
            regrets.append(100*(selected-oracle)/oracle)
            ch=winner!=base_winner
            changed += ch
            points += 1
            if 100_000 <= oracle <= 1_000_000:
                band += 1
                band_changed += ch
        if regrets:
            print(f"| {family} | {scenario} | {statistics.mean(regrets):.3f} | {max(regrets):.3f} | {changed} | {points} | {band} | {band_changed} |")

print()
print("# Confidence in 100 us - 1 ms band")
print("| family | scenario | band_points | >=95pct_directional | ambiguous |")
print("|---|---|---:|---:|---:|")
for family in families:
    for scenario in sorted({k[1] for k in summary}):
        vals=[v for (f,s,n),v in summary.items() if f==family and s==scenario and 100_000 <= min(v[0],v[1]) <= 1_000_000]
        if not vals: continue
        hi=sum(1 for v in vals if v[5]>=.95 or v[5]<=.05)
        print(f"| {family} | {scenario} | {len(vals)} | {hi} | {len(vals)-hi} |")
