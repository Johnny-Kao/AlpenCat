#!/usr/bin/env python3
"""Paired X3: contemporaneous PSI and frozen oracle opportunity labels per workload and runner."""
import csv,json,pathlib,statistics,sys
root=pathlib.Path(sys.argv[1]);rows=[]
for machine in root.rglob("hardware-manifest.json"):
    for w in ("w1","w2","w3"):
        d=machine.parent/w;econ=d/"policy-summary.csv"
        if not econ.exists():continue
        for row in csv.DictReader(econ.open()):
            psi=d/(row["regime"]+".psi.json")
            if not psi.exists():continue
            p=json.loads(psi.read_text());signal={}
            for source in ("/proc/pressure/cpu","/proc/pressure/memory","/proc/pressure/io"):
                before=p["before"][source];after=p["after"][source]
                if "some" in before and "some" in after:
                    signal[source]=(after["some"]-before["some"])*1000/p["elapsed_ns"]
            rows.append({"machine":machine.parent.name,"workload":w,"regime":row["regime"],"opportunity":float(row["recoverable_ns_vs_static"])>0,"savings_pct":float(row["alpencat_savings_pct_vs_static"]),"signal":signal})
if not rows:raise SystemExit("NO PAIRED EVIDENCE")
op=[x for x in rows if x["opportunity"]];nop=[x for x in rows if not x["opportunity"]]
summary={"paired_rows":len(rows),"runner_count":len({x["machine"] for x in rows}),"opportunities":len(op),"no_opportunities":len(nop),"signals":{},"decision":"Descriptive paired evidence only; no thresholds fitted or policy change."}
for s in ("/proc/pressure/cpu","/proc/pressure/memory","/proc/pressure/io"):
    a=[x["signal"][s] for x in op if s in x["signal"]];b=[x["signal"][s] for x in nop if s in x["signal"]]
    summary["signals"][s]={"opportunity_median":statistics.median(a) if a else None,"no_opportunity_median":statistics.median(b) if b else None,"opportunity_count":len(a),"no_opportunity_count":len(b)}
pathlib.Path("x3-paired-summary.json").write_text(json.dumps(summary,indent=2));print(json.dumps(summary,indent=2))
