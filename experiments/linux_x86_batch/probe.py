#!/usr/bin/env python3
"""Linux x86 native-signal batch probe. No routing policy or performance claims."""
import argparse, json, os, platform, re, subprocess, time, multiprocessing as mp
from pathlib import Path

def read(path):
    try: return {"status":"ok","value":Path(path).read_text().strip()}
    except OSError as e: return {"status":"unavailable","reason":str(e)}

def cpuset_count(s):
    try:
        return sum((int(b)-int(a)+1) if "-" in p for p in s.split(",") for a,b in ([p.split("-",1)] if "-" in p else [(p,p)]))
    except Exception: return None

def cgroup_mount():
    try:
        for line in Path("/proc/self/mountinfo").read_text().splitlines():
            fields=line.split(" - ")
            if len(fields)==2 and fields[1].startswith("cgroup2 "): return fields[0].split()[4]
    except OSError: pass
    return None

def snapshot():
    mount=cgroup_mount()
    try:
        entry=next((x.split("::",1)[1] for x in Path("/proc/self/cgroup").read_text().splitlines() if x.startswith("0::")),"/")
    except OSError: entry="/"
    cg=Path(mount) / entry.lstrip("/") if mount else None
    paths=["/proc/pressure/cpu","/proc/pressure/memory","/proc/pressure/io"]
    data={p:read(p) for p in paths}
    for name in ("cpu.max","cpu.stat","cpuset.cpus.effective","memory.current","memory.max","memory.events","cpu.pressure","memory.pressure","io.pressure"):
        data["cgroup/"+name]=read(str(cg/name)) if cg else {"status":"unavailable","reason":"cgroup v2 not mounted"}
    data["online_cpu_count"]=os.cpu_count()
    data["affinity_count"]=len(os.sched_getaffinity(0)) if hasattr(os,"sched_getaffinity") else None
    data["cpuset_effective_count"]=cpuset_count(data["cgroup/cpuset.cpus.effective"].get("value","")) if data["cgroup/cpuset.cpus.effective"]["status"]=="ok" else None
    try: data["lscpu_json"]=json.loads(subprocess.check_output(["lscpu","-J"],text=True,timeout=5))
    except Exception as e: data["lscpu_json"]={"status":"unavailable","reason":str(e)}
    data["numa_online"]=read("/sys/devices/system/node/online")
    data["cpuinfo_model"]=next((x.strip() for x in Path("/proc/cpuinfo").read_text().splitlines() if x.startswith("model name")),"unavailable") if Path("/proc/cpuinfo").exists() else "unavailable"
    return data

def burn(end):
    n=0
    while time.monotonic()<end: n=(n*1664525+1013904223)&0xffffffff

def phase(name,seconds,workers=0,mem_mib=0):
    before=snapshot(); t0=time.monotonic_ns()
    procs=[]
    if workers:
        end=time.monotonic()+seconds
        procs=[mp.Process(target=burn,args=(end,)) for _ in range(workers)]
        for p in procs:p.start()
    buf=bytearray(mem_mib*1024*1024) if mem_mib else None
    if buf:
        for i in range(0,len(buf),4096): buf[i]=1
    if procs:
        for p in procs:p.join(timeout=seconds+5)
        for p in procs:
            if p.is_alive():p.terminate();p.join()
    else:time.sleep(seconds)
    after=snapshot()
    return {"phase":name,"elapsed_ns":time.monotonic_ns()-t0,"before":before,"after":after}

def main():
    ap=argparse.ArgumentParser();ap.add_argument("--out",required=True);ap.add_argument("--id",required=True);ap.add_argument("--duration",type=float,default=2.0);a=ap.parse_args()
    Path(a.out).mkdir(parents=True,exist_ok=True)
    arch=platform.machine().lower()
    meta={"id":a.id,"arch":arch,"kernel":platform.release(),"platform":platform.platform(),"runner":os.getenv("RUNNER_NAME"),"run_id":os.getenv("GITHUB_RUN_ID")}
    base=snapshot()
    eligible=arch in ("x86_64","amd64") and base["/proc/pressure/cpu"]["status"]=="ok"
    meta["eligible"]=eligible;meta["reason"]="eligible" if eligible else "architecture or CPU PSI unavailable"
    result={"metadata":meta,"inventory":base,"phases":[],"interpretation":"inventory/transition proxy only; no oracle opportunities or routing benefit inferred"}
    if eligible:
        cpu=max(1,base.get("affinity_count") or 1)
        worker_count=min(8,max(2,cpu+1))
        result["phases"]=[phase("baseline",a.duration),phase("cpu-pressure",a.duration,workers=worker_count),phase("memory-light",a.duration,mem_mib=16),phase("memory-heavy",a.duration,mem_mib=64),phase("combined",a.duration,workers=worker_count,mem_mib=64),phase("recovery",a.duration)]
    dest=Path(a.out)/"result.json";dest.write_text(json.dumps(result,indent=2))
    print(json.dumps({"id":a.id,"eligible":eligible,"phases":len(result["phases"]),"output":str(dest)}))
if __name__=="__main__":main()
