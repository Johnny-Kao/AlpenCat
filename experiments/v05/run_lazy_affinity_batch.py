#!/usr/bin/env python3
"""Control a single Rust process through allowed CPU affinity envelopes."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--binary",required=True)
    ap.add_argument("--out",required=True)
    args=ap.parse_args()
    allowed=sorted(os.sched_getaffinity(0))
    if not allowed:
        raise RuntimeError("No permitted CPU")
    # Prefer 4 -> 2 -> 1 -> 2 -> 4. Collapse to physical runner limits.
    schedule=[allowed[:min(k,len(allowed))] for k in (4,2,1,2,4)]
    proc=subprocess.Popen([args.binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE,text=True,bufsize=1)
    records=[]
    try:
        for phase,cpus in enumerate(schedule):
            ready=proc.stdout.readline().strip()
            assert ready==f"READY {phase}", (ready,phase)
            os.sched_setaffinity(proc.pid,cpus)
            actual=sorted(os.sched_getaffinity(proc.pid))
            assert actual==cpus,(actual,cpus)
            proc.stdin.write("GO\n")
            proc.stdin.flush()
            row=proc.stdout.readline().strip()
            assert row.startswith(f"RESULT phase={phase} "), row
            vals=dict(v.split("=",1) for v in row.split()[1:])
            parsed={k:int(v) if v.isdigit() else v for k,v in vals.items()}
            parsed["affinity"]=cpus
            records.append(parsed)
            print(json.dumps(parsed),flush=True)
        rc=proc.wait(timeout=20)
        assert rc==0,(rc,proc.stderr.read())
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
    # No signal on unchanged effective CPU count; eager still calibrates.
    for x in records[1:]:
        if not x["changed"]:
            assert x["lazy_calibration_ns"] < x["eager_calibration_ns"], x
    out=Path(args.out)
    out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text(json.dumps(dict(allowed=allowed,schedule=schedule,records=records,
        note="Wall-clock only; fixed order per trial, not CPU-time economics or performance GO"),indent=2)+"\n")
    print("LIFECYCLE_BATCH_PASS")

if __name__=="__main__":
    main()
