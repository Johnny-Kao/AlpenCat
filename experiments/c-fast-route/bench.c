#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#if defined(_WIN32)
#include <windows.h>
static uint64_t now_ns(void)
{
    LARGE_INTEGER frequency;
    LARGE_INTEGER counter;
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&counter);
    return (uint64_t)((counter.QuadPart * 1000000000ULL) / frequency.QuadPart);
}
#else
static uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}
#endif

static volatile uint64_t sink = 0;

static double
bench_constant(size_t iterations)
{
    uint64_t sum = 0;
    uint64_t start = now_ns();
    for (size_t i = 0; i < iterations; ++i) {
        sum += (uint64_t)alpencat_fast_route_constant();
    }
    uint64_t stop = now_ns();
    sink += sum;
    return (double)(stop - start) / (double)iterations;
}

static double
bench_threshold_fixed(const ac_fast_policy_t* policy, size_t iterations)
{
    uint64_t sum = 0;
    uint64_t start = now_ns();
    for (size_t i = 0; i < iterations; ++i) {
        sum += (uint64_t)alpencat_fast_route(policy, 65536, AC_CAP_CPU | AC_CAP_GPU);
    }
    uint64_t stop = now_ns();
    sink += sum;
    return (double)(stop - start) / (double)iterations;
}

static double
bench_threshold_mixed(
        const ac_fast_policy_t* policy,
        const size_t* sizes,
        const uint32_t* states,
        size_t input_count,
        size_t rounds)
{
    uint64_t sum = 0;
    uint64_t start = now_ns();
    for (size_t round = 0; round < rounds; ++round) {
        for (size_t i = 0; i < input_count; ++i) {
            sum += (uint64_t)alpencat_fast_route(policy, sizes[i], states[i]);
        }
    }
    uint64_t stop = now_ns();
    sink += sum;
    return (double)(stop - start) / (double)(input_count * rounds);
}

static double
bench_cached_mixed(
        const uint8_t* table,
        const size_t* sizes,
        size_t input_count,
        size_t rounds)
{
    uint64_t sum = 0;
    uint64_t start = now_ns();
    for (size_t round = 0; round < rounds; ++round) {
        for (size_t i = 0; i < input_count; ++i) {
            sum += (uint64_t)alpencat_fast_route_cached(table, sizes[i]);
        }
    }
    uint64_t stop = now_ns();
    sink += sum;
    return (double)(stop - start) / (double)(input_count * rounds);
}

int
main(void)
{
    enum { INPUT_COUNT = 65536 };
    const size_t iterations = 50000000;
    const size_t rounds = 500;

    size_t* sizes = malloc(sizeof(size_t) * INPUT_COUNT);
    uint32_t* states = malloc(sizeof(uint32_t) * INPUT_COUNT);
    if (!sizes || !states) {
        return 2;
    }

    uint64_t x = 0x9e3779b97f4a7c15ULL;
    for (size_t i = 0; i < INPUT_COUNT; ++i) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        unsigned shift = (unsigned)(x % 21u);
        sizes[i] = ((size_t)1 << shift) + (size_t)(x & 1023u);
        states[i] = AC_CAP_CPU;
        if ((x >> 12) & 1u) {
            states[i] |= AC_CAP_GPU;
        }
        if ((x >> 17) % 97u == 0) {
            states[i] |= AC_STATE_NESTED_PARALLEL;
        }
    }

    ac_fast_policy_t policy = {
        .serial_max = 1024,
        .gpu_min = 262144,
        .required_gpu_state = 0,
    };

    uint8_t table[sizeof(size_t) * 8u];
    for (size_t bucket = 0; bucket < sizeof(table); ++bucket) {
        if (bucket <= 10) {
            table[bucket] = AC_ROUTE_SERIAL;
        } else if (bucket < 18) {
            table[bucket] = AC_ROUTE_CPU;
        } else {
            table[bucket] = AC_ROUTE_GPU;
        }
    }

    printf("iterations_constant=%zu\n", iterations);
    printf("mixed_calls=%zu\n", (size_t)INPUT_COUNT * rounds);
    printf("c_constant_call_ns=%.4f\n", bench_constant(iterations));
    printf("c_threshold_fixed_ns=%.4f\n", bench_threshold_fixed(&policy, iterations));
    printf(
            "c_threshold_mixed_ns=%.4f\n",
            bench_threshold_mixed(&policy, sizes, states, INPUT_COUNT, rounds));
    printf(
            "c_cached_bucket_mixed_ns=%.4f\n",
            bench_cached_mixed(table, sizes, INPUT_COUNT, rounds));
    printf("sink=%" PRIu64 "\n", sink);

    free(states);
    free(sizes);
    return 0;
}
