#include "alpencat_fast_route.h"

#include <assert.h>
#include <stdint.h>

int
main(void)
{
    ac_fast_policy_t policy = {
        .serial_max = 1024,
        .gpu_min = 262144,
        .required_gpu_state = 0,
    };

    assert(alpencat_fast_route(&policy, 1, AC_CAP_CPU | AC_CAP_GPU) == AC_ROUTE_SERIAL);
    assert(alpencat_fast_route(&policy, 1024, AC_CAP_CPU | AC_CAP_GPU) == AC_ROUTE_SERIAL);
    assert(alpencat_fast_route(&policy, 1025, AC_CAP_CPU) == AC_ROUTE_CPU);
    assert(alpencat_fast_route(&policy, 262143, AC_CAP_CPU | AC_CAP_GPU) == AC_ROUTE_CPU);
    assert(alpencat_fast_route(&policy, 262144, AC_CAP_CPU | AC_CAP_GPU) == AC_ROUTE_GPU);
    assert(
            alpencat_fast_route(
                    &policy,
                    1048576,
                    AC_CAP_CPU | AC_CAP_GPU | AC_STATE_NESTED_PARALLEL)
            == AC_ROUTE_SERIAL);
    assert(alpencat_fast_route(&policy, 4096, 0) == AC_ROUTE_ADAPTIVE);

    uint8_t table[sizeof(size_t) * 8u];
    for (size_t i = 0; i < sizeof(table); ++i) {
        table[i] = AC_ROUTE_ADAPTIVE;
    }
    table[10] = AC_ROUTE_SERIAL;
    table[11] = AC_ROUTE_CPU;
    table[18] = AC_ROUTE_GPU;

    assert(alpencat_fast_route_cached(table, 1024) == AC_ROUTE_SERIAL);
    assert(alpencat_fast_route_cached(table, 2048) == AC_ROUTE_CPU);
    assert(alpencat_fast_route_cached(table, 262144) == AC_ROUTE_GPU);

    ac_exact_route_entry_t exact[16] = {0};
    alpencat_fast_route_exact_publish_probe8(
            exact, 16, 7, 4096, AC_ROUTE_CPU);
    assert(
            alpencat_fast_route_exact_cached_probe8(exact, 16, 7, 4096)
            == AC_ROUTE_CPU);
    assert(
            alpencat_fast_route_exact_cached_probe8(exact, 16, 7, 8192)
            == AC_ROUTE_ADAPTIVE);
    return 0;
}
