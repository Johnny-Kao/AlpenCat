#!/usr/bin/env python3
"""Opt-in Guard pilot with real HTTP requests in a real disposable Cgroup v2 service.

Functional GO and product-performance GO are distinct; preserve raw evidence.
"""
import argparse
import concurrent.futures
import csv
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import random
import socket
import statistics
import subprocess
import time

MODES = ("fixed8", "observe", "recommend", "direct", "enforce")
QUOTAS = (2000, 500, 2000)
ROUNDS = 3
CONCURRENT = 12
REQUESTS_PER_PHASE = 84
CPU_ITERATIONS = 1_500_000

def cmd(*args):
    return subprocess.run(args, check=True, capture_output=True,
                          text=True, timeout=25).stdout.strip()

def cstat(path):
    return {a:int(b) for a,b in
            (line.split() for line in (path/"cpu.stat").read_text().splitlines()
             if len(line.split()) == 2)}

def readquota(path):
    a,b=(path/"cpu.max").read_text().split()
    if a == "max": raise AssertionError("quota is unlimited")
    return round(int(a)*1000/int(b))

def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1",0))
        return s.getsockname()[1]

def http_get(host_port, path, timeout=25):
    con=http.client.HTTPConnection("127.0.0.1",host_port,timeout=timeout)
    try:
        con.request("GET",path,headers={"Connection":"close"})
        res=con.getresponse()
        text=res.read().decode()
        return res.status,text
    finally:
        con.close()

def wait_healthy(host_port):
    for _ in range(100):
        try:
            status,body=http_get(host_port,"/health",timeout=1)
            if status==200 and "healthy=1" in body:
                return body
        except (OSError,TimeoutError,http.client.HTTPException):
            pass
        time.sleep(.05)
    raise RuntimeError("HTTP server not healthy")

def send_batch(host_port, phase):
    def one(i):
        started=time.perf_counter_ns()
        status,body=http_get(host_port,f"/work/{phase*100_000+i+1}/{CPU_ITERATIONS}")
        if status != 200 or len(body)!=16:
            raise AssertionError(f"invalid HTTP status / body: {status} {body}")
        return i,body,(time.perf_counter_ns()-started)/1e6
    started=time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=CONCURRENT) as executor:
        result=list(executor.map(one,range(REQUESTS_PER_PHASE)))
    elapsed=time.perf_counter()-started
    values=sorted(z[2] for z in result)
    return {
        "p95_ms":values[math.ceil(.95*len(values))-1],
        "p99_ms":values[math.ceil(.99*len(values))-1],
        "client_wall_ms":elapsed*1000,
        "throughput_rps":len(values)/elapsed,
        "fingerprint":hashlib.sha256("".join(z[1] for z in sorted(result)).encode()).hexdigest(),
    }

def run_service(binary,mode,trial,user):
    name=f"ag-{os.environ.get('GITHUB_RUN_ID','local')}-{trial}-{mode}"
    unit=name+".service"
    p=port()
    started=False
    rows=[]
    try:
        cmd("sudo","-n","systemd-run","--quiet","--unit",name,
            "--uid",user,"--property=CPUQuota=200%",
            "--property=KillMode=control-group",str(Path(binary).absolute()),mode,str(p))
        started=True
        wait_healthy(p)
        root=cmd("sudo","systemctl","show",unit,"-P","ControlGroup")
        if not root.startswith("/"):
            raise AssertionError(f"missing cgroup {root}")
        path=Path("/sys/fs/cgroup"+root)
        pid=cmd("sudo","systemctl","show",unit,"-P","MainPID")
        membership=Path(f"/proc/{int(pid)}/cgroup").read_text()
        if root not in membership:
            raise AssertionError(f"worker is not in dedicated Cgroup: {membership}")
        for phase,milli in enumerate(QUOTAS):
            cmd("sudo","systemctl","set-property","--runtime",unit,f"CPUQuota={milli/10:g}%")
            if abs(readquota(path)-milli)>1:
                raise AssertionError(f"real Cgroup quota did not change at phase {phase}")
            e_start=time.perf_counter_ns()
            status,body=http_get(p,"/event")
            event_ms=(time.perf_counter_ns()-e_start)/1e6
            if status!=200 or "refresh_ok=true" not in body:
                raise AssertionError(f"resource event failed: {status} {body}")
            expected = {
                "fixed8":8, "observe":8, "recommend":8,
                "direct":2 if milli<1000 else 4,
                "enforce":2 if milli<1000 else 4,
            }[mode]
            if f"active_limit={expected}" not in body:
                raise AssertionError(f"{mode} did not choose expected profile in quota {milli}: {body}")
            if mode == "observe" and "recommendation=None" not in body:
                raise AssertionError(f"observe must not propose a worker limit: {body}")
            if mode == "recommend" and f"recommendation=Some({2 if milli<1000 else 4})" not in body:
                raise AssertionError(f"recommend must propose (without enforcing) the profile: {body}")
            if mode in ("direct","enforce") and f"known_cpu=Some({milli/1000})" not in body:
                raise AssertionError(f"resource quota was not correctly observed: {body}")
            before=cstat(path)
            metrics=send_batch(p,phase)
            after=cstat(path)
            throttles=after.get("nr_throttled",0)-before.get("nr_throttled",0)
            rows.append(dict(
                mode=mode, trial=trial, phase=phase, quota_milli=milli,
                chosen_limit=expected, event_ms=event_ms,
                cgroup_cpu_ms=(after.get("usage_usec",0)-before.get("usage_usec",0))/1000,
                throttled_periods=throttles, **metrics
            ))
        if rows[1]["throttled_periods"]<=0:
            raise AssertionError(f"no actual throttling in 0.5 CPU phase: {rows}")
    except Exception:
        try:
            print("SERVICE_LOG",unit,cmd("sudo","journalctl","-u",unit,"-n","20","--no-pager"),
                  flush=True)
        except Exception:
            pass
        raise
    finally:
        if started:
            subprocess.run(["sudo","-n","systemctl","stop",unit],
                           stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,
                           timeout=25)
    return rows

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--binary",required=True)
    parser.add_argument("--output",default="guard-http.json")
    args=parser.parse_args()
    uid=cmd("id","-un")
    rng=random.Random(20261028)
    rows=[]
    for trial in range(ROUNDS):
        modes=list(MODES)
        rng.shuffle(modes)
        grouped={}
        for mode in modes:
            item=run_service(args.binary,mode,trial,uid)
            grouped[mode]=item
            rows.extend(item)
            print(f"TRIAL={trial} MODE={mode} quota=2->0.5->2 "
                  f"limits={[p['chosen_limit'] for p in item]} "
                  f"throttles={[p['throttled_periods'] for p in item]}",flush=True)
        for phase in range(3):
            checks={grouped[m][phase]["fingerprint"] for m in MODES}
            if len(checks)!=1:
                raise AssertionError(f"all policies must return the same HTTP data: {checks}")
    report={"protocol":"guard-opt-in-real-cgroup-http-v0.1",
            "run":os.getenv("GITHUB_RUN_ID","unknown"),
            "quota_cpu_milli":QUOTAS,
            "modes":MODES,
            "requests":REQUESTS_PER_PHASE,
            "client_concurrency":CONCURRENT,
            "rounds":ROUNDS,
            "results":rows}
    stats={}
    for phase,milli in enumerate(QUOTAS):
        stats[milli if phase!=2 else "restore"]= {}
        for mode in MODES:
            mine=[x for x in rows if x["phase"]==phase and x["mode"]==mode]
            stats[milli if phase!=2 else "restore"][mode]={
                k:round(statistics.median(x[k] for x in mine),2)
                for k in ("p95_ms","client_wall_ms","cgroup_cpu_ms","throttled_periods")
            }
            x=stats[milli if phase!=2 else "restore"][mode]
            print(f"SUMMARY phase={phase} quota_milli={milli} mode={mode} "
                  f"p95_ms={x['p95_ms']} wall_ms={x['client_wall_ms']} "
                  f"cgroup_cpu_ms={x['cgroup_cpu_ms']} throttles={x['throttled_periods']}",flush=True)
    report["summary"]=stats
    print("GUARD_FUNCTIONAL_PILOT=PASS",flush=True)
    print("NET_PERFORMANCE_GO=NOT_YET_ESTABLISHED",flush=True)
    with open(args.output,"w",encoding="utf-8") as handle:json.dump(report,handle,indent=2)
    with open(args.output.replace(".json",".csv"),"w",newline="") as handle:
        writer=csv.DictWriter(handle,fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)

if __name__=="__main__":main()
