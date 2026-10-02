#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef enum {
    TASK_CPU_ONLY = 0,
    TASK_GPU_PREFERRED = 1,
    TASK_EITHER = 2
} task_kind_t;

typedef struct {
    task_kind_t kind;
    atomic_uchar claimed;
} shared_task_t;

typedef enum {
    WORKER_CPU = 0,
    WORKER_GPU = 1
} worker_kind_t;

typedef struct {
    size_t* items;
    size_t count;
    atomic_size_t cursor;
} index_queue_t;

typedef struct {
    shared_task_t* tasks;
    size_t task_count;
    atomic_size_t scan_cursor;
    atomic_size_t completed;
    worker_kind_t worker_kind;
    size_t claimed;
    size_t incompatible_seen;
} shared_worker_arg_t;

typedef struct {
    index_queue_t* cpu;
    index_queue_t* gpu;
    index_queue_t* either;
    atomic_size_t* completed;
    size_t task_count;
    worker_kind_t worker_kind;
    size_t claimed;
} split_worker_arg_t;

typedef struct {
    double build_ns_per_task;
    double claim_ns_per_task;
} split_result_t;

typedef struct {
    atomic_size_t* cursor;
    size_t task_count;
    worker_kind_t worker_kind;
    size_t claimed;
} any_worker_arg_t;

static uint64_t
now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static void*
any_worker(void* raw)
{
    any_worker_arg_t* arg = raw;
    const size_t batch = arg->worker_kind == WORKER_GPU ? 32 : 1;
    size_t claimed = 0;
    for (;;) {
        size_t start = atomic_fetch_add_explicit(
                arg->cursor, batch, memory_order_relaxed);
        if (start >= arg->task_count) {
            break;
        }
        size_t remaining = arg->task_count - start;
        claimed += remaining < batch ? remaining : batch;
    }
    arg->claimed = claimed;
    return NULL;
}

static double
run_shared_any(
        size_t task_count,
        size_t cpu_workers,
        size_t gpu_workers)
{
    const size_t worker_count = cpu_workers + gpu_workers;
    pthread_t* threads = calloc(worker_count, sizeof(pthread_t));
    any_worker_arg_t* args = calloc(worker_count, sizeof(any_worker_arg_t));
    if (!threads || !args) {
        exit(2);
    }

    atomic_size_t cursor;
    atomic_init(&cursor, 0);
    uint64_t start = now_ns();
    for (size_t i = 0; i < worker_count; ++i) {
        args[i] = (any_worker_arg_t){
            .cursor = &cursor,
            .task_count = task_count,
            .worker_kind = i < cpu_workers ? WORKER_CPU : WORKER_GPU,
            .claimed = 0,
        };
        if (pthread_create(&threads[i], NULL, any_worker, &args[i]) != 0) {
            exit(3);
        }
    }
    size_t claimed = 0;
    for (size_t i = 0; i < worker_count; ++i) {
        pthread_join(threads[i], NULL);
        claimed += args[i].claimed;
    }
    uint64_t stop = now_ns();
    if (claimed != task_count) {
        fprintf(stderr, "shared-any claim mismatch: %zu != %zu\n", claimed, task_count);
        exit(4);
    }

    free(args);
    free(threads);
    return (double)(stop - start) / (double)task_count;
}

static int
compatible(worker_kind_t worker, task_kind_t task)
{
    if (worker == WORKER_GPU) {
        return task != TASK_CPU_ONLY;
    }
    return task != TASK_GPU_PREFERRED;
}

static size_t
queue_claim_one(index_queue_t* queue)
{
    size_t idx = atomic_fetch_add_explicit(
            &queue->cursor, 1, memory_order_relaxed);
    return idx < queue->count ? 1 : 0;
}

static size_t
queue_claim_batch(index_queue_t* queue, size_t batch)
{
    size_t start = atomic_fetch_add_explicit(
            &queue->cursor, batch, memory_order_relaxed);
    if (start >= queue->count) {
        return 0;
    }
    size_t remaining = queue->count - start;
    return remaining < batch ? remaining : batch;
}

static void*
split_worker(void* raw)
{
    split_worker_arg_t* arg = raw;
    size_t claimed = 0;

    if (arg->worker_kind == WORKER_GPU) {
        const size_t batch = 32;
        while (atomic_load_explicit(arg->completed, memory_order_relaxed)
               < arg->task_count)
        {
            size_t got = queue_claim_batch(arg->gpu, batch);
            if (got == 0) {
                got = queue_claim_batch(arg->either, batch);
            }
            if (got == 0) {
                if (atomic_load_explicit(arg->completed, memory_order_relaxed)
                    >= arg->task_count)
                {
                    break;
                }
                continue;
            }
            claimed += got;
            atomic_fetch_add_explicit(
                    arg->completed, got, memory_order_relaxed);
        }
    } else {
        while (atomic_load_explicit(arg->completed, memory_order_relaxed)
               < arg->task_count)
        {
            size_t got = queue_claim_one(arg->cpu);
            if (got == 0) {
                got = queue_claim_one(arg->either);
            }
            if (got == 0) {
                if (atomic_load_explicit(arg->completed, memory_order_relaxed)
                    >= arg->task_count)
                {
                    break;
                }
                continue;
            }
            claimed += got;
            atomic_fetch_add_explicit(
                    arg->completed, got, memory_order_relaxed);
        }
    }

    arg->claimed = claimed;
    return NULL;
}

static void
fill_tasks(
        task_kind_t* kinds,
        size_t count,
        unsigned cpu_pct,
        unsigned gpu_pct)
{
    uint64_t x = UINT64_C(0x9e3779b97f4a7c15);
    for (size_t i = 0; i < count; ++i) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        unsigned v = (unsigned)(x % 100u);
        kinds[i] = v < cpu_pct
                ? TASK_CPU_ONLY
                : (v < cpu_pct + gpu_pct
                        ? TASK_GPU_PREFERRED
                        : TASK_EITHER);
    }
}

typedef struct {
    shared_task_t* tasks;
    size_t task_count;
    atomic_size_t* scan_cursor;
    atomic_size_t* completed;
    worker_kind_t worker_kind;
    size_t claimed;
    size_t incompatible_seen;
} shared_worker_arg_v2_t;

static void*
shared_worker_v2(void* raw)
{
    shared_worker_arg_v2_t* arg = raw;
    size_t claimed = 0;
    size_t incompatible = 0;

    while (atomic_load_explicit(arg->completed, memory_order_relaxed)
           < arg->task_count)
    {
        size_t start = atomic_fetch_add_explicit(
                arg->scan_cursor, 1, memory_order_relaxed) % arg->task_count;
        int found = 0;
        for (size_t probe = 0; probe < arg->task_count; ++probe) {
            size_t idx = (start + probe) % arg->task_count;
            shared_task_t* task = &arg->tasks[idx];
            if (!compatible(arg->worker_kind, task->kind)) {
                ++incompatible;
                continue;
            }
            unsigned char expected = 0;
            if (atomic_compare_exchange_strong_explicit(
                        &task->claimed,
                        &expected,
                        1,
                        memory_order_acq_rel,
                        memory_order_relaxed))
            {
                ++claimed;
                atomic_fetch_add_explicit(
                        arg->completed, 1, memory_order_relaxed);
                found = 1;
                break;
            }
        }
        if (!found
            && atomic_load_explicit(arg->completed, memory_order_relaxed)
                    >= arg->task_count)
        {
            break;
        }
    }

    arg->claimed = claimed;
    arg->incompatible_seen = incompatible;
    return NULL;
}

static double
run_shared_v2(
        const task_kind_t* kinds,
        size_t task_count,
        size_t cpu_workers,
        size_t gpu_workers,
        size_t* incompatible_out)
{
    const size_t worker_count = cpu_workers + gpu_workers;
    shared_task_t* tasks = calloc(task_count, sizeof(shared_task_t));
    pthread_t* threads = calloc(worker_count, sizeof(pthread_t));
    shared_worker_arg_v2_t* args =
            calloc(worker_count, sizeof(shared_worker_arg_v2_t));
    if (!tasks || !threads || !args) {
        exit(2);
    }

    for (size_t i = 0; i < task_count; ++i) {
        tasks[i].kind = kinds[i];
        atomic_init(&tasks[i].claimed, 0);
    }

    atomic_size_t scan_cursor;
    atomic_size_t completed;
    atomic_init(&scan_cursor, 0);
    atomic_init(&completed, 0);

    uint64_t start = now_ns();
    for (size_t i = 0; i < worker_count; ++i) {
        args[i] = (shared_worker_arg_v2_t){
            .tasks = tasks,
            .task_count = task_count,
            .scan_cursor = &scan_cursor,
            .completed = &completed,
            .worker_kind = i < cpu_workers ? WORKER_CPU : WORKER_GPU,
            .claimed = 0,
            .incompatible_seen = 0,
        };
        if (pthread_create(&threads[i], NULL, shared_worker_v2, &args[i]) != 0) {
            exit(3);
        }
    }

    size_t incompatible = 0;
    for (size_t i = 0; i < worker_count; ++i) {
        pthread_join(threads[i], NULL);
        incompatible += args[i].incompatible_seen;
    }
    uint64_t stop = now_ns();

    *incompatible_out = incompatible;
    free(args);
    free(threads);
    free(tasks);
    return (double)(stop - start) / (double)task_count;
}

static split_result_t
run_split(
        const task_kind_t* kinds,
        size_t task_count,
        size_t cpu_workers,
        size_t gpu_workers)
{
    const uint64_t build_start = now_ns();
    size_t* cpu_items = malloc(task_count * sizeof(size_t));
    size_t* gpu_items = malloc(task_count * sizeof(size_t));
    size_t* either_items = malloc(task_count * sizeof(size_t));
    if (!cpu_items || !gpu_items || !either_items) {
        exit(2);
    }

    size_t cpu_count = 0;
    size_t gpu_count = 0;
    size_t either_count = 0;
    for (size_t i = 0; i < task_count; ++i) {
        switch (kinds[i]) {
        case TASK_CPU_ONLY:
            cpu_items[cpu_count++] = i;
            break;
        case TASK_GPU_PREFERRED:
            gpu_items[gpu_count++] = i;
            break;
        case TASK_EITHER:
            either_items[either_count++] = i;
            break;
        }
    }
    const uint64_t build_stop = now_ns();

    index_queue_t cpu = {cpu_items, cpu_count, ATOMIC_VAR_INIT(0)};
    index_queue_t gpu = {gpu_items, gpu_count, ATOMIC_VAR_INIT(0)};
    index_queue_t either = {either_items, either_count, ATOMIC_VAR_INIT(0)};

    atomic_size_t completed;
    atomic_init(&completed, 0);

    const size_t worker_count = cpu_workers + gpu_workers;
    pthread_t* threads = calloc(worker_count, sizeof(pthread_t));
    split_worker_arg_t* args =
            calloc(worker_count, sizeof(split_worker_arg_t));
    if (!threads || !args) {
        exit(2);
    }

    uint64_t start = now_ns();
    for (size_t i = 0; i < worker_count; ++i) {
        args[i] = (split_worker_arg_t){
            .cpu = &cpu,
            .gpu = &gpu,
            .either = &either,
            .completed = &completed,
            .task_count = task_count,
            .worker_kind = i < cpu_workers ? WORKER_CPU : WORKER_GPU,
            .claimed = 0,
        };
        if (pthread_create(&threads[i], NULL, split_worker, &args[i]) != 0) {
            exit(3);
        }
    }

    for (size_t i = 0; i < worker_count; ++i) {
        pthread_join(threads[i], NULL);
    }
    uint64_t stop = now_ns();

    free(args);
    free(threads);
    free(either_items);
    free(gpu_items);
    free(cpu_items);

    return (split_result_t){
        .build_ns_per_task =
                (double)(build_stop - build_start) / (double)task_count,
        .claim_ns_per_task =
                (double)(stop - start) / (double)task_count,
    };
}

static void
run_case(
        const char* label,
        size_t task_count,
        size_t cpu_workers,
        size_t gpu_workers,
        unsigned cpu_pct,
        unsigned gpu_pct)
{
    task_kind_t* kinds = malloc(task_count * sizeof(task_kind_t));
    if (!kinds) {
        exit(2);
    }
    fill_tasks(kinds, task_count, cpu_pct, gpu_pct);

    size_t incompatible = 0;
    const double shared_ns = run_shared_v2(
            kinds,
            task_count,
            cpu_workers,
            gpu_workers,
            &incompatible);
    const split_result_t split = run_split(
            kinds,
            task_count,
            cpu_workers,
            gpu_workers);
    const double split_total_ns =
            split.build_ns_per_task + split.claim_ns_per_task;

    printf(
            "pool_case=%s tasks=%zu cpu_workers=%zu gpu_workers=%zu "
            "shared_ns=%.4f split3_build_ns=%.4f split3_claim_ns=%.4f "
            "split3_total_ns=%.4f speedup_claim_only=%.4fx "
            "speedup_including_build=%.4fx "
            "shared_incompatible_scans=%zu\n",
            label,
            task_count,
            cpu_workers,
            gpu_workers,
            shared_ns,
            split.build_ns_per_task,
            split.claim_ns_per_task,
            split_total_ns,
            shared_ns / split.claim_ns_per_task,
            shared_ns / split_total_ns,
            incompatible);

    free(kinds);
}

int
main(void)
{
    const size_t task_count = 200000;

    /*
     * Best-case control: every task can run on either backend, so a single
     * atomic cursor is sufficient and no compatibility filtering is needed.
     */
    for (size_t cpu_workers = 1; cpu_workers <= 8; cpu_workers *= 2) {
        const double any_ns = run_shared_any(task_count, cpu_workers, 1);
        printf(
                "pool_any tasks=%zu cpu_workers=%zu gpu_workers=1 "
                "shared_atomic_ns=%.4f\n",
                task_count,
                cpu_workers,
                any_ns);
    }

    /* CPU-only 20%, GPU-preferred 20%, either 60%. */
    run_case("balanced", task_count, 4, 1, 20, 20);

    /* GPU-heavy eligibility. */
    run_case("gpu_heavy", task_count, 4, 1, 10, 60);

    /* CPU-heavy eligibility. */
    run_case("cpu_heavy", task_count, 4, 1, 60, 10);

    /* More CPU workers to expose queue contention. */
    run_case("balanced_8cpu", task_count, 8, 1, 20, 20);

    return 0;
}
