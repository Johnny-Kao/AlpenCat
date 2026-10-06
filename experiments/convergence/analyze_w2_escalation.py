#!/usr/bin/env python3
import argparse, csv, json, math, pathlib, statistics

def med(v): return float(statistics.median(v))

def load(path):
    return [json.loads(x) for x in path.read_text().splitlines() if x.strip()]

def points_for(root, regime):
    return sorted([r for r in load(root/f"{regime}.jsonl") if r.get("record_type")=="point"], key=lambda r:r["work_items"])

def route_cost(row,boundary):
    s=med(row["serial_samples_ns"]); c=row.get("cpu_samples_ns")
    if row["work_items"]<=boundary or c is None: return s
    return med(c)

def oracle_cost(row):
    s=med(row["serial_samples_ns"]); c=row.get("cpu_samples_ns")
    return s if c is None else min(s,med(c))

def consistency(row):
    c=row.get("cpu_samples_ns")
    if c is None:return 1.0
    s=row["serial_samples_ns"]; pairs=list(zip(s,c))
    ms=med(s); mc=med(c); winner="Serial" if ms<=mc else "Cpu"
    return sum(("Serial" if a<=b else "Cpu")==winner for a,b in pairs)/len(pairs)

def margin(row):
    s=med(row["serial_samples_ns"]); c=med(row["cpu_samples_ns"])
    return 100.0*abs(s-c)/min(s,c)

def find_p3(root,regime):
    p=root/f"{regime}-p3.jsonl"
    rv=next(r for r in load(p) if r.get("record_type")=="revalidation")
    return float(rv["revalidation_elapsed_ns"])

def analyze(evidence,sensitivity,warmup_pairs):
    baseline=points_for(evidence,"baseline-full")
    # baseline scalar boundary: first CPU win, previous size
    prev=0; static=baseline[-1]["work_items"]
    for r in baseline:
        c=r.get("cpu_samples_ns")
        if c is not None and med(c)<med(r["serial_samples_ns"]):
            static=prev; break
        prev=r["work_items"]

    out=[]
    for regime in ("half","contention"):
        pts=points_for(evidence,regime)
        maxp=pts[-1]
        p3=find_p3(sensitivity,regime)
        serial=med(maxp["serial_samples_ns"]); cpu=med(maxp["cpu_samples_ns"])
        sent_winner="Serial" if serial<=cpu else "Cpu"
        sent_cost=sum(maxp["serial_samples_ns"])+sum(maxp["cpu_samples_ns"])
        sent_cost += warmup_pairs*(serial+cpu)
        candidate=pts[-1]["work_items"] if sent_winner=="Serial" else static

        static_cycle=sum(route_cost(r,static) for r in pts)
        candidate_cycle=sum(route_cost(r,candidate) for r in pts)
        oracle_cycle=sum(oracle_cost(r) for r in pts)
        calls=len(pts)
        gain_per_call=max(0.0,(static_cycle-candidate_cycle)/calls)
        total_probe=p3+sent_cost
        be=math.inf if gain_per_call<=0 else total_probe/gain_per_call
        out.append({
            "regime":regime,"static_boundary":static,"sentinel_size":maxp["work_items"],
            "sentinel_winner":sent_winner,"sentinel_consistency":consistency(maxp),
            "sentinel_margin_pct":margin(maxp),"p3_cost_ns":p3,
            "sentinel_estimated_cost_ns":sent_cost,"total_probe_cost_ns":total_probe,
            "candidate_boundary":candidate,"static_cycle_ns":static_cycle,
            "candidate_cycle_ns":candidate_cycle,"oracle_cycle_ns":oracle_cycle,
            "candidate_capture_fraction":0.0 if static_cycle<=oracle_cycle else (static_cycle-candidate_cycle)/(static_cycle-oracle_cycle),
            "break_even_calls":be,
            "net_100_calls_ns":gain_per_call*100-total_probe,
            "net_1000_calls_ns":gain_per_call*1000-total_probe,
            "net_10000_calls_ns":gain_per_call*10000-total_probe,
        })
    return static,out

def main():
    ap=argparse.ArgumentParser(); ap.add_argument("evidence",type=pathlib.Path); ap.add_argument("sensitivity",type=pathlib.Path); ap.add_argument("--warmup-pairs",type=int,default=2)
    a=ap.parse_args(); static,rows=analyze(a.evidence,a.sensitivity,a.warmup_pairs)
    payload={"schema_version":1,"static_boundary":static,"policy":"p3_plus_max_sentinel","rows":rows}
    (a.evidence/"escalation-policy.json").write_text(json.dumps(payload,indent=2,sort_keys=True,allow_nan=False)+"\n")
    fields=list(rows[0].keys())
    with (a.evidence/"escalation-policy.csv").open("w",newline="") as f:
        w=csv.DictWriter(f,fieldnames=fields);w.writeheader()
        for r in rows:
            rr=dict(r); rr["break_even_calls"]=None if math.isinf(rr["break_even_calls"]) else rr["break_even_calls"]; w.writerow(rr)
    lines=["# W2 economic escalation probe","",
      "| Regime | P3 | Sentinel | Margin | Consistency | Candidate | Capture | Break-even | Net @1k |",
      "|---|---:|---|---:|---:|---:|---:|---:|---:|"]
    for r in rows:
        be="n/a" if math.isinf(r["break_even_calls"]) else f"{r['break_even_calls']:.0f}"
        lines.append(f"| {r['regime']} | {r['p3_cost_ns']/1e6:.1f} ms | {r['sentinel_winner']} @ {r['sentinel_size']} | {r['sentinel_margin_pct']:.1f}% | {100*r['sentinel_consistency']:.1f}% | {r['candidate_boundary']} | {100*r['candidate_capture_fraction']:.1f}% | {be} calls | {r['net_1000_calls_ns']/1e6:.1f} ms |")
    report="\n".join(lines)+"\n"; (a.evidence/"escalation-policy.md").write_text(report); print(report)
if __name__=="__main__": main()
