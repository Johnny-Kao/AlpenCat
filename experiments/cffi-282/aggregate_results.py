#!/usr/bin/env python3
import csv
import json
import pathlib
import statistics
import sys

root=pathlib.Path(sys.argv[1])
rows=[]
for stdout in sorted((root/"scenarios").glob("*/stdout.log")):
    for line in stdout.read_text().splitlines():
        line=line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj=json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("type")=="case":
            obj["log_dir"]=stdout.parent.name
            rows.append(obj)

if not rows:
    raise SystemExit("no benchmark case records found")

by={}
for row in rows:
    key=(row["scenario"],row["case"])
    by.setdefault(key,[]).append(row)

summary=[]
for (scenario,case), vals in sorted(by.items()):
    round_medians=[v["median_ns_per_op"] for v in vals]
    summary.append({
        "scenario":scenario,
        "case":case,
        "matrix_rounds":len(vals),
        "median_ns_per_op":statistics.median(round_medians),
        "min_round_median_ns":min(round_medians),
        "max_round_median_ns":max(round_medians),
        "round_medians_ns":round_medians,
    })

lookup={(x["scenario"],x["case"]):x for x in summary}
for row in summary:
    base=lookup.get(("S1",row["case"]))
    pr=lookup.get(("S2",row["case"]))
    if base:
        row["vs_s1_pct"]=(base["median_ns_per_op"]-row["median_ns_per_op"])/base["median_ns_per_op"]*100.0
    if pr:
        row["vs_s2_pct"]=(pr["median_ns_per_op"]-row["median_ns_per_op"])/pr["median_ns_per_op"]*100.0

(root/"summary.json").write_text(json.dumps({
    "schema_version":1,
    "rows":summary,
},indent=2))

fields=[
    "scenario","case","matrix_rounds","median_ns_per_op",
    "min_round_median_ns","max_round_median_ns","vs_s1_pct","vs_s2_pct"
]
with (root/"summary.csv").open("w",newline="") as f:
    w=csv.DictWriter(f,fieldnames=fields)
    w.writeheader()
    for row in summary:
        w.writerow({k:row.get(k) for k in fields})

print("scenario case median_ns/op vs_S1% vs_S2%")
for row in summary:
    print(
        f'{row["scenario"]:>2} {row["case"]:<14} '
        f'{row["median_ns_per_op"]:9.2f} '
        f'{row.get("vs_s1_pct",float("nan")):8.2f} '
        f'{row.get("vs_s2_pct",float("nan")):8.2f}'
    )
