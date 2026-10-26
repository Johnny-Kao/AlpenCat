#!/usr/bin/env python3
"""Actual HTTP p95/CPU/SLO evaluation under writable dedicated cgroup v2.
Client-generated requests use TCP and tiny_http; no synthetic latency samples."""
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
import tempfile
import time

MODES = ("fixed1", "fixed2", "fixed4", "fixed8",
         "direct_quota", "alpencat_quota", "direct_guard", "alpencat_guard")
QUOTAS = (2000, 500, 2000)
SIZES = {
    "short": {"requests": 96, "iterations": 1_800_000, "slo_ms": 250},
    "long": {"requests": 144, "iterations": 2_000_000, "slo_ms": 700},
}
ROUNDS = 4
PARALLEL_CLIENTS = 16
SLO_CPU_BUDGET = 1.02
WALL_REGRESSION_BUDGET = 1.05


def cmd(*args):
    return subprocess.run(args, check=True, text=True, capture_output=True, timeout=20).stdout.strip()


def read_cgroup_stat(path):
    fields = {}
    for line in (path / "cpu.stat").read_text().splitlines():
        part = line.split()
        if len(part) == 2:
            fields[part[0]] = int(part[1])
    return fields


def assert_quota(path, milli):
    actual = (path / "cpu.max").read_text().split()
    if len(actual) != 2 or actual[0] == "max":
        raise AssertionError(f"real quota missing: {actual}")
    observed = round(int(actual[0]) / int(actual[1]) * 1000)
    if abs(observed - milli) > 1:
        raise AssertionError(f"quota change failed: {observed} != {milli}")
    return observed


def free_port():
    with socket.socket(socket.AF_INET) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def parse_line(s, prefix):
    if not s.startswith(prefix):
        raise RuntimeError(f"expected '{prefix}' got '{s}'")
    return dict(token.split("=", 1) for token in s.split()[1:])


def percentile95(latencies):
    ordered = sorted(latencies)
    return ordered[math.ceil(0.95 * len(ordered)) - 1]


def load_http(port, phase, case):
    n = case["requests"]
    rounds = case["iterations"]

    def one(index):
        rid = phase * 100_000 + index + 1
        started = time.perf_counter_ns()
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=25)
        try:
            conn.request("GET", f"/work/{rid}/{rounds}", headers={"Connection": "close"})
            response = conn.getresponse()
            body = response.read().decode("ascii")
            if response.status != 200 or len(body) != 16:
                raise AssertionError(f"HTTP status/output invalid: {response.status}, {body}")
            return (rid, body, (time.perf_counter_ns() - started) / 1e6)
        finally:
            conn.close()

    start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=PARALLEL_CLIENTS) as executor:
        results = list(executor.map(one, range(n)))
    wall_s = time.perf_counter() - start
    data = sorted(results)
    fingerprints = hashlib.sha256("".join(x[1] for x in data).encode()).hexdigest()
    return {
        "requests": n,
        "status_errors": 0,
        "http_p95_ms": percentile95([x[2] for x in results]),
        "http_p99_ms": sorted(x[2] for x in results)[math.ceil(0.99 * n) - 1],
        "http_mean_ms": statistics.mean(x[2] for x in results),
        "client_wall_ms": wall_s * 1000,
        "requests_per_s": n / wall_s,
        "checksum": fingerprints,
    }


def one_service(binary, mode, size, serial, runner_user):
    unit = f"ah-{os.getenv('GITHUB_RUN_ID', 'local')}-{serial}.service"
    config = SIZES[size]
    rows = []
    with tempfile.TemporaryDirectory(prefix="http-slo-") as tmp:
        sockpath = str(Path(tmp) / "control.sock")
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(sockpath)
        listener.listen(1)
        listener.settimeout(20)
        port = free_port()
        started_unit = False
        try:
            cmd("sudo", "-n", "systemd-run", "--quiet", "--unit", unit[:-8],
                "--uid", runner_user, "--property=CPUQuota=200%",
                "--property=KillMode=control-group",
                str(Path(binary).absolute()), mode, str(port), sockpath)
            started_unit = True
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(40)
                control = conn.makefile("r", encoding="utf-8")
                hello = parse_line(control.readline().strip(), "READY ")
                pid = int(hello["pid"])
                if int(hello["port"]) != port:
                    raise AssertionError("server port mismatch")
                cg = cmd("sudo", "systemctl", "show", unit, "-P", "ControlGroup")
                if not cg.startswith("/"):
                    raise AssertionError(f"no service cgroup: {cg}")
                proc_cgroup = Path(f"/proc/{pid}/cgroup").read_text()
                if cg not in proc_cgroup:
                    raise AssertionError(f"worker not in owned cgroup: {proc_cgroup} {cg}")
                path = Path("/sys/fs/cgroup" + cg)
                for phase, milli in enumerate(QUOTAS):
                    cmd("sudo", "systemctl", "set-property", "--runtime", unit,
                        f"CPUQuota={milli / 10:g}%")
                    assert_quota(path, milli)
                    before = read_cgroup_stat(path)
                    conn.sendall(f"GO {milli}\n".encode())
                    selection = parse_line(control.readline().strip(), "PHASE ")
                    if int(selection["phase"]) != phase or int(selection["quota_milli"]) != milli:
                        raise AssertionError(f"observed quota mismatch: {selection}")
                    actual_workers = int(selection["workers"])
                    expected = {"fixed1":1, "fixed2":2, "fixed4":4, "fixed8":8,
                                "direct_quota": 1 if milli < 1000 else 2,
                                "alpencat_quota": 1 if milli < 1000 else 2,
                                "direct_guard": 2 if milli < 1000 else 4,
                                "alpencat_guard": 2 if milli < 1000 else 4}[mode]
                    if actual_workers != expected:
                        raise AssertionError(f"selected wrong worker count: {actual_workers} vs {expected}")
                    result = load_http(port, phase, config)
                    conn.sendall(b"DONE\n")
                    end = parse_line(control.readline().strip(), "END ")
                    if int(end["phase"]) != phase:
                        raise AssertionError(f"bad end protocol: {end}")
                    after = read_cgroup_stat(path)
                    throttled = after.get("nr_throttled", 0) - before.get("nr_throttled", 0)
                    if throttled < 0:
                        raise AssertionError("invalid cgroup counter delta")
                    rows.append(dict(
                        mode=mode, size=size, phase=phase, quota_milli=milli,
                        workers=actual_workers, server_cpu_ms=int(end["cpu_ns"])/1e6,
                        server_wall_ms=int(end["wall_ns"])/1e6,
                        decision_ms=int(selection["decision_ns"])/1e6,
                        throttled_periods=throttled,
                        throttled_ms=(after.get("throttled_usec", 0) -
                                      before.get("throttled_usec", 0))/1000,
                        cgroup_cpu_ms=(after.get("usage_usec", 0) -
                                       before.get("usage_usec", 0))/1000,
                        **result,
                    ))
                if rows[1]["throttled_periods"] <= 0:
                    raise AssertionError("restricted real quota did not throttle: invalid experiment")
                control.close()
        except Exception:
            try:
                logs = cmd("sudo", "journalctl", "-u", unit, "-n", "15", "--no-pager")
                print("SERVICE_DEBUG",unit,logs,flush=True)
            except Exception:
                pass
            raise
        finally:
            if started_unit:
                subprocess.run(["sudo","-n","systemctl","stop",unit],
                               stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,
                               timeout=20)
            listener.close()
    return rows


def median(xs):
    return statistics.median(xs)


def main():
    p=argparse.ArgumentParser()
    p.add_argument("--binary",required=True)
    p.add_argument("--output",default="http-slo-results.json")
    a=p.parse_args()
    uid=cmd("id","-un")
    rng=random.Random(20261026)
    rows=[]
    serial=0
    # Two independently measured workload regimes, one fixed objective per regime.
    for size in SIZES:
        for mode in MODES:  # one charged-free warm-up per mode
            one_service(a.binary,mode,size,serial,uid)
            serial+=1
        for round_id in range(ROUNDS):
            shuffled=list(MODES)
            rng.shuffle(shuffled)
            group={}
            for mode in shuffled:
                phases=one_service(a.binary,mode,size,serial,uid)
                serial+=1
                group[mode]=phases
                for phase in phases:
                    rows.append(dict(round=round_id,**phase))
            for phase in range(3):
                fingerprints={group[mode][phase]["checksum"] for mode in MODES}
                if len(fingerprints)!=1:
                    raise AssertionError(f"non-equivalent HTTP output: {size}/{round_id}/{phase}")
    results={}
    for size in SIZES:
        subset=[r for r in rows if r["size"]==size]
        observed={}
        for mode in MODES:
            rr=[r for r in subset if r["mode"]==mode]
            observed[mode]={}
            for metric in ("server_cpu_ms","server_wall_ms","client_wall_ms",
                           "http_p95_ms","http_p99_ms","decision_ms","throttled_periods"):
                vals=[sum(x[metric] for x in rr if x["round"]==i) for i in range(ROUNDS)]
                observed[mode][metric]={"median":round(median(vals),3),"paired":vals}
            observed[mode]["sla_phase_passes"]=sum(x["http_p95_ms"]<=SIZES[size]["slo_ms"] for x in rr)
        comparison={}
        for baseline in ("fixed1","fixed2","fixed4","fixed8",
                         "direct_quota","alpencat_quota","direct_guard"):
            base=observed[baseline]
            al=observed["alpencat_guard"]
            metrics={}
            for metric in ("http_p95_ms","server_cpu_ms","client_wall_ms"):
                gain=[100*(b-a)/b for a,b in
                      zip(al[metric]["paired"],base[metric]["paired"])]
                metrics[metric]={"paired_gains_pct":[round(x,2) for x in gain],
                                 "median_gain_pct":round(median(gain),2),
                                 "positive_over_5_pct":sum(x>5 for x in gain)}
            comparison[baseline]=metrics
        # GO only if a useful p95/CPU/throughput frontier persists in
        # all scenarios; comparing same guard policy checks SDK parity.
        base=comparison["fixed8"]
        parity=comparison["direct_guard"]
        benefit=(base["http_p95_ms"]["positive_over_5_pct"]>=3
                 and base["server_cpu_ms"]["median_gain_pct"]>=-2
                 and base["client_wall_ms"]["median_gain_pct"]>=-5)
        sdk_parity=(abs(parity["http_p95_ms"]["median_gain_pct"])<5
                    and abs(parity["server_cpu_ms"]["median_gain_pct"])<5)
        results[size]={
            "absolute_p95_slo_ms":SIZES[size]["slo_ms"],
            "medians_by_mode":{m:{k:v["median"] for k,v in val.items() if k!="sla_phase_passes"}
                               for m,val in observed.items()},
            "sla_phase_passes_by_mode":{m:val["sla_phase_passes"] for m,val in observed.items()},
            "paired_comparisons":comparison,
            "guard_tradeoff_gain_vs_fixed8":benefit,
            "alpencat_same_policy_parity":sdk_parity,
        }
        print(f"SCENARIO size={size} real_cgroup_quota=2->0.5->2 cpu p95_slo_ms={SIZES[size]['slo_ms']}",flush=True)
        for mode in MODES:
            v=results[size]["medians_by_mode"][mode]
            print(f"MEDIAN size={size} mode={mode} http_p95_sum_ms={v['http_p95_ms']:.1f} "
                  f"cpu_sum_ms={v['server_cpu_ms']:.1f} http_wall_sum_ms={v['client_wall_ms']:.1f} "
                  f"throttled_periods={v['throttled_periods']:.0f} "
                  f"slo_phase_passes={observed[mode]['sla_phase_passes']}/12",flush=True)
        for base,vals in comparison.items():
            print(f"GAIN size={size} baseline={base} p95_pct={vals['http_p95_ms']['median_gain_pct']:.2f} "
                  f"cpu_pct={vals['server_cpu_ms']['median_gain_pct']:.2f} "
                  f"wall_pct={vals['client_wall_ms']['median_gain_pct']:.2f}",flush=True)
        print(f"VERDICT size={size} tradeoff={benefit} parity_with_direct={sdk_parity}",flush=True)
    overall=all(v["guard_tradeoff_gain_vs_fixed8"] and
                v["alpencat_same_policy_parity"] for v in results.values())
    print(f"HTTP_SLO_GUARD_STABLE_GO={str(overall).upper()}",flush=True)
    print("A negative economic outcome is a valid result, not CI failure.",flush=True)
    report={
        "experiment":"real_http_service_dynamic_cgroup_slo",
        "quota_cpu_milli":QUOTAS,
        "workload":SIZES,
        "client_concurrency":PARALLEL_CLIENTS,
        "rounds":ROUNDS,
        "runner":os.uname().release,
        "github_run_id":os.getenv("GITHUB_RUN_ID"),
        "notes":[
            "Uses real tiny_http server, HTTP-over-TCP external Python load and persistent 8-thread pool",
            "Closed-loop 16-client saturated load, not open-loop arrival process",
            "Real own-service systemd Cgroup v2 cpu.max 2 -> 0.5 -> 2; actual cpu.stat",
            "p95 is actual external client full HTTP response latency per phase; tables print SUM of three phase p95s",
            "Predeclared guard policy 4->2->4 is hypothetical and NOT live exploration",
            "OS direct_guard and AlpenCat guard use identical rule; any app gain is from policy, not resource-library speed",
            "All computations validated by response digest equality across strategies and rounds",
            "No publishing to releases/main"
        ],
        "results":results, "raw":rows, "overall_predeclared_go":overall
    }
    with open(a.output,"w",encoding="utf-8") as f:json.dump(report,f,indent=2)
    with open(a.output.replace(".json",".csv"),"w",newline="",encoding="utf-8") as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


if __name__=="__main__":
    main()
