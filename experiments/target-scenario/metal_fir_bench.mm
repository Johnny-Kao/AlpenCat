#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <vector>
#include <cerrno>
#include <sys/mman.h>

static const char *kShader = R"METAL(
#include <metal_stdlib>
using namespace metal;

kernel void fir_f32(
    const device float *x [[buffer(0)]],
    const device float *h [[buffer(1)]],
    device float *y [[buffer(2)]],
    constant uint &n [[buffer(3)]],
    constant uint &taps [[buffer(4)]],
    uint gid [[thread_position_in_grid]])
{
    if (gid >= n) return;
    float acc = 0.0f;
    for (uint j = 0; j < taps; ++j) {
        acc = fma(x[gid + j], h[j], acc);
    }
    y[gid] = acc;
}
)METAL";

__attribute__((noinline))
static void fir_cpu(const float *x, const float *h, float *y, size_t n, size_t taps) {
    for (size_t i = 0; i < n; ++i) {
        float acc = 0.0f;
        for (size_t j = 0; j < taps; ++j) {
            acc += x[i + j] * h[j];
        }
        y[i] = acc;
    }
}

static double median(std::vector<double> values) {
    std::sort(values.begin(), values.end());
    return values[values.size() / 2];
}

static size_t iterations_for(size_t n, size_t taps, bool focus) {
    const size_t target_ops = focus ? 16'000'000 : 8'000'000;
    size_t iters = target_ops / std::max<size_t>(1, n * taps);
    const size_t min_iters = focus ? 15 : 5;
    const size_t max_iters = focus ? 80 : 100;
    return std::max<size_t>(min_iters, std::min<size_t>(max_iters, iters));
}

static double run_cpu(
    const float *x,
    const float *h,
    float *y,
    size_t n,
    size_t taps)
{
    auto start = std::chrono::steady_clock::now();
    fir_cpu(x, h, y, n, taps);
    auto end = std::chrono::steady_clock::now();
    return std::chrono::duration<double, std::nano>(end - start).count();
}

struct GpuSample {
    double host_ns;
    double device_ns;
};

static GpuSample run_gpu(
    id<MTLCommandQueue> queue,
    id<MTLComputePipelineState> pipeline,
    id<MTLBuffer> x,
    id<MTLBuffer> h,
    id<MTLBuffer> y,
    uint32_t n,
    uint32_t taps)
{
    auto start = std::chrono::steady_clock::now();

    id<MTLCommandBuffer> cb = [queue commandBuffer];
    id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
    [enc setComputePipelineState:pipeline];
    [enc setBuffer:x offset:0 atIndex:0];
    [enc setBuffer:h offset:0 atIndex:1];
    [enc setBuffer:y offset:0 atIndex:2];
    [enc setBytes:&n length:sizeof(n) atIndex:3];
    [enc setBytes:&taps length:sizeof(taps) atIndex:4];

    NSUInteger width = std::min<NSUInteger>(256, pipeline.maxTotalThreadsPerThreadgroup);
    [enc dispatchThreads:MTLSizeMake(n, 1, 1)
      threadsPerThreadgroup:MTLSizeMake(width, 1, 1)];
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];

    auto end = std::chrono::steady_clock::now();
    double host_ns =
        std::chrono::duration<double, std::nano>(end - start).count();

    double device_ns = 0.0;
    if (cb.GPUEndTime > cb.GPUStartTime && cb.GPUStartTime > 0.0) {
        device_ns = (cb.GPUEndTime - cb.GPUStartTime) * 1e9;
    }

    return {host_ns, device_ns};
}

int main() {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            std::fprintf(stderr, "metal_device=NONE\n");
            return 2;
        }

        NSError *error = nil;
        NSString *source = [NSString stringWithUTF8String:kShader];
        id<MTLLibrary> library = [device newLibraryWithSource:source options:nil error:&error];
        if (!library) {
            std::fprintf(stderr, "metal_library_error=%s\n",
                         error.localizedDescription.UTF8String);
            return 3;
        }

        id<MTLFunction> function = [library newFunctionWithName:@"fir_f32"];
        id<MTLComputePipelineState> pipeline =
            [device newComputePipelineStateWithFunction:function error:&error];
        if (!pipeline) {
            std::fprintf(stderr, "metal_pipeline_error=%s\n",
                         error.localizedDescription.UTF8String);
            return 4;
        }

        id<MTLCommandQueue> queue = [device newCommandQueue];
        if (!queue) {
            std::fprintf(stderr, "metal_queue=NONE\n");
            return 5;
        }

        const size_t max_n = 262144;
        const size_t max_taps = 128;
        const size_t x_count = max_n + max_taps;

        id<MTLBuffer> x_buf =
            [device newBufferWithLength:x_count * sizeof(float)
                                options:MTLResourceStorageModeShared];
        id<MTLBuffer> h_buf =
            [device newBufferWithLength:max_taps * sizeof(float)
                                options:MTLResourceStorageModeShared];
        id<MTLBuffer> y_gpu_buf =
            [device newBufferWithLength:max_n * sizeof(float)
                                options:MTLResourceStorageModeShared];

        if (!x_buf || !h_buf || !y_gpu_buf) {
            std::fprintf(stderr, "buffer_allocation_failed=1\n");
            return 6;
        }

        float *x = static_cast<float *>(x_buf.contents);
        float *h = static_cast<float *>(h_buf.contents);
        float *y_gpu = static_cast<float *>(y_gpu_buf.contents);
        std::vector<float> y_cpu(max_n);

        std::memset(y_gpu_buf.contents, 0, y_gpu_buf.length);
        std::fill(y_cpu.begin(), y_cpu.end(), 0.0f);

        const int lock_x = mlock(x_buf.contents, x_buf.length);
        const int lock_h = mlock(h_buf.contents, h_buf.length);
        const int lock_y = mlock(y_gpu_buf.contents, y_gpu_buf.length);

        const bool focus = std::getenv("ALPENCAT_FOCUS") != nullptr;

        for (size_t i = 0; i < x_count; ++i) {
            const int centered = static_cast<int>(i % 97) - 48;
            x[i] = static_cast<float>(centered) * 0.001f;
        }
        for (size_t j = 0; j < max_taps; ++j) {
            h[j] = 1.0f / static_cast<float>(j + 1);
        }

        std::printf("# AlpenCat target-scenario real Metal FIR benchmark\n");
        std::printf("device=%s\n", device.name.UTF8String);
        std::printf("unified_memory=%s\n", device.hasUnifiedMemory ? "true" : "false");
        std::printf("storage_mode=shared\n");
        std::printf("gpu_measurement=host_submit_to_completion_and_device_interval\n");
        std::printf("focus_mode=%s\n", focus ? "true" : "false");
        std::printf("x_addr=%p h_addr=%p y_gpu_addr=%p y_cpu_addr=%p\n",
                    x_buf.contents, h_buf.contents, y_gpu_buf.contents, y_cpu.data());
        std::printf("mlock_x=%d mlock_h=%d mlock_y=%d errno=%d\n",
                    lock_x, lock_h, lock_y, errno);
        std::printf("| taps | n | iters | cpu_p50_ns | gpu_host_p50_ns | gpu_device_p50_ns | winner | winner_gain_pct |\n");
        std::printf("|---:|---:|---:|---:|---:|---:|---|---:|\n");

        for (size_t taps : {16ul, 64ul, 128ul}) {
            std::vector<size_t> sizes;
            if (focus) {
                if (taps == 16) {
                    sizes = {65536ul, 131072ul, 262144ul};
                } else if (taps == 64) {
                    sizes = {32768ul, 65536ul, 131072ul};
                } else {
                    sizes = {8192ul, 16384ul, 32768ul};
                }
            } else {
                sizes = {
                    64ul, 128ul, 256ul, 512ul, 1024ul, 2048ul,
                    4096ul, 8192ul, 16384ul, 32768ul, 65536ul,
                    131072ul, 262144ul
                };
            }
            for (size_t n : sizes) {

                // Correctness check outside timed samples.
                fir_cpu(x, h, y_cpu.data(), n, taps);
                (void)run_gpu(queue, pipeline, x_buf, h_buf, y_gpu_buf,
                              static_cast<uint32_t>(n),
                              static_cast<uint32_t>(taps));

                double max_abs = 0.0;
                for (size_t i = 0; i < n; ++i) {
                    max_abs = std::max(
                        max_abs,
                        std::abs(static_cast<double>(y_cpu[i]) -
                                 static_cast<double>(y_gpu[i])));
                }
                if (max_abs > 1e-3) {
                    std::fprintf(stderr,
                        "correctness_failed taps=%zu n=%zu max_abs=%g\n",
                        taps, n, max_abs);
                    return 7;
                }

                for (int i = 0; i < 3; ++i) {
                    (void)run_cpu(x, h, y_cpu.data(), n, taps);
                    (void)run_gpu(queue, pipeline, x_buf, h_buf, y_gpu_buf,
                                  static_cast<uint32_t>(n),
                                  static_cast<uint32_t>(taps));
                }

                const size_t iters = iterations_for(n, taps, focus);
                std::vector<double> cpu_samples;
                std::vector<double> gpu_host_samples;
                std::vector<double> gpu_device_samples;
                cpu_samples.reserve(iters);
                gpu_host_samples.reserve(iters);
                gpu_device_samples.reserve(iters);

                for (size_t i = 0; i < iters; ++i) {
                    if ((i & 1u) == 0u) {
                        cpu_samples.push_back(
                            run_cpu(x, h, y_cpu.data(), n, taps));
                        GpuSample g = run_gpu(
                            queue, pipeline, x_buf, h_buf, y_gpu_buf,
                            static_cast<uint32_t>(n),
                            static_cast<uint32_t>(taps));
                        gpu_host_samples.push_back(g.host_ns);
                        if (g.device_ns > 0.0) gpu_device_samples.push_back(g.device_ns);
                    } else {
                        GpuSample g = run_gpu(
                            queue, pipeline, x_buf, h_buf, y_gpu_buf,
                            static_cast<uint32_t>(n),
                            static_cast<uint32_t>(taps));
                        gpu_host_samples.push_back(g.host_ns);
                        if (g.device_ns > 0.0) gpu_device_samples.push_back(g.device_ns);
                        cpu_samples.push_back(
                            run_cpu(x, h, y_cpu.data(), n, taps));
                    }
                }

                const double cpu_ns = median(cpu_samples);
                const double gpu_host_ns = median(gpu_host_samples);
                const double gpu_device_ns =
                    gpu_device_samples.empty() ? 0.0 : median(gpu_device_samples);

                const bool cpu_wins = cpu_ns <= gpu_host_ns;
                const double best = std::min(cpu_ns, gpu_host_ns);
                const double worst = std::max(cpu_ns, gpu_host_ns);
                const double gain_pct = 100.0 * (worst - best) / worst;

                std::printf(
                    "| %zu | %zu | %zu | %.1f | %.1f | %.1f | %s | %.2f%% |\n",
                    taps, n, iters, cpu_ns, gpu_host_ns, gpu_device_ns,
                    cpu_wins ? "CPU" : "GPU", gain_pct);
                std::printf(
                    "fir_result taps=%zu n=%zu iters=%zu cpu_ns=%.3f "
                    "gpu_host_ns=%.3f gpu_device_ns=%.3f winner=%s "
                    "winner_gain_pct=%.4f max_abs=%g\n",
                    taps, n, iters, cpu_ns, gpu_host_ns, gpu_device_ns,
                    cpu_wins ? "CPU" : "GPU", gain_pct, max_abs);
            }
        }
    }
    return 0;
}
