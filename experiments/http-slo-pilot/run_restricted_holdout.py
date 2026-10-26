#!/usr/bin/env python3
"""Predeclared holdout under real CPU quota: independently mixed HTTP request costs.

Do not change the rule or GO thresholds after observing results.
"""
import concurrent.futures
import csv
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import random
import statistics
import sys
import time

sys.path.insert(0,str(Path(__file__).resolve().parent))
import run_http_slo as pilot

MODES = ("fixed8","fixed4","fixed2","direct_guard","alpencat_guard","alpencat_quota")
CASES = {
    "mixed": {"requests":120,"slo_ms":160,"size":"mixed"},
    "weighted": {"requests":192,"slo_ms":220,"size":"weighted"},
}
ROUNDS = 5
BUDGETS = {
    "restricted_p95_improvement_pct":5,
    "min_paired_passes":4,
    "max_cpu_regression_pct":5,
    "max_http_wall_regression_pct":5,
    "direct_same_policy_parity_abs_pct":5,
}

def load_mixed(port,phase,config):
    n=config["requests"]
    kind=config["size"]
    def one(i):
        rid=phase*100_000+i+1
        if kind=="mixed":
            rounds=2_800_000 if i%2==0 else 1_100_000
        else:
            rounds=3_000_000 if i%4==0 else 1_500_000
        started=time.perf_counter_ns()
        conn=http.client.HTTPConnection("127.0.0.1",port,timeout=25)
        try:
            conn.request("GET",f"/work/{rid}/{rounds}",headers={"Connection":"close"})
            rsp=conn.getresponse()
            body=rsp.read().decode("ascii")
            if rsp.status!=200 or len(body)!=16:
                raise AssertionError(f"bad HTTP response: {rsp.status} {body}")
            return rid,body,(time.perf_counter_ns()-started)/1e6
        finally:
            conn.close()
    start=time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as executor:
        observations=list(executor.map(one,range(n)))
    wall=time.perf_counter()-start
    by_id=sorted(observations)
    lat=sorted(o[2] for o in observations)
    return {
        "requests":n, "status_errors":0,
        "http_p95_ms":lat[math.ceil(0.95*n)-1],
        "http_p99_ms":lat[math.ceil(0.99*n)-1],
        "http_mean_ms":statistics.mean(lat),
        "client_wall_ms":wall*1000,
        "requests_per_s":n/wall,
        "checksum":hashlib.sha256("".join(x[1] for x in by_id).encode()).hexdigest(),
    }

def pct(b,a):
    return 100*(b-a)/b

def med(rows,mode,metric):
    return round(statistics.median(x[metric] for x in rows if x["mode"]==mode),3)

def main():
    import argparse
    p=argparse.ArgumentParser()
    p.add_argument("--binary",required=True)
    p.add_argument("--output",default="http-holdout.json")
    args=p.parse_args()
    pilot.SIZES=CASES
    pilot.load_http=load_mixed
    rng=random.Random(20261027) # holdout shuffled order differs from exploratory run
    uid=pilot.cmd("id","-un")
    raw=[]
    serial=0
    for case in CASES:
        for mode in MODES:
            pilot.one_service(args.binary,mode,case,serial,uid)
            serial+=1
        for round_id in range(ROUNDS):
            order=list(MODES)
            rng.shuffle(order)
            grouped={}
            for mode in order:
                phases=pilot.one_service(args.binary,mode,case,serial,uid)
                serial+=1
                grouped[mode]=phases
                for item in phases:
                    raw.append(dict(case=case,round=round_id,**item))
            for phase in range(3):
                if len({grouped[m][phase]["checksum"] for m in MODES})!=1:
                    raise AssertionError("non-identical real HTTP responses across policies")

    results={}
    for case in CASES:
        selected=[x for x in raw if x["case"]==case]
        phase=[x for x in selected if x["phase"]==1]
        round_pairs={}
        for base in ("fixed8","fixed4","fixed2","direct_guard","alpencat_quota"):
            g={}
            for metric in ("http_p95_ms","server_cpu_ms","client_wall_ms"):
                gains=[]
                for rnd in range(ROUNDS):
                    reference=next(x[metric] for x in phase if x["mode"]==base and x["round"]==rnd)
                    actual=next(x[metric] for x in phase if x["mode"]=="alpencat_guard" and x["round"]==rnd)
                    gains.append(pct(reference,actual))
                g[metric]={"median_gain_pct":round(statistics.median(gains),3),
                           "paired_pct":[round(v,2) for v in gains],
                           "rounds_above_5_pct":sum(v>5 for v in gains)}
            round_pairs[base]=g
        best=round_pairs["fixed8"]
        direct=round_pairs["direct_guard"]
        restricted_go=(
            best["http_p95_ms"]["rounds_above_5_pct"]>=BUDGETS["min_paired_passes"]
            and best["server_cpu_ms"]["median_gain_pct"]>=-BUDGETS["max_cpu_regression_pct"]
            and best["client_wall_ms"]["median_gain_pct"]>=-BUDGETS["max_http_wall_regression_pct"]
        )
        parity=(
            abs(direct["http_p95_ms"]["median_gain_pct"])<=BUDGETS["direct_same_policy_parity_abs_pct"]
            and abs(direct["server_cpu_ms"]["median_gain_pct"])<=BUDGETS["direct_same_policy_parity_abs_pct"]
        )
        slos={mode:sum(x["http_p95_ms"]<=CASES[case]["slo_ms"]
                       for x in phase if x["mode"]==mode) for mode in MODES}
        medians={mode:{k:med(phase,mode,k) for k in
                       ("http_p95_ms","server_cpu_ms","client_wall_ms","throttled_periods")}
                 for mode in MODES}
        results[case]={
            "restricted_phase_p95_target_ms":CASES[case]["slo_ms"],
            "restricted_phase_medians":medians,
            "restricted_phase_slo_passes_of_5":slos,
            "comparisons_alpencat_guard_vs":round_pairs,
            "conditional_guard_tradeoff_go":restricted_go,
            "same_rule_os_parity":parity,
        }
        print("HOLDOUT",case,"real_quota_0.5_cpu",flush=True)
        for mode,v in medians.items():
            print(f"MEDIAN case={case} mode={mode} http_p95_ms={v['http_p95_ms']:.2f} "
                  f"server_cpu_ms={v['server_cpu_ms']:.2f} client_wall_ms={v['client_wall_ms']:.2f} "
                  f"throttled_periods={v['throttled_periods']:.0f} "
                  f"p95_SLO_passes={slos[mode]}/5",flush=True)
        for mode,v in round_pairs.items():
            print(f"GAIN case={case} versus={mode} p95_pct={v['http_p95_ms']['median_gain_pct']:.2f} "
                  f"p95_gt5={v['http_p95_ms']['rounds_above_5_pct']}/5 "
                  f"cpu_pct={v['server_cpu_ms']['median_gain_pct']:.2f} "
                  f"wall_pct={v['client_wall_ms']['median_gain_pct']:.2f}",flush=True)
        print(f"VERDICT case={case} conditional_gain={restricted_go} same_rule_parity={parity}",flush=True)
    overall=all(x["conditional_guard_tradeoff_go"] and x["same_rule_os_parity"] for x in results.values())
    print(f"HOLDOUT_0_5CPU_CONDITIONAL_GO={str(overall).upper()}",flush=True)
    print("No production performance claim; same-policy direct OS access is a real competitor.",flush=True)
    report={
        "experiment":"heldout_mixed_http_0.5cpu_condition",
        "preregistered_thresholds":BUDGETS,
        "modes":MODES, "quota_sequence_milli":[2000,500,2000],
        "traffic_case":CASES,"rounds":ROUNDS,
        "github_run_id":os.getenv("GITHUB_RUN_ID"),
        "method":"real tiny_http service, 16 closed-loop TCP clients, external actual Cgroup v2 quota",
        "result":results,"overall_conditional_go":overall,"raw":raw,
        "warning":"Even conditional GO is not unique AlpenCat Core compute speedup; same rule direct OS should be equivalent."
    }
    with open(args.output,"w",encoding="utf-8") as f:json.dump(report,f,indent=2)
    with open(args.output.replace(".json",".csv"),"w",newline="",encoding="utf-8") as f:
        writer=csv.DictWriter(f,fieldnames=list(raw[0]))
        writer.writeheader()
        writer.writerows(raw)

if __name__=="__main__":
    main()
