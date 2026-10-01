#include <stdint.h>

static uint64_t cffi_bench_call_counter = 0;

int cffi_bench_ret0(void) { return 7; }
int cffi_bench_add1(int a) { return a + 1; }
int cffi_bench_add2(int a, int b) { return a + b; }
int cffi_bench_add4(int a, int b, int c, int d) { return a + b + c + d; }
double cffi_bench_add2d(double a, double b) { return a + b; }
int cffi_bench_deref(const int *p) { return *p; }

int cffi_bench_counted_add2(int a, int b) {
    cffi_bench_call_counter += 1;
    return a + b;
}

void cffi_bench_reset_counter(void) {
    cffi_bench_call_counter = 0;
}

uint64_t cffi_bench_get_counter(void) {
    return cffi_bench_call_counter;
}
