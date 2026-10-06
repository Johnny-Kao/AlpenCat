#!/usr/bin/env python3
import argparse, csv, json, pathlib, statistics
from collections import Counter, defaultdict

SIZES = [4096, 16384, 65536, 262144, 1048576, 4194304]

def med(v):
    return float(statistics.median(v))

def load_points(path):
    out=[]
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        row=json.loads(line)
        if row.get("record_type")=="point":
            out.append(row)
    return sorted(out,key=lambda r:r["work_items"])

def winner(row):
    cpu=row.get("cpu_samples_ns")
    if cpu is None:
        return "Serial"
    return "Cpu" if med(cpu) < med(row["serial_samples_ns"]) else "Serial"

def route_cost(row, route):
    if route=="Cpu" and row.get("cpu_samples_ns") is not None:
        return med(row["cpu_samples_ns"])
    return med(row["serial_samples_ns"])

def profile(points):
    seq=[winner(r) for r in points]
    oracle=sum(min(route_cost(r,"Serial"), route_cost(r,"Cpu")) for r in points)

    best_scalar=None
    for boundary in [0]+[r["work_items"] for r in points]:
        cost=sum(route_cost(r, "Serial" if r["work_items"]<=boundary else "Cpu") for r in points)
        cand=(cost,boundary)
        if best_scalar is None or cand<best_scalar:
            best_scalar=cand

    best_interval=None
    sizes=[r["work_items"] for r in points]
    for lo in [0]+sizes:
        for hi in sizes:
            if hi<lo:
                continue
            cost=sum(route_cost(r, "Cpu" if lo<r["work_items"]<=hi else "Serial") for r in points)
            cand=(cost,lo,hi)
            if best_interval is None or cand<best_interval:
                best_interval=cand

    scalar_gap=0.0 if oracle<=0 else 100.0*(best_scalar[0]-oracle)/oracle
    interval_gap=0.0 if oracle<=0 else 100.0*(best_interval[0]-oracle)/oracle
    return {
        "sequence": seq,
        "sequence_text": ">".join(seq),
        "switches": sum(a!=b for a,b in zip(seq,seq[1:])),
        "scalar_boundary": best_scalar[1],
        "scalar_gap_pct": scalar_gap,
        "interval_lower": best_interval[1],
        "interval_upper": best_interval[2],
        "interval_gap_pct": interval_gap,
        "interval_exact": interval_gap < 1e-9,
    }

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("root", type=pathlib.Path)
    args=ap.parse_args()

    rows=[]
    by_regime=defaultdict(list)
    for path in sorted(args.root.glob("*.jsonl")):
        stem=path.stem
        if "-b" not in stem:
            continue
        regime, blockpart = stem.rsplit("-b",1)
        block=int(blockpart)
        p=profile(load_points(path))
        row={"regime":regime,"block":block,**p}
        rows.append(row)
        by_regime[regime].append(row)

    if not rows:
        raise SystemExit("no stability block evidence found")

    summary={"schema_version":1,"regimes":{}}
    for regime, items in sorted(by_regime.items()):
        seq_counts=Counter(r["sequence_text"] for r in items)
        per_size={}
        for idx,size in enumerate(SIZES):
            wins=Counter(r["sequence"][idx] for r in items)
            per_size[str(size)]={k:v/len(items) for k,v in wins.items()}
        summary["regimes"][regime]={
            "blocks":len(items),
            "dominant_sequence":seq_counts.most_common(1)[0][0],
            "dominant_sequence_fraction":seq_counts.most_common(1)[0][1]/len(items),
            "non_monotonic_fraction":sum(r["switches"]>1 for r in items)/len(items),
            "median_scalar_gap_pct":statistics.median(r["scalar_gap_pct"] for r in items),
            "median_interval_gap_pct":statistics.median(r["interval_gap_pct"] for r in items),
            "interval_exact_fraction":sum(r["interval_exact"] for r in items)/len(items),
            "per_size_winner_fraction":per_size,
            "scalar_boundaries":dict(Counter(str(r["scalar_boundary"]) for r in items)),
            "intervals":dict(Counter(f"({r['interval_lower']},{r['interval_upper']}]" for r in items)),
        }

    (args.root/"stability-summary.json").write_text(json.dumps(summary,indent=2,sort_keys=True)+"\n")
    with (args.root/"stability-blocks.csv").open("w",newline="") as f:
        fields=["regime","block","sequence_text","switches","scalar_boundary","scalar_gap_pct","interval_lower","interval_upper","interval_gap_pct","interval_exact"]
        w=csv.DictWriter(f,fieldnames=fields); w.writeheader()
        for row in rows:
            w.writerow({k:row[k] for k in fields})

    lines=["# W2 stability blocks","",
           "| Regime | Blocks | Dominant sequence | Dominant | Non-monotonic | Median scalar gap | Median interval gap | Exact interval |",
           "|---|---:|---|---:|---:|---:|---:|---:|"]
    for regime,row in summary["regimes"].items():
        lines.append(f"| {regime} | {row['blocks']} | {row['dominant_sequence']} | {100*row['dominant_sequence_fraction']:.1f}% | {100*row['non_monotonic_fraction']:.1f}% | {row['median_scalar_gap_pct']:.3f}% | {row['median_interval_gap_pct']:.3f}% | {100*row['interval_exact_fraction']:.1f}% |")
    report="\n".join(lines)+"\n"
    (args.root/"stability-summary.md").write_text(report)
    print(report)

if __name__=="__main__":
    main()
