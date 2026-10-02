#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <atomic>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstring>
#include <thread>
#include <vector>

static std::atomic<bool> g_stop{false};

static void on_signal(int) {
    g_stop.store(true, std::memory_order_relaxed);
}

static void cpu_worker() {
    volatile double x = 1.000001;
    while (!g_stop.load(std::memory_order_relaxed)) {
        for (int i = 0; i < 500000; ++i) {
            x = x * 1.0000001 + 0.0000001;
        }
    }
}

static void memory_worker(size_t bytes) {
    std::vector<unsigned char> buf(bytes, 1);
    volatile unsigned long long sink = 0;
    while (!g_stop.load(std::memory_order_relaxed)) {
        for (size_t i = 0; i < buf.size(); i += 64) {
            buf[i] = static_cast<unsigned char>(buf[i] + 1);
            sink += buf[i];
        }
    }
    std::fprintf(stderr, "memory_sink=%llu\n", sink);
}

static const char *kGpuShader = R"METAL(
#include <metal_stdlib>
using namespace metal;
kernel void burn(device float *x [[buffer(0)]], constant uint &rounds [[buffer(1)]], uint gid [[thread_position_in_grid]]) {
    if (gid >= 262144) return;
    float v = x[gid] + 0.000001f;
    for (uint i = 0; i < rounds; ++i) {
        v = fma(v, 1.000001f, 0.000001f);
    }
    x[gid] = v;
}
)METAL";

static void gpu_worker(uint32_t rounds, int sleep_us) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        if (!device) {
            std::fprintf(stderr, "gpu_load_device=NONE\n");
            return;
        }
        NSError *error = nil;
        NSString *src = [NSString stringWithUTF8String:kGpuShader];
        id<MTLLibrary> lib = [device newLibraryWithSource:src options:nil error:&error];
        if (!lib) {
            std::fprintf(stderr, "gpu_load_library_error=%s\n", error.localizedDescription.UTF8String);
            return;
        }
        id<MTLFunction> fn = [lib newFunctionWithName:@"burn"];
        id<MTLComputePipelineState> pipe =
            [device newComputePipelineStateWithFunction:fn error:&error];
        if (!pipe) {
            std::fprintf(stderr, "gpu_load_pipeline_error=%s\n", error.localizedDescription.UTF8String);
            return;
        }
        id<MTLCommandQueue> q = [device newCommandQueue];
        id<MTLBuffer> b =
            [device newBufferWithLength:262144 * sizeof(float)
                                options:MTLResourceStorageModeShared];
        std::memset(b.contents, 0, b.length);

        while (!g_stop.load(std::memory_order_relaxed)) {
            @autoreleasepool {
                id<MTLCommandBuffer> cb = [q commandBuffer];
                id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
                [enc setComputePipelineState:pipe];
                [enc setBuffer:b offset:0 atIndex:0];
                [enc setBytes:&rounds length:sizeof(rounds) atIndex:1];
                NSUInteger width = std::min<NSUInteger>(
                    256, pipe.maxTotalThreadsPerThreadgroup);
                [enc dispatchThreads:MTLSizeMake(262144, 1, 1)
                  threadsPerThreadgroup:MTLSizeMake(width, 1, 1)];
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
                if (sleep_us > 0) {
                    std::this_thread::sleep_for(std::chrono::microseconds(sleep_us));
                }
            }
        }
    }
}

int main(int argc, char **argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: %s cpu_light|cpu_heavy|mem_resident|mem_bw|gpu_light|gpu_heavy|app_like|video_like|game_like\n", argv[0]);
        return 2;
    }

    std::signal(SIGTERM, on_signal);
    std::signal(SIGINT, on_signal);

    const std::string mode = argv[1];
    std::vector<std::thread> threads;

    auto start_cpu = [&](int count) {
        for (int i = 0; i < count; ++i) threads.emplace_back(cpu_worker);
    };
    auto start_mem_bw = [&]() {
        threads.emplace_back(memory_worker, 512ull * 1024ull * 1024ull);
    };
    auto start_mem_resident = [&]() {
        threads.emplace_back([] {
            const size_t bytes = 1024ull * 1024ull * 1024ull;
            std::vector<unsigned char> buf(bytes, 1);
            for (size_t i = 0; i < buf.size(); i += 16384) buf[i] ^= 1;
            while (!g_stop.load(std::memory_order_relaxed)) {
                std::this_thread::sleep_for(std::chrono::milliseconds(50));
            }
        });
    };
    auto start_gpu_light = [&]() {
        threads.emplace_back(gpu_worker, 32u, 1000);
    };
    auto start_gpu_heavy = [&]() {
        threads.emplace_back(gpu_worker, 256u, 0);
    };

    if (mode == "cpu_light") start_cpu(1);
    else if (mode == "cpu_heavy") start_cpu(2);
    else if (mode == "mem_resident") start_mem_resident();
    else if (mode == "mem_bw") start_mem_bw();
    else if (mode == "gpu_light") start_gpu_light();
    else if (mode == "gpu_heavy") start_gpu_heavy();
    else if (mode == "app_like") { start_cpu(1); start_mem_resident(); }
    else if (mode == "video_like") { start_cpu(1); start_gpu_light(); }
    else if (mode == "game_like") { start_cpu(2); start_mem_bw(); start_gpu_heavy(); }

    if (threads.empty()) {
        std::fprintf(stderr, "unknown mode=%s\n", mode.c_str());
        return 3;
    }

    std::printf("load_mode=%s\n", mode.c_str());
    std::fflush(stdout);

    for (auto &t : threads) t.join();
    return 0;
}
