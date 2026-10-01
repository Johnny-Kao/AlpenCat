import ctypes, json, os, resource, statistics, sys, time
from pathlib import Path
from cffi import FFI
import cffi, _cffi_backend

OUT=Path(os.environ.get("BENCH_OUT","artifacts"))
OUT.mkdir(parents=True, exist_ok=True)
scenario=os.environ["SCENARIO"]
loops=int(os.environ.get("BENCH_LOOPS","1000000"))
rounds=int(os.environ.get("BENCH_ROUNDS","11"))
warmup=int(os.environ.get("BENCH_WARMUP","100000"))

ffi=FFI()
ffi.cdef("""
int cffi_bench_ret0(void);
int cffi_bench_add1(int);
int cffi_bench_add2(int, int);
int cffi_bench_add4(int, int, int, int);
double cffi_bench_add2d(double, double);
int cffi_bench_deref(const int *);
""")
lib=ffi.dlopen(str((Path.cwd()/"libcffi_bench.so").resolve()))
p=ffi.new("int *",123)
cases={
 "ret0":lambda:lib.cffi_bench_ret0(),
 "add1":lambda:lib.cffi_bench_add1(1),
 "add2":lambda:lib.cffi_bench_add2(1,2),
 "add4":lambda:lib.cffi_bench_add4(1,2,3,4),
 "add2d":lambda:lib.cffi_bench_add2d(1.0,2.0),
 "deref_ptr":lambda:lib.cffi_bench_deref(p),
}
meta={
 "scenario":scenario,"python":sys.version,"cffi_file":cffi.__file__,
 "backend_file":_cffi_backend.__file__,"loops":loops,"rounds":rounds,"warmup":warmup,
 "function_cdata_size":sys.getsizeof(lib.cffi_bench_add2),
 "pointer_cdata_size":sys.getsizeof(p),
}
(OUT/"python_identity.json").write_text(json.dumps(meta,indent=2))
raw=open(OUT/"benchmark.jsonl","w")
summary={}
for name,fn in cases.items():
    for _ in range(warmup): fn()
    vals=[]
    cpu_vals=[]
    for i in range(rounds):
        t0=time.perf_counter_ns(); c0=time.process_time_ns()
        value=None
        for _ in range(loops): value=fn()
        cpu=time.process_time_ns()-c0; wall=time.perf_counter_ns()-t0
        ns=wall/loops; cns=cpu/loops
        vals.append(ns); cpu_vals.append(cns)
        raw.write(json.dumps({"scenario":scenario,"case":name,"round":i,
          "wall_ns_per_call":ns,"cpu_ns_per_call":cns,"wall_total_ns":wall,
          "cpu_total_ns":cpu,"result":repr(value),
          "ru_maxrss":resource.getrusage(resource.RUSAGE_SELF).ru_maxrss})+"\n")
        raw.flush()
    s=sorted(vals)
    summary[name]={
      "median_ns":statistics.median(vals),"mean_ns":statistics.mean(vals),
      "min_ns":min(vals),"max_ns":max(vals),
      "p95_ns":s[min(len(s)-1,round((len(s)-1)*.95))],
      "stdev_ns":statistics.pstdev(vals),
      "median_cpu_ns":statistics.median(cpu_vals),
      "calls_per_sec":1e9/statistics.median(vals)
    }
raw.close()

swiss_stats = None
try:
    backend = ctypes.CDLL(_cffi_backend.__file__)
    names = [
        "runtime_cffi_route_calls",
        "runtime_cffi_fast_routes",
        "runtime_cffi_legacy_routes",
        "runtime_cffi_shadow_checks",
        "runtime_cffi_shadow_mismatches",
        "runtime_cffi_backup_fallbacks",
    ]
    swiss_stats = {}
    for name in names:
        fn = getattr(backend, name)
        fn.restype = ctypes.c_ulonglong
        swiss_stats[name] = int(fn())
    (OUT/"swiss-counters.json").write_text(json.dumps(swiss_stats,indent=2))
except (AttributeError, OSError):
    pass

payload={"scenario":scenario,"cases":summary,"swiss_stats":swiss_stats}
(OUT/"summary.json").write_text(json.dumps(payload,indent=2))
print(json.dumps(payload,indent=2))
