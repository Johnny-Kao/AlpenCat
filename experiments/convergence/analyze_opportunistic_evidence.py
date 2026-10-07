#!/usr/bin/env python3
import argparse
import json
import pathlib


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def median(values):
    xs=sorted(float(x) for x in values)
    return xs[len(xs)//2]


def paired_regret(point):
    s=point.get("serial_samples_ns")
    c=point.get("cpu_samples_ns")
    if not isinstance(s,list) or not isinstance(c,list) or not s or not c:
        return 0.0
    return max(0.0, median(c)-median(s))


def simulate_opportunistic(point, horizon, sample_rate, alternate_cost_fraction=1.0):
    weight=float(point.get("weight",1.0))
    target_fraction=weight
    expected_target_calls=horizon*target_fraction
    samples=expected_target_calls*sample_rate
    regret=paired_regret(point)
    sample_cost=samples*regret*alternate_cost_fraction
    # Evidence benefit is not credited until at least one expected sample exists.
    acquired=samples>=1.0
    return {
        "expected_target_calls": expected_target_calls,
        "expected_samples": samples,
        "sampling_cost_ns": sample_cost,
        "evidence_acquired": acquired,
    }


def main():
    p=argparse.ArgumentParser()
    p.add_argument("evidence_root", type=pathlib.Path)
    p.add_argument("--horizons", type=int, nargs="+", default=[100,1000,10000])
    p.add_argument("--sample-rates", type=float, nargs="+", default=[0.01,0.05,0.10])
    args=p.parse_args()

    rows=load_jsonl(args.evidence_root/"evidence.jsonl")
    by_regime={}
    for row in rows:
        if row.get("record_type")=="point":
            by_regime.setdefault(row["regime"],[]).append(row)

    out={"schema_version":1,"label":"opportunistic-evidence-simulator-v1","cases":[]}
    for regime,pts in sorted(by_regime.items()):
        for pt in sorted(pts,key=lambda x:int(x["work_items"])):
            for h in args.horizons:
                for rate in args.sample_rates:
                    sim=simulate_opportunistic(pt,h,rate)
                    sim.update({"regime":regime,"work_items":int(pt["work_items"]),"horizon":h,"sample_rate":rate})
                    out["cases"].append(sim)
    target=args.evidence_root/"opportunistic-evidence.json"
    target.write_text(json.dumps(out,indent=2,sort_keys=True)+"\n")
    print(target)

if __name__=="__main__":
    main()
