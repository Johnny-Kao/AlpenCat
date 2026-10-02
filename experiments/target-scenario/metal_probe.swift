import Foundation
import Metal

guard let device = MTLCreateSystemDefaultDevice() else {
    fputs("metal_device=NONE\n", stderr)
    exit(2)
}

print("metal_device=\(device.name)")
print("metal_low_power=\(device.isLowPower)")
print("metal_headless=\(device.isHeadless)")
if #available(macOS 10.15, *) {
    print("metal_unified_memory=\(device.hasUnifiedMemory)")
}
print("metal_registry_id=\(device.registryID)")
