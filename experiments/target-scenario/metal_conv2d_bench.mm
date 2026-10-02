#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sys/mman.h>
#include <vector>

static const char *kShader = R"METAL(
#include <metal_stdlib>
using namespace metal;

kernel void conv2d_f32(
    const device float *x [[buffer(0)]],
    const device float *k [[buffer(1)]],
    device float *y [[buffer(2)]],
    constant uint &w [[buffer(3)]],
    constant uint &h [[buffer(4)]],
    constant uint &ks [[buffer(5)]],
    uint2 gid [[thread_position_in_grid]])
{
    if (gid.x >= w || gid.y >= h) return;
    int r = int(ks / 2u);
    float acc = 0.0f;
    for (uint ky = 0; ky < ks; ++ky) {
        for (uint kx = 0; kx < ks; ++kx) {
            int ix = clamp(int(gid.x) + int(kx) - r, 0, int(w) - 1);
            int iy = clamp(int(gid.y) + int(ky) - r, 0, int(h) - 1);
            acc = fma(x[iy * int(w) + ix], k[ky * ks + kx], acc);
        }
    }
    y[gid.y * w + gid.x] = acc;
}
)METAL";

static double median(std::vector<double> v) {
    std::sort(v.begin(), v.end());
    return v[v.size()/2];
}

__attribute__((noinline))
static void conv_cpu(
    const float *x, const float *k, float *y,
    size_t w, size_t h, size_t ks)
{
    int r = int(ks/2);
    for (size_t yy=0; yy<h; ++yy) {
        for (size_t xx=0; xx<w; ++xx) {
            float acc = 0.0f;
            for (size_t ky=0; ky<ks; ++ky) {
                for (size_t kx=0; kx<ks; ++kx) {
                    int ix = std::max(0, std::min(int(w)-1, int(xx)+int(kx)-r));
                    int iy = std::max(0, std::min(int(h)-1, int(yy)+int(ky)-r));
                    acc += x[size_t(iy)*w + size_t(ix)] * k[ky*ks+kx];
                }
            }
            y[yy*w+xx] = acc;
        }
    }
}

struct GpuSample { double host_ns; double device_ns; };

static GpuSample conv_gpu(
    id<MTLCommandQueue> q,
    id<MTLComputePipelineState> p,
    id<MTLBuffer> x,
    id<MTLBuffer> k,
    id<MTLBuffer> y,
    uint32_t w, uint32_t h, uint32_t ks)
{
    auto t0 = std::chrono::steady_clock::now();
    id<MTLCommandBuffer> cb = [q commandBuffer];
    id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
    [enc setComputePipelineState:p];
    [enc setBuffer:x offset:0 atIndex:0];
    [enc setBuffer:k offset:0 atIndex:1];
    [enc setBuffer:y offset:0 atIndex:2];
    [enc setBytes:&w length:sizeof(w) atIndex:3];
    [enc setBytes:&h length:sizeof(h) atIndex:4];
    [enc setBytes:&ks length:sizeof(ks) atIndex:5];

    NSUInteger tw = 16, th = 16;
    [enc dispatchThreads:MTLSizeMake(w,h,1)
      threadsPerThreadgroup:MTLSizeMake(tw,th,1)];
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
    auto t1 = std::chrono::steady_clock::now();

    double dev_ns = 0.0;
    if (cb.GPUEndTime > cb.GPUStartTime && cb.GPUStartTime > 0.0)
        dev_ns = (cb.GPUEndTime - cb.GPUStartTime) * 1e9;

    return {std::chrono::duration<double,std::nano>(t1-t0).count(), dev_ns};
}

int main() {
    @autoreleasepool {
        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        if (!dev) return 2;
        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithSource:[NSString stringWithUTF8String:kShader]
                                               options:nil error:&err];
        if (!lib) {
            std::fprintf(stderr, "conv_library_error=%s\n", err.localizedDescription.UTF8String);
            return 3;
        }
        id<MTLComputePipelineState> p =
            [dev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"conv2d_f32"]
                                               error:&err];
        if (!p) return 4;
        id<MTLCommandQueue> q = [dev newCommandQueue];

        const size_t max_w = 1024, max_h = 1024, max_ks = 7;
        id<MTLBuffer> xb = [dev newBufferWithLength:max_w*max_h*sizeof(float)
                                           options:MTLResourceStorageModeShared];
        id<MTLBuffer> kb = [dev newBufferWithLength:max_ks*max_ks*sizeof(float)
                                           options:MTLResourceStorageModeShared];
        id<MTLBuffer> yb = [dev newBufferWithLength:max_w*max_h*sizeof(float)
                                           options:MTLResourceStorageModeShared];
        if (!xb || !kb || !yb) return 5;
        float *x = static_cast<float *>(xb.contents);
        float *k = static_cast<float *>(kb.contents);
        float *yg = static_cast<float *>(yb.contents);
        std::vector<float> yc(max_w*max_h);

        for (size_t i=0;i<max_w*max_h;++i) x[i] = float(int(i%101)-50)*0.001f;
        for (size_t i=0;i<max_ks*max_ks;++i) k[i] = 1.0f/float(i+1);
        (void)mlock(xb.contents, xb.length);
        (void)mlock(kb.contents, kb.length);
        (void)mlock(yb.contents, yb.length);

        std::printf("# Metal Conv2D benchmark\n");
        std::printf("device=%s unified_memory=%s\n", dev.name.UTF8String,
                    dev.hasUnifiedMemory ? "true" : "false");

        struct Case { size_t w,h,ks; };
        const Case cases[] = {
            {64,64,3},{128,128,3},{256,256,3},{512,512,3},
            {64,64,5},{128,128,5},{256,256,5},{512,512,5},
            {64,64,7},{128,128,7},{256,256,7}
        };

        for (auto c : cases) {
            conv_cpu(x,k,yc.data(),c.w,c.h,c.ks);
            auto warm = conv_gpu(q,p,xb,kb,yb,(uint32_t)c.w,(uint32_t)c.h,(uint32_t)c.ks);
            double max_abs=0.0;
            for(size_t i=0;i<c.w*c.h;++i)
                max_abs=std::max(max_abs,std::abs(double(yc[i])-double(yg[i])));
            if(max_abs>2e-3) {
                std::fprintf(stderr,"conv_correctness_failed w=%zu h=%zu ks=%zu err=%g\n",
                             c.w,c.h,c.ks,max_abs);
                return 6;
            }
            for(int i=0;i<3;++i){
                conv_cpu(x,k,yc.data(),c.w,c.h,c.ks);
                (void)conv_gpu(q,p,xb,kb,yb,(uint32_t)c.w,(uint32_t)c.h,(uint32_t)c.ks);
            }
            size_t reps=(c.w<=128?20:(c.w<=256?10:6));
            std::vector<double> cs,gs,gds;
            for(size_t r=0;r<reps;++r){
                if((r&1u)==0){
                    auto t0=std::chrono::steady_clock::now();
                    conv_cpu(x,k,yc.data(),c.w,c.h,c.ks);
                    auto t1=std::chrono::steady_clock::now();
                    cs.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                    auto g=conv_gpu(q,p,xb,kb,yb,(uint32_t)c.w,(uint32_t)c.h,(uint32_t)c.ks);
                    gs.push_back(g.host_ns); if(g.device_ns>0)gds.push_back(g.device_ns);
                } else {
                    auto g=conv_gpu(q,p,xb,kb,yb,(uint32_t)c.w,(uint32_t)c.h,(uint32_t)c.ks);
                    gs.push_back(g.host_ns); if(g.device_ns>0)gds.push_back(g.device_ns);
                    auto t0=std::chrono::steady_clock::now();
                    conv_cpu(x,k,yc.data(),c.w,c.h,c.ks);
                    auto t1=std::chrono::steady_clock::now();
                    cs.push_back(std::chrono::duration<double,std::nano>(t1-t0).count());
                }
            }
            double cpu=median(cs), gpu=median(gs), gd=gds.empty()?0.0:median(gds);
            std::printf(
                "conv_result w=%zu h=%zu ks=%zu work=%zu reps=%zu cpu_ns=%.3f "
                "gpu_host_ns=%.3f gpu_device_ns=%.3f winner=%s max_abs=%g\n",
                c.w,c.h,c.ks,c.w*c.h*c.ks*c.ks,reps,cpu,gpu,gd,
                cpu<=gpu?"CPU":"GPU",max_abs);
        }
    }
    return 0;
}
