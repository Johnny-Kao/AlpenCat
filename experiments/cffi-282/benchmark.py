#!/usr/bin/env python3
import argparse
import json
import statistics
import sys
import time

from cffi import FFI
import _cffi_backend

p = argparse.ArgumentParser()
p.add_argument("--scenario", required=True)
p.add_argument("--library", required=True)
p.add_argument("--matrix-round", type=int, required=True)
p.add_argument("--loops", type=int, default=500_000)
p.add_argument("--rounds", type=int, default=5)
p.add_argument("--warmup", type=int, default=50_000)
args = p.parse_args()

ffi = FFI()
ffi.cdef("""
int cffi_bench_ret0(void);
int cffi_bench_add1(int);
int cffi_bench_add2(int, int);
int cffi_bench_add4(int, int, int, int);
double cffi_bench_add2d(double, double);
int cffi_bench_deref(const int *);
""")
lib = ffi.dlopen(args.library)
ptr = ffi.new("int *", 123)

class IntSubclass(int):
    pass

subclass_one = IntSubclass(1)

CASES = {
    "ret0": (lambda: lib.cffi_bench_ret0(), 1),
    "add1": (lambda: lib.cffi_bench_add1(1), 1),
    "add2": (lambda: lib.cffi_bench_add2(1, 2), 1),
    "add4": (lambda: lib.cffi_bench_add4(1, 2, 3, 4), 1),
    "add2d": (lambda: lib.cffi_bench_add2d(1.0, 2.0), 1),
    "deref_ptr": (lambda: lib.cffi_bench_deref(ptr), 1),
    "add1_subclass": (lambda: lib.cffi_bench_add1(subclass_one), 1),
}

def mixed():
    lib.cffi_bench_ret0()
    lib.cffi_bench_add1(1)
    lib.cffi_bench_add2(1, 2)
    lib.cffi_bench_add4(1, 2, 3, 4)
    lib.cffi_bench_add2d(1.0, 2.0)
    lib.cffi_bench_deref(ptr)
    lib.cffi_bench_add1(subclass_one)

CASES["mixed"] = (mixed, 7)

# Correctness smoke before timing.
assert lib.cffi_bench_ret0() == 7
assert lib.cffi_bench_add1(1) == 2
assert lib.cffi_bench_add2(1, 2) == 3
assert lib.cffi_bench_add4(1, 2, 3, 4) == 10
assert lib.cffi_bench_add2d(1.0, 2.0) == 3.0
assert lib.cffi_bench_deref(ptr) == 123
assert lib.cffi_bench_add1(subclass_one) == 2

print(json.dumps({
    "type": "metadata",
    "scenario": args.scenario,
    "matrix_round": args.matrix_round,
    "python": sys.version,
    "backend_file": _cffi_backend.__file__,
    "loops": args.loops,
    "rounds": args.rounds,
    "warmup": args.warmup,
}), flush=True)

for name, (fn, ops_per_iter) in CASES.items():
    for _ in range(args.warmup):
        fn()

    samples = []
    for _ in range(args.rounds):
        t0 = time.perf_counter_ns()
        for _ in range(args.loops):
            fn()
        elapsed = time.perf_counter_ns() - t0
        samples.append(elapsed / (args.loops * ops_per_iter))

    record = {
        "type": "case",
        "scenario": args.scenario,
        "case": name,
        "ops_per_iter": ops_per_iter,
        "median_ns_per_op": statistics.median(samples),
        "min_ns_per_op": min(samples),
        "max_ns_per_op": max(samples),
        "samples_ns_per_op": samples,
    }
    print(json.dumps(record), flush=True)
