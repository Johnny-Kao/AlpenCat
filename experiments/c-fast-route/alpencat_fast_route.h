#ifndef ALPENCAT_FAST_ROUTE_H
#define ALPENCAT_FAST_ROUTE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    AC_ROUTE_SERIAL = 0,
    AC_ROUTE_CPU = 1,
    AC_ROUTE_GPU = 2,
    AC_ROUTE_ADAPTIVE = 3
} ac_route_t;

enum {
    AC_CAP_CPU = 1u << 0,
    AC_CAP_GPU = 1u << 1,
    AC_STATE_NESTED_PARALLEL = 1u << 2,
    AC_STATE_GPU_RESIDENT = 1u << 3
};

typedef struct {
    size_t serial_max;
    size_t gpu_min;
    uint32_t required_gpu_state;
} ac_fast_policy_t;

typedef struct {
    uint64_t key;
    uint8_t route;
} ac_exact_route_entry_t;

/*
 * L0 threshold route.
 *
 * This is deliberately small: it answers only cases for which a published
 * policy is decisive. AC_ROUTE_ADAPTIVE means "leave the fast path and ask the
 * slower control plane".
 */
ac_route_t alpencat_fast_route(
        const ac_fast_policy_t* policy,
        size_t work_items,
        uint32_t state);

/*
 * C ABI call-overhead control. It always returns CPU.
 */
ac_route_t alpencat_fast_route_constant(void);

/*
 * Cached route lookup using an exact pre-published size bucket.
 * A table value of AC_ROUTE_ADAPTIVE means the cache intentionally misses.
 */
ac_route_t alpencat_fast_route_cached(
        const uint8_t* route_by_log2_size,
        size_t work_items);

/*
 * Exact task+size cache lookup. The table is read-only on the hot path;
 * a slower control plane may publish/replace entries between epochs.
 *
 * table_size must be a power of two.
 */
ac_route_t alpencat_fast_route_exact_cached(
        const ac_exact_route_entry_t* table,
        size_t table_size,
        uint32_t task_class,
        size_t work_items);

void alpencat_fast_route_exact_publish(
        ac_exact_route_entry_t* table,
        size_t table_size,
        uint32_t task_class,
        size_t work_items,
        ac_route_t route);

/*
 * Bounded open-addressed exact cache. The hot path probes at most eight
 * consecutive slots. This trades a few extra comparisons for materially
 * better collision tolerance than the single-slot cache.
 */
ac_route_t alpencat_fast_route_exact_cached_probe8(
        const ac_exact_route_entry_t* table,
        size_t table_size,
        uint32_t task_class,
        size_t work_items);

void alpencat_fast_route_exact_publish_probe8(
        ac_exact_route_entry_t* table,
        size_t table_size,
        uint32_t task_class,
        size_t work_items,
        ac_route_t route);

/*
 * Epoch-scoped variants. policy_epoch is part of the exact key so a new
 * machine/policy generation cannot accidentally reuse a route learned under an
 * older environment.
 */
ac_route_t alpencat_fast_route_epoch_cached_probe8(
        const ac_exact_route_entry_t* table,
        size_t table_size,
        uint64_t policy_epoch,
        uint32_t task_class,
        size_t work_items);

void alpencat_fast_route_epoch_publish_probe8(
        ac_exact_route_entry_t* table,
        size_t table_size,
        uint64_t policy_epoch,
        uint32_t task_class,
        size_t work_items,
        ac_route_t route);

#ifdef __cplusplus
}
#endif

#endif
