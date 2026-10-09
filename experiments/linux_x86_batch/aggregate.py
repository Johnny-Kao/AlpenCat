#!/usr/bin/env python3
"""Aggregate independent probe artifacts; no fitted thresholds or claims of performance gain."""
import json,sys
from collections import Counter
from pathlib import Path
root=Path(sys.argv[1]);outputs=list(root.rglob("result.json"))
rows=[json.loads(p.read_text()) for p in outputs]
summary={"total":len(rows),"eligible":sum(r["metadata"]["eligible"] for r in rows),"runner_classes":dict(Counter(r["metadata"]["id"].split("-s")[0] for r in rows)),"hosts":[{"id":r["metadata"]["id"],"model":r["inventory"]["cpuinfo_model"],"arch":r["metadata"]["arch"],"eligible":r["metadata"]["eligible"],"phases":[p["phase"] for p in r["phases"]]} for r in rows],"decision":"X1/X2 inventory only. X3 opportunity discrimination requires labeled independent oracle evidence."}
Path("batch-summary.json").write_text(json.dumps(summary,indent=2))
print(json.dumps({k:v for k,v in summary.items() if k!="hosts"},indent=2))
if not outputs:raise SystemExit("No probe results collected")
