#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static uint64_t
now_ns(void)
{
#if defined(_WIN32)
    return 0;
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
#endif
}

static ac_route_t
oracle(uint64_t epoch, size_t work_items)
{
    if (work_items <= 2048) {
        return AC_ROUTE_SERIAL;
    }
    return (epoch & 1u) ? AC_ROUTE_CPU : AC_ROUTE_GPU;
}

int
main(void)
{
    enum { KEY_COUNT = 128 };
    const size_t table_sizes[] = {512, 1024, 2048, 4096};
    const size_t epochs = 10000;
    const size_t calls_per_epoch = KEY_COUNT * 4;
    size_t total_wrong = 0;

    for (size_t ti = 0; ti < sizeof(table_sizes) / sizeof(table_sizes[0]); ++ti) {
        const size_t table_size = table_sizes[ti];
        ac_exact_route_entry_t* cache =
                calloc(table_size, sizeof(ac_exact_route_entry_t));
        if (!cache) {
            return 2;
        }

        size_t wrong = 0;
        size_t adaptive = 0;

        /*
         * First prove that the same task+size under a new policy epoch cannot
         * hit the prior epoch's route.
         */
        alpencat_fast_route_epoch_publish_probe8(
                cache, table_size, 1, 7, 65536, AC_ROUTE_CPU);
        if (alpencat_fast_route_epoch_cached_probe8(
                    cache, table_size, 2, 7, 65536)
            != AC_ROUTE_ADAPTIVE)
        {
            ++wrong;
        }

        const uint64_t start = now_ns();
        for (uint64_t epoch = 1; epoch <= epochs; ++epoch) {
        /*
         * Cold pass for this epoch: misses are expected and stand in for the
         * adaptive control plane publishing the new policy generation.
         */
        for (size_t i = 0; i < KEY_COUNT; ++i) {
            const size_t work_items = 1024 + i * 257;
            ac_route_t expected = oracle(epoch, work_items);
            ac_route_t route = alpencat_fast_route_epoch_cached_probe8(
                    cache, table_size, epoch, 1, work_items);
            if (route == AC_ROUTE_ADAPTIVE) {
                ++adaptive;
                route = expected;
                alpencat_fast_route_epoch_publish_probe8(
                        cache, table_size, epoch, 1, work_items, route);
            }
            if (route != expected) {
                ++wrong;
            }
        }

        /*
         * Three warm replays in the same epoch.
         */
        for (size_t replay = 0; replay < 3; ++replay) {
            for (size_t i = 0; i < KEY_COUNT; ++i) {
                const size_t work_items = 1024 + i * 257;
                ac_route_t expected = oracle(epoch, work_items);
                ac_route_t route = alpencat_fast_route_epoch_cached_probe8(
                        cache, table_size, epoch, 1, work_items);
                if (route == AC_ROUTE_ADAPTIVE) {
                    ++adaptive;
                    route = expected;
                    alpencat_fast_route_epoch_publish_probe8(
                            cache, table_size, epoch, 1, work_items, route);
                }
                if (route != expected) {
                    ++wrong;
                }
            }
        }
        }

        const uint64_t stop = now_ns();
        const size_t calls = epochs * calls_per_epoch;
        const double adaptive_rate = 100.0 * (double)adaptive / (double)calls;
        const double ns_per_call =
#if defined(_WIN32)
                0.0;
#else
                (double)(stop - start) / (double)calls;
#endif

        printf(
                "policy_churn table=%zu epochs=%zu calls=%zu adaptive_calls=%zu "
                "adaptive_rate=%.4f%% route_ns=%.4f wrong=%zu\n",
                table_size,
                epochs,
                calls,
                adaptive,
                adaptive_rate,
                ns_per_call,
                wrong);
        total_wrong += wrong;
        free(cache);
    }

    return total_wrong == 0 ? 0 : 1;
}
