#!/usr/bin/env python3
"""Execute frozen universal economics with contemporaneous PSI on the same runner."""
import importlib.util,json,pathlib,re,sys,time
ROOT=pathlib.Path(__file__).resolve().parents[2]
target=ROOT/"experiments/convergence/run_universal_cross_machine.py"
spec=importlib.util.spec_from_file_location("frozen_universal",target)
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
original=module.run_example
def readpsi(path):
    try:
        v=pathlib.Path(path).read_text();return {m.group(1):int(m.group(2)) for m in re.finditer(r"(?m)^(some|full) .*?total=(\d+)",v)}
    except OSError as e:return {"unavailable":str(e)}
def capture(binary,out_path,regime,start_boundary,bootstrap,repeats):
    paths={"/proc/pressure/cpu":"/proc/pressure/cpu","/proc/pressure/memory":"/proc/pressure/memory","/proc/pressure/io":"/proc/pressure/io"}
    before={k:readpsi(v) for k,v in paths.items()}
    t0=time.monotonic_ns()
    original(binary,out_path,regime,start_boundary,bootstrap,repeats)
    dt=time.monotonic_ns()-t0
    after={k:readpsi(v) for k,v in paths.items()}
    (out_path.parent/(regime+".psi.json")).write_text(json.dumps({"before":before,"after":after,"elapsed_ns":dt,"workload":out_path.parent.name,"regime":regime},indent=2))
module.run_example=capture
if __name__=="__main__":module.main()
