#!/usr/bin/env python3
"""Gate 1: evidence of *recoverable compute regret*, not a CI success contest.

Dedicated systemd service with actual Cgroup v2 quota changes, original
runtime-api Serial/Cpu routes, same-process phase transitions, paired samples.
"""
import argparse
import csv
import json
import os
from pathlib import Path
import random
import socket
import statistics as stats
import subprocess
import tempfile

QUOTAS_MILLI = (2000, 1500, 500, 2000)
REPEATS = 5
FAMILIES = ("compute", "memory")
SIZES = {"compute": (256, 2048, 16384), "memory": (8192, 65536, 524288)}
MIN_ROUTE_MARGIN = 5.0
MIN_GROSS_CPU_SAVING = 1.0
MIN_REPLICATION = 4

def cmd(*args):
    return subprocess.run(args, check=True, capture_output=True,
                          text=True, timeout=30).stdout.strip()

def parse(line, kind):
    if not line.startswith(kind + " "):
        raise RuntimeError(f"protocol expected {kind}: {line!r}")
    return dict(word.split("=", 1) for word in line.split()[1:])

def check_quota(cgroup, requested):
    terms = (cgroup / "cpu.max").read_text().split()
    if len(terms) != 2 or terms[0] == "max":
        raise AssertionError("expected finite actual cgroup-v2 quota")
    milli = round(int(terms[0]) / int(terms[1]) * 1000)
    if abs(milli - requested) > 1:
        raise AssertionError(f"quota control failed: {milli} != {requested}")
    return milli

def stats_for(cgroup):
    return {k: int(v) for k,v in
            (line.split() for line in (cgroup/"cpu.stat").read_text().splitlines()
             if len(line.split()) == 2)}

def run_once(binary, trial):
    unit = f"ag1-{os.getenv('GITHUB_RUN_ID','local')}-{trial}.service"
    username = cmd("id", "-un")
    collected = []
    with tempfile.TemporaryDirectory(prefix="alp-g1-") as dirname:
        socket_path = str(Path(dirname)/"control.sock")
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(socket_path)
        listener.listen(1)
        listener.settimeout(25)
        service_started = False
        try:
            cmd("sudo", "-n", "systemd-run", "--quiet", "--unit", unit[:-8],
                "--uid", username, "--property=CPUQuota=200%",
                "--property=KillMode=control-group",
                str(Path(binary).absolute()), socket_path)
            service_started = True
            conn,_ = listener.accept()
            with conn:
                conn.settimeout(75)
                reader = conn.makefile("r", encoding="utf-8")
                ready = parse(reader.readline().strip(), "READY")
                pid = int(ready["pid"])
                root = cmd("sudo", "systemctl", "show", unit, "-P", "ControlGroup")
                if not root.startswith("/"):
                    raise AssertionError(f"service has no cgroup: {root}")
                assert root in Path(f"/proc/{pid}/cgroup").read_text(), "wrong cgroup membership"
                cg=Path("/sys/fs/cgroup" + root)
                for phase,quota in enumerate(QUOTAS_MILLI):
                    cmd("sudo", "systemctl", "set-property", "--runtime", unit,
                        f"CPUQuota={quota/10:g}%")
                    check_quota(cg,quota)
                    before=stats_for(cg)
                    conn.sendall(f"GO {quota}\n".encode())
                    for _ in range(6):
                        row=parse(reader.readline().strip(),"POINT")
                        if int(row["phase"])!=phase or int(row["quota_milli"])!=quota:
                            raise AssertionError(f"real quota/phase mismatch: {row}")
                        family=row["family"]
                        size=int(row["n"])
                        assert size in SIZES[family]
                        assert row["cpu_backend"] in ("Cpu","Serial")
                        assert int(row["serial_wall_ns"])>0
                        assert int(row["cpu_wall_ns"])>0
                        collected.append({
                            "trial":trial,"phase":phase,"quota_milli":quota,
                            "family":family,"items":size,
                            "available_parallelism":int(row["available_parallelism"]),
                            "cpu_actual_route":row["cpu_backend"],
                            **{k:int(row[k]) for k in
                               ("serial_wall_ns","serial_cpu_ns",
                                "cpu_wall_ns","cpu_cpu_ns")},
                            "checksum":row["checksum"],
                        })
                    done=parse(reader.readline().strip(),"DONE")
                    if int(done["phase"])!=phase:raise AssertionError("missing phase boundary")
                    after=stats_for(cg)
                    n=after.get("nr_throttled",0)-before.get("nr_throttled",0)
                    for row in collected[-6:]:
                        row["phase_throttled_periods"]=n
                        row["phase_cgroup_usage_usec"]=after.get("usage_usec",0)-before.get("usage_usec",0)
                    print(f"PHASE trial={trial} phase={phase} quota_milli={quota} "
                          f"throttled={n} routes=" +
                          ",".join(f"{r['family']}{r['items']}:{r['cpu_actual_route']}"
                                   for r in collected[-6:]),flush=True)
                reader.close()
        except Exception:
            try:
                print("SERVICE_DEBUG",cmd("sudo","journalctl","-u",unit,"-n","20","--no-pager"),
                      flush=True)
            except Exception:pass
            raise
        finally:
            if service_started:
                subprocess.run(["sudo","-n","systemctl","stop",unit],
                               stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=30)
            listener.close()
    return collected

def savings(rows, key):
    # Optimistic oracle: may pick best candidate with zero decision cost.
    # Static: picks a single API route for all four phases for this family/size.
    a=sum(row["serial_"+key+"_ns"] for row in rows)
    b=sum(row["cpu_"+key+"_ns"] for row in rows)
    reference=min(a,b)
    oracle=sum(min(row["serial_"+key+"_ns"],row["cpu_"+key+"_ns"]) for row in rows)
    return 100*(reference-oracle)/reference if reference else 0.

def winner(row, metric):
    a=row["serial_"+metric+"_ns"]
    b=row["cpu_"+metric+"_ns"]
    if row["cpu_actual_route"]!="Cpu":return "no-parallel"
    if a < b * .95:return "serial"
    if b < a * .95:return "cpu"
    return "tie"

def median(xs):return stats.median(xs)

def analyze(raw):
    # Per-trial upper bound uses the actual observed cost; never extrapolate.
    per_family={}; per_point={}
    for family in FAMILIES:
        per_family[family]={}
        for metric in ("cpu","wall"):
            gains=[]
            for trial in range(REPEATS):
                vals=[]
                for size in SIZES[family]:
                    subset=[r for r in raw if r["family"]==family and r["items"]==size
                            and r["trial"]==trial]
                    vals.append(subset)
                # Compare static optimum across regimes at every size (already generous).
                opt_static=sum(min(sum(r["serial_"+metric+"_ns"] for r in sub),
                                   sum(r["cpu_"+metric+"_ns"] for r in sub))
                               for sub in vals)
                oracle=sum(sum(min(r["serial_"+metric+"_ns"],r["cpu_"+metric+"_ns"])
                               for r in sub) for sub in vals)
                gains.append(100*(opt_static-oracle)/opt_static if opt_static else 0)
            per_family[family][metric]={
                "trial_gross_oracle_upper_bound_pct":[round(v,3) for v in gains],
                "median_pct":round(median(gains),3),
                "above_one_pct_rounds":sum(v>MIN_GROSS_CPU_SAVING for v in gains),
                "best_case_pct":round(max(gains),3)
            }
        pointlist=[]
        for size in SIZES[family]:
            for quota_index in range(len(QUOTAS_MILLI)):
                rs=[r for r in raw if r["family"]==family and r["items"]==size
                    and r["phase"]==quota_index]
                pointlist.append({
                    "items":size,
                    "phase":quota_index,
                    "quota_milli":QUOTAS_MILLI[quota_index],
                    "cpu_actual_route_count":sum(r["cpu_actual_route"]=="Cpu" for r in rs),
                    "median_serial_wall_ns":median([r["serial_wall_ns"] for r in rs]),
                    "median_cpu_wall_ns":median([r["cpu_wall_ns"] for r in rs]),
                    "median_serial_cpu_ns":median([r["serial_cpu_ns"] for r in rs]),
                    "median_cpu_cpu_ns":median([r["cpu_cpu_ns"] for r in rs]),
                    "cpu_winners_5pct":{label:sum(winner(r,"cpu")==label for r in rs)
                                        for label in ("serial","cpu","tie","no-parallel")},
                    "wall_winners_5pct":{label:sum(winner(r,"wall")==label for r in rs)
                                        for label in ("serial","cpu","tie","no-parallel")},
                })
        per_point[family]=pointlist
    crossings={}
    for family in FAMILIES:
        crossings[family]=[]
        for size in SIZES[family]:
            points=[p for p in per_point[family] if p["items"]==size]
            for metric in ("cpu","wall"):
                # Opposite winners, each >=4/5, both genuine CPU routes available.
                picks=[("serial" if p[f"{metric}_winners_5pct"]["serial"]>=MIN_REPLICATION
                        else "cpu" if p[f"{metric}_winners_5pct"]["cpu"]>=MIN_REPLICATION
                        else "unknown") for p in points]
                crossed=("serial" in picks and "cpu" in picks)
                if crossed:
                    crossings[family].append({"size":size,"metric":metric,
                                              "winning_routes_by_phase":picks})
    cpu_gain_pass=all(per_family[f]["cpu"]["above_one_pct_rounds"]>=MIN_REPLICATION
                      for f in FAMILIES)
    cpu_cross_pass=all(any(c["metric"]=="cpu" for c in crossings[f]) for f in FAMILIES)
    gate1_go=cpu_gain_pass and cpu_cross_pass
    return {"family":per_family,"points":per_point,"crossings":crossings,
            "predeclared_cpu_gross_gain_pass":cpu_gain_pass,
            "predeclared_cpu_route_crossover_pass":cpu_cross_pass,
            "gate1_go":gate1_go,
            "interpretation":(
                "GO means only evidence for real route crossover and gross >1% "
                "reclaimable CPU cost, not net benefit, not a Beta release."
                if gate1_go else
                "NO-GO or inconclusive for CPU-economics convergence on these preselected "
                "two workload families. Do not launch generic adaptation development."
            )}

def main():
    p=argparse.ArgumentParser()
    p.add_argument("--binary",required=True)
    p.add_argument("--output",default="gate1-results.json")
    a=p.parse_args()
    raw=[]
    for trial in range(REPEATS):
        raw.extend(run_once(a.binary,trial))
    for family in FAMILIES:
        for size in SIZES[family]:
            rows=[r for r in raw if r["family"]==family and r["items"]==size]
            assert len(rows)==len(QUOTAS_MILLI)*REPEATS
            # An equal workload produces the same checksum in every phase/round.
            assert len({r["checksum"] for r in rows})==1,"incorrect route outputs"
    assert any(r["phase_throttled_periods"]>0 for r in raw if r["phase"]==2), \
        "half-CPU quota did not produce genuine throttling; integrity failure"
    report=analyze(raw)
    print("PREDECLARED_G1_MIN_GROSS_CPU_GAIN_PCT=1.0",flush=True)
    for family in FAMILIES:
        print(f"FAMILY {family} gross_cpu_oracle_savings_pct="
              f"{report['family'][family]['cpu']['median_pct']} "
              f"gross_wall_oracle_savings_pct={report['family'][family]['wall']['median_pct']} "
              f"cpu_crossovers={len([x for x in report['crossings'][family] if x['metric']=='cpu'])} "
              f"wall_crossovers={len([x for x in report['crossings'][family] if x['metric']=='wall'])}",
              flush=True)
        for row in report["points"][family]:
            print(f"POINT_SUMMARY family={family} n={row['items']} phase={row['phase']} "
                  f"quota_milli={row['quota_milli']} cpu_backends={row['cpu_actual_route_count']}/5 "
                  f"CPU_5pct_winners={row['cpu_winners_5pct']} "
                  f"WALL_5pct_winners={row['wall_winners_5pct']}",flush=True)
    print("GATE1_CPU_GROSS_ECONOMICS_GO="+str(report["gate1_go"]).upper(),flush=True)
    print(report["interpretation"],flush=True)
    report.update({
        "protocol":"gate1-original-runtime-serial-cpu",
        "quota_milli":QUOTAS_MILLI,
        "families":SIZES,
        "repeats":REPEATS,
        "route_margin_pct":MIN_ROUTE_MARGIN,
        "min_one_pct_rounds":MIN_REPLICATION,
        "same_process":True,
        "runner":os.uname().release,
        "github_run_id":os.getenv("GITHUB_RUN_ID"),
        "upper_bound_disclaimer":"Perfect-knowledge per-phase route oracle with ZERO policy/control cost, generous per-size static baseline. Not deployable.",
        "raw":raw,
    })
    with open(a.output,"w",encoding="utf-8") as out:json.dump(report,out,indent=2)
    with open(a.output.replace(".json",".csv"),"w",newline="",encoding="utf-8") as out:
        writer=csv.DictWriter(out,fieldnames=list(raw[0]))
        writer.writeheader()
        writer.writerows(raw)

if __name__=="__main__":main()
