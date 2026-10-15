// Minimal native memory-locality workload. Reports per-process wall and CPU time.
// Usage: ./server_locality_native <elements> <reps>
// CPU affinity supplied by the parent launcher, no privileged operations.
#define _GNU_SOURCE
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <sys/resource.h>
#include <string.h>

typedef struct { const uint64_t *data; size_t len; size_t start; uint64_t sum; } Job;
static uint64_t now_ns(clockid_t clock) {
    struct timespec ts;
    clock_gettime(clock, &ts);
    return ((uint64_t)ts.tv_sec * 1000000000ULL) + ts.tv_nsec;
}
static void *run(void *ptr) {
    Job *j = ptr;
    uint64_t sum = 0;
    // Each thread touches its own contiguous half. Cache / NUMA experiments
    // must choose a footprint large enough to dominate thread-start overhead.
    for (size_t i = j->start; i < j->start + j->len; i += 8) {
        sum += j->data[i];
    }
    j->sum = sum;
    return NULL;
}
int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: %s elements repetitions\n", argv[0]); return 2; }
    size_t n = strtoull(argv[1], NULL, 10);
    int reps = atoi(argv[2]);
    if (n < 1024 || n % 16 != 0 || n > (1ULL << 27) || reps < 5 || reps > 200) return 2;
    uint64_t *data = NULL;
    if (posix_memalign((void**)&data, 64, n * sizeof(uint64_t))) return 2;
    for (size_t i = 0; i < n; i++) data[i] = (uint64_t)(i % 101);
    uint64_t expected = 0;
    for (size_t i = 0; i < n / 2; i += 8) expected += data[i];
    for (size_t i = n / 2; i < n; i += 8) expected += data[i];
    uint64_t *wall = calloc(reps, sizeof(*wall));
    uint64_t *cpu = calloc(reps, sizeof(*cpu));
    if (!wall || !cpu) return 2;
    for (int r = 0; r < reps + 3; r++) {
        pthread_t t[2];
        Job jobs[2] = { {data, n / 2, 0, 0}, {data, n / 2, n / 2, 0} };
        uint64_t c0 = now_ns(CLOCK_PROCESS_CPUTIME_ID), w0 = now_ns(CLOCK_MONOTONIC);
        if (pthread_create(&t[0], NULL, run, &jobs[0]) ||
            pthread_create(&t[1], NULL, run, &jobs[1])) return 3;
        pthread_join(t[0], NULL);
        pthread_join(t[1], NULL);
        uint64_t wd = now_ns(CLOCK_MONOTONIC) - w0;
        uint64_t cd = now_ns(CLOCK_PROCESS_CPUTIME_ID) - c0;
        if (jobs[0].sum + jobs[1].sum != expected) {
            fprintf(stderr, "checksum mismatch\n"); return 4;
        }
        if (r >= 3) { wall[r-3] = wd; cpu[r-3] = cd; }
    }
    printf("{\"elements\":%zu,\"repetitions\":%d,\"checksum\":%llu,\"wall_ns\":[",
           n,reps,(unsigned long long)expected);
    for (int i = 0; i < reps; i++) printf("%s%llu", i?",":"",(unsigned long long)wall[i]);
    printf("],\"cpu_ns\":[");
    for (int i = 0; i < reps; i++) printf("%s%llu", i?",":"",(unsigned long long)cpu[i]);
    puts("]}");
    free(cpu); free(wall); free(data);
    return 0;
}
