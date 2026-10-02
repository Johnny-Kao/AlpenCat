#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <complex>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sys/mman.h>
#include <vector>

static const char *kShader = R"METAL(
#include <metal_stdlib>
using namespace metal;

kernel void bit_reverse(
    const device float2 *input [[buffer(0)]],
    device float2 *output [[buffer(1)]],
    constant uint &n [[buffer(2)]],
    constant uint &bits [[buffer(3)]],
    uint gid [[thread_position_in_grid]])
{
    if (gid >= n) return;
    uint x = gid;
    uint r = 0;
    for (uint i = 0; i < bits; ++i) {
        r = (r << 1) | (x & 1u);
        x >>= 1;
    }
    output[r] = input[gid];
}

kernel void fft_stage(
    const device float2 *src [[buffer(0)]],
    device float2 *dst [[buffer(1)]],
    constant uint &n [[buffer(2)]],
    constant uint &len [[buffer(3)]],
    uint gid [[thread_position_in_grid]])
{
    uint half = len >> 1;
    uint butterflies = n >> 1;
    if (gid >= butterflies) return;

    uint group = gid / half;
    uint j = gid - group * half;
    uint i0 = group * len + j;
    uint i1 = i0 + half;

    float angle = -2.0f * M_PI_F * float(j) / float(len);
    float2 w = float2(cos(angle), sin(angle));
    float2 b = src[i1];
    float2 t = float2(
        b.x * w.x - b.y * w.y,
        b.x * w.y + b.y * w.x
    );
    float2 a = src[i0];
    dst[i0] = a + t;
    dst[i1] = a - t;
}
)METAL";

static double median(std::vector<double> v) {
    std::sort(v.begin(), v.end());
    return v[v.size() / 2];
}

static unsigned ilog2_pow2(size_t n) {
    unsigned bits = 0;
    while ((size_t(1) << bits) < n) ++bits;
    return bits;
}

static size_t bit_reverse_index(size_t x, unsigned bits) {
    size_t r = 0;
    for (unsigned i = 0; i < bits; ++i) {
        r = (r << 1) | (x & 1u);
        x >>= 1;
    }
    return r;
}

__attribute__((noinline))
static void fft_cpu(const std::complex<float> *in, std::complex<float> *out, size_t n) {
    unsigned bits = ilog2_pow2(n);
    for (size_t i = 0; i < n; ++i) out[bit_reverse_index(i, bits)] = in[i];

    for (size_t len = 2; len <= n; len <<= 1) {
        const size_t half = len >> 1;
        const float scale = -2.0f * float(M_PI) / float(len);
        for (size_t base = 0; base < n; base += len) {
            for (size_t j = 0; j < half; ++j) {
                float angle = scale * float(j);
                std::complex<float> w(std::cos(angle), std::sin(angle));
                auto a = out[base + j];
                auto b = out[base + j + half] * w;
                out[base + j] = a + b;
                out[base + j + half] = a - b;
            }
        }
    }
}

struct GpuSample {
    double host_ns;
    double device_ns;
    id<MTLBuffer> output;
};

static GpuSample fft_gpu(
    id<MTLCommandQueue> q,
    id<MTLComputePipelineState> bitrev,
    id<MTLComputePipelineState> stage,
    id<MTLBuffer> input,
    id<MTLBuffer> a,
    id<MTLBuffer> b,
    uint32_t n)
{
    uint32_t bits = ilog2_pow2(n);
    auto begin = std::chrono::steady_clock::now();

    id<MTLCommandBuffer> cb = [q commandBuffer];

    {
        id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
        [enc setComputePipelineState:bitrev];
        [enc setBuffer:input offset:0 atIndex:0];
        [enc setBuffer:a offset:0 atIndex:1];
        [enc setBytes:&n length:sizeof(n) atIndex:2];
        [enc setBytes:&bits length:sizeof(bits) atIndex:3];
        NSUInteger w = std::min<NSUInteger>(256, bitrev.maxTotalThreadsPerThreadgroup);
        [enc dispatchThreads:MTLSizeMake(n,1,1)
          threadsPerThreadgroup:MTLSizeMake(w,1,1)];
        [enc endEncoding];
    }

    id<MTLBuffer> src = a;
    id<MTLBuffer> dst = b;
    for (uint32_t len = 2; len <= n; len <<= 1) {
        id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
        [enc setComputePipelineState:stage];
        [enc setBuffer:src offset:0 atIndex:0];
        [enc setBuffer:dst offset:0 atIndex:1];
        [enc setBytes:&n length:sizeof(n) atIndex:2];
        [enc setBytes:&len length:sizeof(len) atIndex:3];
        NSUInteger threads = n / 2;
        NSUInteger w = std::min<NSUInteger>(256, stage.maxTotalThreadsPerThreadgroup);
        [enc dispatchThreads:MTLSizeMake(threads,1,1)
          threadsPerThreadgroup:MTLSizeMake(w,1,1)];
        [enc endEncoding];
        id<MTLBuffer> tmp = src;
        src = dst;
        dst = tmp;
    }

    [cb commit];
    [cb waitUntilCompleted];
    auto end = std::chrono::steady_clock::now();

    double device_ns = 0.0;
    if (cb.GPUEndTime > cb.GPUStartTime && cb.GPUStartTime > 0.0) {
        device_ns = (cb.GPUEndTime - cb.GPUStartTime) * 1e9;
    }

    return {
        std::chrono::duration<double, std::nano>(end - begin).count(),
        device_ns,
        src
    };
}

int main() {
    @autoreleasepool {
        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        if (!dev) return 2;
        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithSource:[NSString stringWithUTF8String:kShader]
                                               options:nil error:&err];
        if (!lib) {
            std::fprintf(stderr, "fft_library_error=%s\n", err.localizedDescription.UTF8String);
            return 3;
        }
        id<MTLComputePipelineState> bitrev =
            [dev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"bit_reverse"]
                                               error:&err];
        id<MTLComputePipelineState> stage =
            [dev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"fft_stage"]
                                               error:&err];
        if (!bitrev || !stage) return 4;
        id<MTLCommandQueue> q = [dev newCommandQueue];

        const size_t max_n = 65536;
        const size_t bytes = max_n * sizeof(std::complex<float>);
        id<MTLBuffer> in_buf = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        id<MTLBuffer> a_buf = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        id<MTLBuffer> b_buf = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        if (!in_buf || !a_buf || !b_buf) return 5;

        auto *in = static_cast<std::complex<float> *>(in_buf.contents);
        std::vector<std::complex<float>> cpu(max_n);
        for (size_t i = 0; i < max_n; ++i) {
            float t = float(i % 1024) / 1024.0f;
            in[i] = {std::sin(6.28318530718f * t) + 0.1f * std::cos(31.0f * t),
                     0.05f * std::sin(11.0f * t)};
        }
        (void)mlock(in_buf.contents, in_buf.length);
        (void)mlock(a_buf.contents, a_buf.length);
        (void)mlock(b_buf.contents, b_buf.length);

        std::printf("# Metal FFT benchmark\n");
        std::printf("device=%s unified_memory=%s\n", dev.name.UTF8String,
                    dev.hasUnifiedMemory ? "true" : "false");
        std::printf("| n | reps | cpu_p50_ns | gpu_p50_ns | gpu_device_p50_ns | winner |\n");
        std::printf("|---:|---:|---:|---:|---:|---|\n");

        for (size_t n : {256ul, 512ul, 1024ul, 2048ul, 4096ul, 8192ul, 16384ul, 32768ul, 65536ul}) {
            fft_cpu(in, cpu.data(), n);
            GpuSample warm = fft_gpu(q, bitrev, stage, in_buf, a_buf, b_buf, (uint32_t)n);
            auto *gpu_out = static_cast<std::complex<float> *>(warm.output.contents);
            double max_err = 0.0;
            for (size_t i = 0; i < n; ++i) {
                max_err = std::max(max_err, double(std::abs(cpu[i] - gpu_out[i])));
            }
            double tolerance = 2e-3 * std::sqrt(double(n));
            if (max_err > tolerance) {
                std::fprintf(stderr, "fft_correctness_failed n=%zu max_err=%g tol=%g\n",
                             n, max_err, tolerance);
                return 6;
            }

            for (int i = 0; i < 3; ++i) {
                fft_cpu(in, cpu.data(), n);
                (void)fft_gpu(q, bitrev, stage, in_buf, a_buf, b_buf, (uint32_t)n);
            }

            size_t reps = n <= 4096 ? 40 : (n <= 16384 ? 20 : 10);
            std::vector<double> cpu_ns, gpu_ns, gpu_dev_ns;
            for (size_t r = 0; r < reps; ++r) {
                if ((r & 1u) == 0) {
                    auto t0 = std::chrono::steady_clock::now();
                    fft_cpu(in, cpu.data(), n);
                    auto t1 = std::chrono::steady_clock::now();
                    cpu_ns.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                    auto g = fft_gpu(q, bitrev, stage, in_buf, a_buf, b_buf, (uint32_t)n);
                    gpu_ns.push_back(g.host_ns);
                    if (g.device_ns > 0) gpu_dev_ns.push_back(g.device_ns);
                } else {
                    auto g = fft_gpu(q, bitrev, stage, in_buf, a_buf, b_buf, (uint32_t)n);
                    gpu_ns.push_back(g.host_ns);
                    if (g.device_ns > 0) gpu_dev_ns.push_back(g.device_ns);
                    auto t0 = std::chrono::steady_clock::now();
                    fft_cpu(in, cpu.data(), n);
                    auto t1 = std::chrono::steady_clock::now();
                    cpu_ns.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                }
            }

            double c = median(cpu_ns);
            double g = median(gpu_ns);
            double gd = gpu_dev_ns.empty() ? 0.0 : median(gpu_dev_ns);
            std::printf("| %zu | %zu | %.1f | %.1f | %.1f | %s |\n",
                        n, reps, c, g, gd, c <= g ? "CPU" : "GPU");
            std::printf("fft_result n=%zu reps=%zu cpu_ns=%.3f gpu_host_ns=%.3f "
                        "gpu_device_ns=%.3f winner=%s max_err=%g\n",
                        n, reps, c, g, gd, c <= g ? "CPU" : "GPU", max_err);
        }
    }
    return 0;
}
