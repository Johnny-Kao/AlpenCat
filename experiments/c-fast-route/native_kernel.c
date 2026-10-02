#include "native_kernel.h"

#if defined(_MSC_VER)
#define AC_NOINLINE __declspec(noinline)
#elif defined(__GNUC__) || defined(__clang__)
#define AC_NOINLINE __attribute__((noinline))
#else
#define AC_NOINLINE
#endif

AC_NOINLINE void
alpencat_axpy_f32(size_t n, float a, const float* x, float* y)
{
    for (size_t i = 0; i < n; ++i) {
        y[i] = a * x[i] + y[i];
    }
}
