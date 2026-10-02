#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

typedef struct {
    uint32_t task_class;
    size_t work_items;
    ac_route_t observed_best;
    size_t serial_safe_max;
    size_t cpu_safe_min;
} replay_point_t;

static uint64_t
now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static ac_route_t
threshold_route(const replay_point_t* point)
{
    if (point->work_items <= point->serial_safe_max) {
        return AC_ROUTE_SERIAL;
    }
    if (point->work_items >= point->cpu_safe_min) {
        return AC_ROUTE_CPU;
    }
    return AC_ROUTE_ADAPTIVE;
}

static ac_route_t
two_level_route(
        ac_exact_route_entry_t* cache,
        size_t cache_size,
        const replay_point_t* point,
        size_t* adaptive_calls)
{
    ac_route_t route = threshold_route(point);
    if (route != AC_ROUTE_ADAPTIVE) {
        return route;
    }

    route = alpencat_fast_route_exact_cached(
            cache,
            cache_size,
            point->task_class,
            point->work_items);
    if (route != AC_ROUTE_ADAPTIVE) {
        return route;
    }

    /*
     * Replay-only oracle standing in for the slow M12/M13 path.
     * Production code would call the adaptive control plane here, then publish
     * its validated decision into the read-mostly cache.
     */
    ++*adaptive_calls;
    route = point->observed_best;
    alpencat_fast_route_exact_publish(
            cache,
            cache_size,
            point->task_class,
            point->work_items,
            route);
    return route;
}

int
main(void)
{
    static const replay_point_t points[] = {
        {1, 256, AC_ROUTE_SERIAL, 2048, 16384},
        {1, 512, AC_ROUTE_SERIAL, 2048, 16384},
        {1, 1024, AC_ROUTE_SERIAL, 2048, 16384},
        {1, 2048, AC_ROUTE_SERIAL, 2048, 16384},
        {1, 4096, AC_ROUTE_SERIAL, 2048, 16384},
        {1, 8192, AC_ROUTE_CPU, 2048, 16384},
        {1, 16384, AC_ROUTE_CPU, 2048, 16384},
        {1, 32768, AC_ROUTE_CPU, 2048, 16384},
        {1, 65536, AC_ROUTE_CPU, 2048, 16384},
        {1, 262144, AC_ROUTE_CPU, 2048, 16384},
        {1, 1048576, AC_ROUTE_CPU, 2048, 16384},

        {2, 256, AC_ROUTE_SERIAL, 512, 4096},
        {2, 512, AC_ROUTE_SERIAL, 512, 4096},
        {2, 1024, AC_ROUTE_SERIAL, 512, 4096},
        {2, 2048, AC_ROUTE_CPU, 512, 4096},
        {2, 4096, AC_ROUTE_CPU, 512, 4096},
        {2, 8192, AC_ROUTE_CPU, 512, 4096},
        {2, 16384, AC_ROUTE_CPU, 512, 4096},
        {2, 32768, AC_ROUTE_CPU, 512, 4096},
        {2, 65536, AC_ROUTE_CPU, 512, 4096},
        {2, 262144, AC_ROUTE_CPU, 512, 4096},
        {2, 1048576, AC_ROUTE_CPU, 512, 4096},
    };

    enum { CACHE_SIZE = 64 };
    ac_exact_route_entry_t cache[CACHE_SIZE];
    memset(cache, 0, sizeof(cache));

    size_t adaptive_calls = 0;
    size_t wrong = 0;

    /* Cold pass: only ambiguous points may enter the adaptive control plane. */
    for (size_t i = 0; i < sizeof(points) / sizeof(points[0]); ++i) {
        ac_route_t route = two_level_route(cache, CACHE_SIZE, &points[i], &adaptive_calls);
        if (route != points[i].observed_best) {
            ++wrong;
        }
    }

    const size_t cold_adaptive_calls = adaptive_calls;

    /* Warm replay: repeat a stable workload many times. */
    const size_t rounds = 1000000;
    const size_t point_count = sizeof(points) / sizeof(points[0]);
    const uint64_t start = now_ns();
    for (size_t round = 0; round < rounds; ++round) {
        for (size_t i = 0; i < point_count; ++i) {
            ac_route_t route = two_level_route(cache, CACHE_SIZE, &points[i], &adaptive_calls);
            if (route != points[i].observed_best) {
                ++wrong;
            }
        }
    }
    const uint64_t stop = now_ns();

    const size_t warm_calls = rounds * point_count;
    const size_t warm_adaptive_calls = adaptive_calls - cold_adaptive_calls;
    const double warm_fast_coverage =
            100.0 * (double)(warm_calls - warm_adaptive_calls) / (double)warm_calls;
    const double warm_ns = (double)(stop - start) / (double)warm_calls;

    printf("cold_points=%zu\n", point_count);
    printf("cold_adaptive_calls=%zu\n", cold_adaptive_calls);
    printf(
            "cold_adaptive_rate=%.6f%%\n",
            100.0 * (double)cold_adaptive_calls / (double)point_count);
    printf("warm_calls=%zu\n", warm_calls);
    printf("warm_adaptive_calls=%zu\n", warm_adaptive_calls);
    printf("warm_fast_coverage=%.6f%%\n", warm_fast_coverage);
    printf("warm_route_ns=%.4f\n", warm_ns);
    printf("wrong_routes=%zu\n", wrong);

    return wrong == 0 ? 0 : 1;
}
