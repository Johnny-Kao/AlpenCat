import json, math, os
from pathlib import Path
from cffi import FFI

out=Path(os.environ.get("BENCH_OUT","artifacts"))
out.mkdir(parents=True,exist_ok=True)
ffi=FFI()
ffi.cdef("""
int cffi_bench_add1(int);
int cffi_bench_add2(int,int);
double cffi_bench_add2d(double,double);
int cffi_bench_deref(const int *);
int cffi_bench_counted_add2(int, int);
void cffi_bench_reset_counter(void);
unsigned long long cffi_bench_get_counter(void);
""")
lib=ffi.dlopen(str((Path.cwd()/"libcffi_bench.so").resolve()))

class MyInt(int): pass
class IntLike:
    def __int__(self): return 7

p=ffi.new("int *",123)
cases=[
 ("int-normal",lambda:lib.cffi_bench_add1(4)),
 ("int-bool",lambda:lib.cffi_bench_add1(True)),
 ("int-subclass",lambda:lib.cffi_bench_add1(MyInt(4))),
 ("int-custom",lambda:lib.cffi_bench_add1(IntLike())),
 ("int-huge",lambda:lib.cffi_bench_add1(1<<100)),
 ("int-float",lambda:lib.cffi_bench_add1(4.0)),
 ("i32-min",lambda:lib.cffi_bench_add1(-(2**31))),
 ("i32-max",lambda:lib.cffi_bench_add1(2**31-1)),
 ("i32-under",lambda:lib.cffi_bench_add1(-(2**31)-1)),
 ("i32-over",lambda:lib.cffi_bench_add1(2**31)),
 ("double-normal",lambda:lib.cffi_bench_add2d(1.5,2.5)),
 ("double-int",lambda:lib.cffi_bench_add2d(1,2)),
 ("double-negzero",lambda:lib.cffi_bench_add2d(-0.0,0.0)),
 ("double-inf",lambda:lib.cffi_bench_add2d(float("inf"),1.0)),
 ("double-nan",lambda:lib.cffi_bench_add2d(float("nan"),1.0)),
 ("pointer",lambda:lib.cffi_bench_deref(p)),
]
rows=[]
for name,fn in cases:
    try:
        value=fn()
        if isinstance(value,float) and math.isnan(value): rep="nan"
        else: rep=repr(value)
        rows.append({"case":name,"status":"OK","value":rep})
    except Exception as exc:
        rows.append({"case":name,"status":"ERR","type":type(exc).__name__,"message":str(exc)})
# Safety invariant for fallback/shadow experiments:
# every logical Python call must issue exactly one C call.
lib.cffi_bench_reset_counter()
counted_calls = int(os.environ.get("CORRECTNESS_COUNTED_CALLS", "10000"))
for i in range(counted_calls):
    value = lib.cffi_bench_counted_add2(i & 7, 2)
    if value != (i & 7) + 2:
        raise AssertionError("counted_add2 result mismatch")
counter_value = int(lib.cffi_bench_get_counter())
rows.append({
    "case": "single-ffi-call-invariant",
    "status": "OK" if counter_value == counted_calls else "ERR",
    "logical_calls": counted_calls,
    "c_calls": counter_value,
})
if counter_value != counted_calls:
    raise AssertionError(
        f"double/missing C invocation: logical={counted_calls} c_calls={counter_value}"
    )

(out/"correctness.json").write_text(json.dumps(rows,indent=2))
for row in rows: print(json.dumps(row,sort_keys=True))
