#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct {
    uint32_t task_class;
    size_t work_items;
    ac_route_t oracle_route;
} key_t;

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

static key_t
make_key(size_t index)
{
    key_t key;
    key.task_class = (uint32_t)(1u + (index % 64u));
    key.work_items = 2048u + (index * 257u);
    key.oracle_route = (index & 1u) ? AC_ROUTE_CPU : AC_ROUTE_SERIAL;
    return key;
}

typedef ac_route_t (*lookup_fn)(
        const ac_exact_route_entry_t*, size_t, uint32_t, size_t);
typedef void (*publish_fn)(
        ac_exact_route_entry_t*, size_t, uint32_t, size_t, ac_route_t);

static void
run_case(
        const char* strategy,
        lookup_fn lookup,
        publish_fn publish,
        size_t working_set,
        size_t table_size,
        size_t rounds)
{
    ac_exact_route_entry_t* cache =
            calloc(table_size, sizeof(ac_exact_route_entry_t));
    key_t* keys = malloc(working_set * sizeof(key_t));
    if (!cache || !keys) {
        fprintf(stderr, "allocation failure\n");
        exit(2);
    }

    for (size_t i = 0; i < working_set; ++i) {
        keys[i] = make_key(i);
    }

    size_t cold_adaptive = 0;
    size_t wrong = 0;
    for (size_t i = 0; i < working_set; ++i) {
        key_t key = keys[i];
        ac_route_t route = lookup(
                cache, table_size, key.task_class, key.work_items);
        if (route == AC_ROUTE_ADAPTIVE) {
            ++cold_adaptive;
            route = key.oracle_route;
            publish(
                    cache, table_size, key.task_class, key.work_items, route);
        }
        if (route != key.oracle_route) {
            ++wrong;
        }
    }

    size_t warm_adaptive = 0;
    const size_t calls = working_set * rounds;
    const uint64_t start = now_ns();
    for (size_t round = 0; round < rounds; ++round) {
        for (size_t i = 0; i < working_set; ++i) {
            key_t key = keys[i];
            ac_route_t route = lookup(
                    cache, table_size, key.task_class, key.work_items);
            if (route == AC_ROUTE_ADAPTIVE) {
                ++warm_adaptive;
                route = key.oracle_route;
                publish(
                        cache, table_size, key.task_class, key.work_items, route);
            }
            if (route != key.oracle_route) {
                ++wrong;
            }
        }
    }
    const uint64_t stop = now_ns();

    const double coverage =
            100.0 * (double)(calls - warm_adaptive) / (double)calls;
    const double ns_per_call =
#if defined(_WIN32)
            0.0;
#else
            (double)(stop - start) / (double)calls;
#endif

    printf(
            "capacity_case strategy=%s working_set=%zu table=%zu cold_adaptive=%zu "
            "warm_coverage=%.4f%% warm_ns=%.4f wrong=%zu\n",
            strategy,
            working_set,
            table_size,
            cold_adaptive,
            coverage,
            ns_per_call,
            wrong);

    free(keys);
    free(cache);
}

int
main(void)
{
    printf("exact_entry_bytes=%zu\n", sizeof(ac_exact_route_entry_t));
    const size_t table_sizes[] = {16, 32, 64, 128, 256, 512, 1024, 2048, 4096};
    const size_t working_sets[] = {16, 64, 256, 1024};

    for (size_t wi = 0; wi < sizeof(working_sets) / sizeof(working_sets[0]); ++wi) {
        for (size_t ti = 0; ti < sizeof(table_sizes) / sizeof(table_sizes[0]); ++ti) {
            size_t working_set = working_sets[wi];
            size_t table_size = table_sizes[ti];
            size_t rounds = 1000000u / working_set;
            if (rounds < 1000) {
                rounds = 1000;
            }
            run_case(
                    "direct",
                    alpencat_fast_route_exact_cached,
                    alpencat_fast_route_exact_publish,
                    working_set,
                    table_size,
                    rounds);
            run_case(
                    "probe8",
                    alpencat_fast_route_exact_cached_probe8,
                    alpencat_fast_route_exact_publish_probe8,
                    working_set,
                    table_size,
                    rounds);
        }
    }
    return 0;
}
