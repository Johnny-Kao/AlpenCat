#include <stdio.h>
#include <stdint.h>

#if defined(__x86_64__) || defined(__i386__)
#include <cpuid.h>
#endif

int main(void) {
#if defined(__x86_64__) || defined(__i386__)
    unsigned int eax = 0, ebx = 0, ecx = 0, edx = 0;
    char vendor[13] = {0};

    if (!__get_cpuid(0, &eax, &ebx, &ecx, &edx)) {
        fprintf(stderr, "cpuid leaf 0 unavailable\n");
        return 2;
    }

    *(uint32_t *)&vendor[0] = ebx;
    *(uint32_t *)&vendor[4] = edx;
    *(uint32_t *)&vendor[8] = ecx;

    printf("vendor=%s\n", vendor);

    if (!__get_cpuid_count(6, 0, &eax, &ebx, &ecx, &edx)) {
        printf("leaf6_available=0\n");
        printf("hfi_supported=0\n");
        return 0;
    }

    printf("leaf6_available=1\n");
    printf("leaf6_eax=0x%08x\n", eax);
    printf("hfi_bit19=%u\n", (eax >> 19) & 1u);
    printf("hfi_supported=%u\n", (eax >> 19) & 1u);
    return 0;
#else
    printf("vendor=non-x86\n");
    printf("leaf6_available=0\n");
    printf("hfi_supported=0\n");
    return 0;
#endif
}
