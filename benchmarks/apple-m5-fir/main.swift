// Standalone full-path FIR benchmark. Reconstructed 2026-10-02;
// the previous temporary Swift harness was not recoverable.
import Foundation
import Metal
import Dispatch

let device = MTLCreateSystemDefaultDevice()!
let queue = device.makeCommandQueue()!
let source = """
#include <metal_stdlib>
using namespace metal;
kernel void fir(device const float *x [[buffer(0)]],
                device const float *h [[buffer(1)]],
                device float *y [[buffer(2)]],
                constant uint &n [[buffer(3)]],
                constant uint &taps [[buffer(4)]],
                uint i [[thread_position_in_grid]]) {
    if (i >= n) return;
    float sum = 0.0f;
    for (uint j = 0; j < taps; ++j) sum += x[i+j] * h[j];
    y[i] = sum;
}
"""
let library = try device.makeLibrary(source: source, options: nil)
let pipeline = try device.makeComputePipelineState(function: library.makeFunction(name: "fir")!)

func cpu(_ x: [Float], _ h: [Float], _ n: Int) -> [Float] {
    var y = [Float](repeating: 0, count: n)
    x.withUnsafeBufferPointer { xp in
        h.withUnsafeBufferPointer { hp in
            y.withUnsafeMutableBufferPointer { yp in
                DispatchQueue.concurrentPerform(iterations: 64) { chunk in
                    let lo = chunk * n / 64
                    let hi = (chunk + 1) * n / 64
                    for i in lo..<hi {
                        var sum: Float = 0
                        for j in 0..<h.count { sum += xp[i+j] * hp[j] }
                        yp[i] = sum
                    }
                }
            }
        }
    }
    return y
}

func gpu(_ x: [Float], _ h: [Float], _ n: Int) -> [Float] {
    let xb = x.withUnsafeBytes { device.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared)! }
    let hb = h.withUnsafeBytes { device.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared)! }
    let yb = device.makeBuffer(length: n * MemoryLayout<Float>.stride, options: .storageModeShared)!
    let command = queue.makeCommandBuffer()!
    let encoder = command.makeComputeCommandEncoder()!
    encoder.setComputePipelineState(pipeline)
    encoder.setBuffer(xb, offset: 0, index: 0)
    encoder.setBuffer(hb, offset: 0, index: 1)
    encoder.setBuffer(yb, offset: 0, index: 2)
    var count = UInt32(n)
    var taps = UInt32(h.count)
    encoder.setBytes(&count, length: 4, index: 3)
    encoder.setBytes(&taps, length: 4, index: 4)
    encoder.dispatchThreads(MTLSize(width: n, height: 1, depth: 1),
        threadsPerThreadgroup: MTLSize(width: min(256, pipeline.maxTotalThreadsPerThreadgroup), height: 1, depth: 1))
    encoder.endEncoding()
    command.commit()
    command.waitUntilCompleted()
    precondition(command.status == .completed, "Metal command failed")
    return Array(UnsafeBufferPointer(start: yb.contents().assumingMemoryBound(to: Float.self), count: n))
}

func measure(_ operation: () -> [Float]) -> (Double, [Float]) {
    let start = DispatchTime.now().uptimeNanoseconds
    let output = operation()
    let elapsed = Double(DispatchTime.now().uptimeNanoseconds - start)
    return (elapsed, output)
}
func percentile(_ values: [Double], _ fraction: Double) -> Double {
    let sorted = values.sorted()
    return sorted[Int(Double(sorted.count - 1) * fraction)]
}

fputs("device=\(device.name); cpu=GCD/64 chunks; gpu=Metal full path including output copy; warmup=3; paired_repeats=11\n", stderr)
print("n,taps,cpu_p50_ns,gpu_p50_ns,cpu_p90_ns,gpu_p90_ns,max_abs_error")
for taps in [64, 128, 256] {
    for n in [16_384, 32_768, 65_536, 262_144, 1_048_576] {
        let x = (0..<(n+taps-1)).map { Float(sin(Double($0) * 0.01)) }
        let h = (0..<taps).map { Float(cos(Double($0) * 0.07) / Double(taps)) }
        for _ in 0..<3 { _ = cpu(x, h, n); _ = gpu(x, h, n) }
        var cs: [Double] = []
        var gs: [Double] = []
        var maxError: Float = 0
        for rep in 0..<11 {
            let c: (Double, [Float])
            let g: (Double, [Float])
            if rep % 2 == 0 { c = measure { cpu(x, h, n) }; g = measure { gpu(x, h, n) } }
            else { g = measure { gpu(x, h, n) }; c = measure { cpu(x, h, n) } }
            cs.append(c.0); gs.append(g.0)
            for i in 0..<n { maxError = max(maxError, abs(c.1[i] - g.1[i])) }
        }
        precondition(maxError < 1e-5, "FIR output mismatch")
        print("\(n),\(taps),\(percentile(cs, 0.5)),\(percentile(gs, 0.5)),\(percentile(cs, 0.9)),\(percentile(gs, 0.9)),\(maxError)")
    }
}
