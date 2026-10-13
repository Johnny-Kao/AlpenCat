#!/usr/bin/env python3
"""Boundary validity audit using independent static-boundary and per-point route evidence.

Diagnostic/offline oracle: not a deployable native event detector.
"""
import argparse,csv,json,pathlib,statistics
from collections import defaultdict
from importlib.util import module_from_spec,spec_from_file_location
ROOT=pathlib.Path(__file__).resolve().parents[2]
spec=spec_from_file_location("econ",ROOT/"experiments/convergence/analyze_economics.py")
econ=module_from_spec(spec);spec.loader.exec_module(econ)

def audit(machine):
    cases=[];metrics=defaultdict(lambda: {"invalid":0,"valid":0,"regret_ns":0.0,"measurements":0})
    for work in ("w1","w2","w3"):
        d=machine/work
        if not (d/"baseline-full.jsonl").exists():continue
        groups=defaultdict(list)
        for path in sorted(d.glob("*.jsonl")):
            if path.name=="evidence.jsonl":continue
            for line in path.read_text().splitlines():
                x=json.loads(line)
                if x.get("record_type")=="point":groups[x["regime"]].append(x)
        if "baseline-full" not in groups:continue
        baseline=econ.derive_boundary(groups["baseline-full"])
        for regime,points in groups.items():
            mismatches=0;regret=0.;n=0;ties=0
            for row in points:
                if row.get("cpu_samples_ns") is None:continue
                n+=1
                selected,route=econ.selected_cost(row,baseline)
                optimal,oroute=econ.oracle_cost(row)
                diff=max(0.,selected-optimal)
                if diff>0:mismatches+=1;regret+=diff*float(row.get("weight",1))
                if selected==optimal:ties+=1
            if not n:continue
            cases.append({"machine":machine.name,"workload":work,"regime":regime,
             "static_boundary":baseline,"measured_points":n,"invalid_points":mismatches,
             "positive_regret_ns_per_grid":regret,"boundary_invalid":mismatches>0,
             "label_basis":"pointwise measured serial-vs-CPU route regret; diagnostic oracle",
             "transition_proxy":regime!="baseline-full"})
    return cases

def main():
    p=argparse.ArgumentParser();p.add_argument("root",type=pathlib.Path);p.add_argument("--out",type=pathlib.Path,default=pathlib.Path("boundary-validity-summary.json"));a=p.parse_args()
    machine_dirs=[x.parent for x in a.root.rglob("hardware-manifest.json")]
    rows=[row for machine in machine_dirs for row in audit(machine)]
    if not rows:raise SystemExit("INVALID: no point-level boundary evidence")
    tp=sum(x["boundary_invalid"] and x["transition_proxy"] for x in rows)
    fp=sum(not x["boundary_invalid"] and x["transition_proxy"] for x in rows)
    fn=sum(x["boundary_invalid"] and not x["transition_proxy"] for x in rows)
    tn=sum(not x["boundary_invalid"] and not x["transition_proxy"] for x in rows)
    # The proxy is known regime metadata, not a real-time transition signal.
    result={"schema":1,"machines":len(machine_dirs),"cases":len(rows),"invalid_cases":tp+fn,
      "valid_cases":fp+tn,"transition_proxy_confusion":{"tp":tp,"fp":fp,"fn":fn,"tn":tn},
      "regret_ns_unweighted_grid_sum":sum(x["positive_regret_ns_per_grid"] for x in rows),
      "regret_ns_median_per_case":statistics.median(x["positive_regret_ns_per_grid"] for x in rows),
      "notes":["Offline oracle; not a production validity detector",
       "Baseline may have route regret: snapshot boundary is not an exact oracle",
       "A case is invalid if any measured point has positive route regret; a more conservative materiality policy requires independent tolerances",
       "No pre-transition native event timing, no runtime adapter and no valid net economic GO from these data"] }
    a.out.write_text(json.dumps(result,indent=2))
    with a.out.with_suffix(".csv").open("w",newline="") as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]));writer.writeheader();writer.writerows(rows)
    print(json.dumps(result,indent=2))
if __name__=="__main__":main()
