import json, os, statistics
from pathlib import Path

root=Path(os.environ.get("RESULT_ROOT","paired-artifacts"))
keys=["s1-legacy","s2-pr282","s3-swiss-replace","s4-shadow","s5-backup","s6-bypass"]
groups={key:[] for key in keys}

for p in root.glob("*/summary.json"):
    data=json.loads(p.read_text())
    scenario=data["scenario"]
    for key in keys:
        if scenario.startswith(key):
            groups[key].append(data)

for key,val in groups.items():
    if len(val) != 2:
        raise SystemExit(f"expected two passes for {key}, got {len(val)}")

cases=groups["s1-legacy"][0]["cases"].keys()
rows=[]
for case in cases:
    row={"case":case}
    medians={}
    for key in keys:
        vals=[x["cases"][case]["median_ns"] for x in groups[key]]
        medians[key]=statistics.median(vals)
        row[f"{key}_pass_medians_ns"]=vals
        row[f"{key}_paired_median_ns"]=medians[key]
    legacy=medians["s1-legacy"]
    pr282=medians["s2-pr282"]
    for key in keys[1:]:
        row[f"{key}_vs_legacy_pct"]=(legacy-medians[key])/legacy*100.0
        row[f"{key}_vs_pr282_pct"]=(pr282-medians[key])/pr282*100.0
    rows.append(row)

stats={}
for key in keys[2:]:
    stats[key]=[x.get("swiss_stats") for x in groups[key]]

Path("comparison.json").write_text(json.dumps({"rows":rows,"swiss_stats":stats},indent=2))
lines=[
 "# CFFI same-runner Swiss Knife benchmark","",
 "| case | legacy | PR #282 | Swiss replace | Shadow | Backup | Bypass |",
 "|---|---:|---:|---:|---:|---:|---:|"
]
for r in rows:
    lines.append(
      f'| {r["case"]} | {r["s1-legacy_paired_median_ns"]:.2f} | '
      f'{r["s2-pr282_paired_median_ns"]:.2f} | '
      f'{r["s3-swiss-replace_paired_median_ns"]:.2f} | '
      f'{r["s4-shadow_paired_median_ns"]:.2f} | '
      f'{r["s5-backup_paired_median_ns"]:.2f} | '
      f'{r["s6-bypass_paired_median_ns"]:.2f} |')
lines += ["","## Relative to matching baseline","",
 "| case | Swiss route vs #282 | Shadow vs #282 | Backup vs #282 | Base+Swiss bypass vs Legacy |",
 "|---|---:|---:|---:|---:|"
]
for r in rows:
    lines.append(
      f'| {r["case"]} | {r["s3-swiss-replace_vs_pr282_pct"]:.2f}% | '
      f'{r["s4-shadow_vs_pr282_pct"]:.2f}% | {r["s5-backup_vs_pr282_pct"]:.2f}% | '
      f'{r["s6-bypass_vs_legacy_pct"]:.2f}% |')
Path("comparison.md").write_text("\n".join(lines)+"\n")
print("\n".join(lines))
