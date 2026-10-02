#ifndef ALPENCAT_NATIVE_KERNEL_H
#define ALPENCAT_NATIVE_KERNEL_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void alpencat_axpy_f32(
        size_t n,
        float a,
        const float* x,
        float* y);

#ifdef __cplusplus
}
#endif

#endif
