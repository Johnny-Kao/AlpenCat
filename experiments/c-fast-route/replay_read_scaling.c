#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include "alpencat_fast_route.h"

#include <inttypes.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct {
    const ac_exact_route_entry_t* table;
    size_t table_size;
    size_t iterations;
    uint64_t sum;
} worker_arg_t;

static uint64_t
now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static void*
worker(void* raw)
{
    worker_arg_t* arg = raw;
    uint64_t sum = 0;
    for (size_t i = 0; i < arg->iterations; ++i) {
        const size_t key_index = i & 1023u;
        const uint32_t task_class = (uint32_t)(1u + (key_index % 64u));
        const size_t work_items = 2048u + key_index * 257u;
        sum += (uint64_t)alpencat_fast_route_exact_cached_probe8(
                arg->table, arg->table_size, task_class, work_items);
    }
    arg->sum = sum;
    return NULL;
}

static void
run_case(size_t thread_count)
{
    enum { TABLE_SIZE = 4096, KEY_COUNT = 1024 };
    ac_exact_route_entry_t* table =
            calloc(TABLE_SIZE, sizeof(ac_exact_route_entry_t));
    if (!table) {
        exit(2);
    }

    for (size_t i = 0; i < KEY_COUNT; ++i) {
        const uint32_t task_class = (uint32_t)(1u + (i % 64u));
        const size_t work_items = 2048u + i * 257u;
        const ac_route_t route = (i & 1u) ? AC_ROUTE_CPU : AC_ROUTE_SERIAL;
        alpencat_fast_route_exact_publish_probe8(
                table, TABLE_SIZE, task_class, work_items, route);
    }

    const size_t iterations_per_thread = 2000000;
    pthread_t* threads = calloc(thread_count, sizeof(pthread_t));
    worker_arg_t* args = calloc(thread_count, sizeof(worker_arg_t));
    if (!threads || !args) {
        exit(2);
    }

    uint64_t start = now_ns();
    for (size_t i = 0; i < thread_count; ++i) {
        args[i] = (worker_arg_t){
            .table = table,
            .table_size = TABLE_SIZE,
            .iterations = iterations_per_thread,
            .sum = 0,
        };
        if (pthread_create(&threads[i], NULL, worker, &args[i]) != 0) {
            exit(3);
        }
    }

    uint64_t sum = 0;
    for (size_t i = 0; i < thread_count; ++i) {
        pthread_join(threads[i], NULL);
        sum += args[i].sum;
    }
    uint64_t stop = now_ns();

    const size_t calls = thread_count * iterations_per_thread;
    const double ns_per_call = (double)(stop - start) / (double)calls;
    printf(
            "c_probe8_shared_threads_%zu_ns=%.4f calls=%zu sink=%" PRIu64 "\n",
            thread_count,
            ns_per_call,
            calls,
            sum);

    free(args);
    free(threads);
    free(table);
}

int
main(void)
{
    for (size_t threads = 1; threads <= 8; threads *= 2) {
        run_case(threads);
    }
    return 0;
}
