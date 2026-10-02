#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <sys/mman.h>
#include <vector>

static const char *kShader = R"METAL(
#include <metal_stdlib>
using namespace metal;

kernel void reduce_sum(
    const device float *input [[buffer(0)]],
    device float *output [[buffer(1)]],
    constant uint &n [[buffer(2)]],
    uint tid [[thread_index_in_threadgroup]],
    uint3 tg [[threadgroup_position_in_grid]])
{
    threadgroup float scratch[256];
    uint base = tg.x * 512u;
    uint i0 = base + tid;
    uint i1 = i0 + 256u;
    float v = 0.0f;
    if (i0 < n) v += input[i0];
    if (i1 < n) v += input[i1];
    scratch[tid] = v;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    for (uint s = 128u; s > 0u; s >>= 1u) {
        if (tid < s) scratch[tid] += scratch[tid + s];
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (tid == 0) output[tg.x] = scratch[0];
}
)METAL";

static double median(std::vector<double> v) {
    std::sort(v.begin(), v.end());
    return v[v.size() / 2];
}

__attribute__((noinline))
static float reduce_cpu(const float *x, size_t n) {
    float s = 0.0f;
    for (size_t i = 0; i < n; ++i) s += x[i];
    return s;
}

struct GpuSample {
    double host_ns;
    double device_ns;
    float value;
};

static GpuSample reduce_gpu(
    id<MTLCommandQueue> q,
    id<MTLComputePipelineState> pipe,
    id<MTLBuffer> input,
    id<MTLBuffer> scratch_a,
    id<MTLBuffer> scratch_b,
    uint32_t n)
{
    auto start = std::chrono::steady_clock::now();
    id<MTLCommandBuffer> cb = [q commandBuffer];

    id<MTLBuffer> src = input;
    id<MTLBuffer> dst = scratch_a;
    uint32_t cur_n = n;
    bool flip = false;

    while (cur_n > 1) {
        uint32_t groups = (cur_n + 511u) / 512u;
        id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
        [enc setComputePipelineState:pipe];
        [enc setBuffer:src offset:0 atIndex:0];
        [enc setBuffer:dst offset:0 atIndex:1];
        [enc setBytes:&cur_n length:sizeof(cur_n) atIndex:2];
        [enc dispatchThreadgroups:MTLSizeMake(groups,1,1)
             threadsPerThreadgroup:MTLSizeMake(256,1,1)];
        [enc endEncoding];

        cur_n = groups;
        src = dst;
        flip = !flip;
        dst = flip ? scratch_b : scratch_a;
        if (dst == src) dst = (src == scratch_a) ? scratch_b : scratch_a;
    }

    [cb commit];
    [cb waitUntilCompleted];
    auto end = std::chrono::steady_clock::now();

    double device_ns = 0.0;
    if (cb.GPUEndTime > cb.GPUStartTime && cb.GPUStartTime > 0.0) {
        device_ns = (cb.GPUEndTime - cb.GPUStartTime) * 1e9;
    }
    float value = *static_cast<float *>(src.contents);
    return {
        std::chrono::duration<double,std::nano>(end-start).count(),
        device_ns,
        value
    };
}

int main() {
    @autoreleasepool {
        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        if (!dev) return 2;
        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithSource:[NSString stringWithUTF8String:kShader]
                                               options:nil error:&err];
        if (!lib) return 3;
        id<MTLComputePipelineState> pipe =
            [dev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"reduce_sum"]
                                               error:&err];
        if (!pipe) return 4;
        id<MTLCommandQueue> q = [dev newCommandQueue];

        const size_t max_n = 4 * 1024 * 1024;
        id<MTLBuffer> input = [dev newBufferWithLength:max_n*sizeof(float)
                                             options:MTLResourceStorageModeShared];
        id<MTLBuffer> a = [dev newBufferWithLength:max_n*sizeof(float)
                                         options:MTLResourceStorageModeShared];
        id<MTLBuffer> b = [dev newBufferWithLength:max_n*sizeof(float)
                                         options:MTLResourceStorageModeShared];
        if (!input || !a || !b) return 5;
        float *x = static_cast<float *>(input.contents);
        for (size_t i=0;i<max_n;++i) x[i] = float((int(i%31)-15)) * 0.001f;
        (void)mlock(input.contents, input.length);
        (void)mlock(a.contents, a.length);
        (void)mlock(b.contents, b.length);

        std::printf("# Metal reduction benchmark\n");
        std::printf("device=%s unified_memory=%s\n", dev.name.UTF8String,
                    dev.hasUnifiedMemory ? "true" : "false");
        std::printf("| n | reps | cpu_p50_ns | gpu_p50_ns | gpu_device_p50_ns | winner |\n");
        std::printf("|---:|---:|---:|---:|---:|---|\n");

        for (size_t n : {1024ul, 4096ul, 16384ul, 65536ul, 262144ul, 1048576ul, 4194304ul}) {
            float cpu_v = reduce_cpu(x,n);
            auto warm = reduce_gpu(q,pipe,input,a,b,(uint32_t)n);
            double abs_err = std::abs(double(cpu_v) - double(warm.value));
            double tol = std::max(1e-3, std::abs(double(cpu_v))*1e-3);
            if (abs_err > tol) {
                std::fprintf(stderr,"reduction_correctness_failed n=%zu cpu=%g gpu=%g err=%g tol=%g\n",
                             n,cpu_v,warm.value,abs_err,tol);
                return 6;
            }

            for(int i=0;i<3;++i){
                (void)reduce_cpu(x,n);
                (void)reduce_gpu(q,pipe,input,a,b,(uint32_t)n);
            }
            size_t reps = n <= 65536 ? 50 : (n <= 1048576 ? 20 : 8);
            std::vector<double> cpu_ns,gpu_ns,gpu_dev;
            volatile float sink = 0.0f;
            for(size_t r=0;r<reps;++r){
                if((r&1u)==0){
                    auto t0=std::chrono::steady_clock::now();
                    sink += reduce_cpu(x,n);
                    auto t1=std::chrono::steady_clock::now();
                    cpu_ns.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                    auto g=reduce_gpu(q,pipe,input,a,b,(uint32_t)n);
                    gpu_ns.push_back(g.host_ns); if(g.device_ns>0) gpu_dev.push_back(g.device_ns);
                    sink += g.value;
                }else{
                    auto g=reduce_gpu(q,pipe,input,a,b,(uint32_t)n);
                    gpu_ns.push_back(g.host_ns); if(g.device_ns>0) gpu_dev.push_back(g.device_ns);
                    sink += g.value;
                    auto t0=std::chrono::steady_clock::now();
                    sink += reduce_cpu(x,n);
                    auto t1=std::chrono::steady_clock::now();
                    cpu_ns.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                }
            }
            double c=median(cpu_ns), g=median(gpu_ns), gd=gpu_dev.empty()?0.0:median(gpu_dev);
            std::printf("| %zu | %zu | %.1f | %.1f | %.1f | %s |\n",
                        n,reps,c,g,gd,c<=g?"CPU":"GPU");
            std::printf("reduction_result n=%zu reps=%zu cpu_ns=%.3f gpu_host_ns=%.3f "
                        "gpu_device_ns=%.3f winner=%s abs_err=%g sink=%g\n",
                        n,reps,c,g,gd,c<=g?"CPU":"GPU",abs_err,double(sink));
        }
    }
    return 0;
}
