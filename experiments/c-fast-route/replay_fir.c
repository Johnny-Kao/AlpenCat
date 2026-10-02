#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdint.h>

typedef struct {
    size_t work_items;
    ac_route_t observed_best;
} replay_point_t;

typedef struct {
    size_t serial_safe_max;
    size_t cpu_safe_min;
} fir_policy_t;

static ac_route_t
fir_fast_route(const fir_policy_t* policy, size_t work_items)
{
    if (work_items <= policy->serial_safe_max) {
        return AC_ROUTE_SERIAL;
    }
    if (work_items >= policy->cpu_safe_min) {
        return AC_ROUTE_CPU;
    }
    return AC_ROUTE_ADAPTIVE;
}

static void
run_profile(
        const char* name,
        const fir_policy_t* policy,
        const replay_point_t* points,
        size_t count)
{
    size_t direct = 0;
    size_t direct_correct = 0;
    size_t adaptive = 0;

    for (size_t i = 0; i < count; ++i) {
        ac_route_t route = fir_fast_route(policy, points[i].work_items);
        if (route == AC_ROUTE_ADAPTIVE) {
            ++adaptive;
            continue;
        }

        ++direct;
        if (route == points[i].observed_best) {
            ++direct_correct;
        }
    }

    printf(
            "%s direct=%zu adaptive=%zu coverage=%.2f%% direct_accuracy=%.2f%%\n",
            name,
            direct,
            adaptive,
            100.0 * (double)direct / (double)count,
            direct == 0 ? 0.0 : 100.0 * (double)direct_correct / (double)direct);
}

int
main(void)
{
    /*
     * Replay evidence from AlpenCat upfirdn Adaptive Benchmark run 36939765234.
     *
     * The two profiles intentionally leave a two-point uncertainty window
     * around each observed serial/CPU crossover. The window is not optimized
     * to maximize coverage; it is a conservative guardrail intended to test
     * whether FastRoute can skip the expensive control plane without making
     * wrong decisions on the observed matrix.
     */
    static const replay_point_t fir_profile_a[] = {
        {256, AC_ROUTE_SERIAL},
        {512, AC_ROUTE_SERIAL},
        {1024, AC_ROUTE_SERIAL},
        {2048, AC_ROUTE_SERIAL},
        {4096, AC_ROUTE_SERIAL},
        {8192, AC_ROUTE_CPU},
        {16384, AC_ROUTE_CPU},
        {32768, AC_ROUTE_CPU},
        {65536, AC_ROUTE_CPU},
        {262144, AC_ROUTE_CPU},
        {1048576, AC_ROUTE_CPU},
    };

    static const replay_point_t fir_profile_b[] = {
        {256, AC_ROUTE_SERIAL},
        {512, AC_ROUTE_SERIAL},
        {1024, AC_ROUTE_SERIAL},
        {2048, AC_ROUTE_CPU},
        {4096, AC_ROUTE_CPU},
        {8192, AC_ROUTE_CPU},
        {16384, AC_ROUTE_CPU},
        {32768, AC_ROUTE_CPU},
        {65536, AC_ROUTE_CPU},
        {262144, AC_ROUTE_CPU},
        {1048576, AC_ROUTE_CPU},
    };

    const fir_policy_t policy_a = {
        .serial_safe_max = 2048,
        .cpu_safe_min = 16384,
    };
    const fir_policy_t policy_b = {
        .serial_safe_max = 512,
        .cpu_safe_min = 4096,
    };

    run_profile(
            "fir_profile_a",
            &policy_a,
            fir_profile_a,
            sizeof(fir_profile_a) / sizeof(fir_profile_a[0]));
    run_profile(
            "fir_profile_b",
            &policy_b,
            fir_profile_b,
            sizeof(fir_profile_b) / sizeof(fir_profile_b[0]));

    return 0;
}
