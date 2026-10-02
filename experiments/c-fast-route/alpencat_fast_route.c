#include "alpencat_fast_route.h"

#if defined(_MSC_VER)
#define AC_NOINLINE __declspec(noinline)
#elif defined(__GNUC__) || defined(__clang__)
#define AC_NOINLINE __attribute__((noinline))
#else
#define AC_NOINLINE
#endif

static unsigned
log2_bucket(size_t value)
{
#if defined(__GNUC__) || defined(__clang__)
    if (value == 0) {
        return 0;
    }
#  if SIZE_MAX == UINT64_MAX
    return (unsigned)(63u - (unsigned)__builtin_clzll((unsigned long long)value));
#  else
    return (unsigned)(31u - (unsigned)__builtin_clzl((unsigned long)value));
#  endif
#else
    unsigned bucket = 0;
    while (value > 1) {
        value >>= 1;
        ++bucket;
    }
    return bucket;
#endif
}

AC_NOINLINE ac_route_t
alpencat_fast_route(
        const ac_fast_policy_t* policy,
        size_t work_items,
        uint32_t state)
{
    if (state & AC_STATE_NESTED_PARALLEL) {
        return AC_ROUTE_SERIAL;
    }

    if (work_items <= policy->serial_max) {
        return AC_ROUTE_SERIAL;
    }

    if ((state & AC_CAP_GPU)
        && (state & policy->required_gpu_state) == policy->required_gpu_state
        && work_items >= policy->gpu_min)
    {
        return AC_ROUTE_GPU;
    }

    if (state & AC_CAP_CPU) {
        return AC_ROUTE_CPU;
    }

    return AC_ROUTE_ADAPTIVE;
}

AC_NOINLINE ac_route_t
alpencat_fast_route_constant(void)
{
    return AC_ROUTE_CPU;
}

AC_NOINLINE ac_route_t
alpencat_fast_route_cached(const uint8_t* route_by_log2_size, size_t work_items)
{
    unsigned bucket = log2_bucket(work_items);
    if (bucket >= sizeof(size_t) * 8u) {
        return AC_ROUTE_ADAPTIVE;
    }
    return (ac_route_t)route_by_log2_size[bucket];
}
