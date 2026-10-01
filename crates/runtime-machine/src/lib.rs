//! Lightweight host capability discovery.
//!
//! This crate deliberately avoids heavyweight hardware-inventory dependencies.
//! Partial information is valid: callers must treat unknown fields as unknown.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProfile {
    pub os: &'static str,
    pub architecture: &'static str,
    pub logical_cpus: usize,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
}

impl HostProfile {
    pub fn discover() -> Self {
        let (memory_total_bytes, memory_available_bytes) = discover_memory();

        Self {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            logical_cpus: std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1),
            memory_total_bytes,
            memory_available_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other(u32),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDeviceProfile {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub vendor_id: Option<u32>,
    pub device_id: Option<u32>,
    pub dedicated_memory_bytes: Option<u64>,
}

impl GpuDeviceProfile {
    pub const fn vendor(&self) -> GpuVendor {
        match self.vendor_id {
            Some(0x10de) => GpuVendor::Nvidia,
            Some(0x1002) | Some(0x1022) => GpuVendor::Amd,
            Some(0x8086) => GpuVendor::Intel,
            Some(0x106b) => GpuVendor::Apple,
            Some(id) => GpuVendor::Other(id),
            None => GpuVendor::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineProfile {
    pub host: HostProfile,
    pub gpus: Vec<GpuDeviceProfile>,
}

impl MachineProfile {
    pub fn host_only() -> Self {
        Self {
            host: HostProfile::discover(),
            gpus: Vec::new(),
        }
    }
}

#[cfg(target_os = "linux")]
fn discover_memory() -> (Option<u64>, Option<u64>) {
    let Ok(contents) = std::fs::read_to_string("/proc/meminfo") else {
        return (None, None);
    };

    let total = parse_meminfo_kib(&contents, "MemTotal:");
    let available = parse_meminfo_kib(&contents, "MemAvailable:");
    (total, available)
}

#[cfg(not(target_os = "linux"))]
fn discover_memory() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(target_os = "linux")]
fn parse_meminfo_kib(contents: &str, key: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.trim();
        let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        kib.checked_mul(1024)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_profile_has_nonzero_parallelism() {
        let profile = HostProfile::discover();
        assert!(profile.logical_cpus >= 1);
        assert!(!profile.os.is_empty());
        assert!(!profile.architecture.is_empty());
    }

    #[test]
    fn gpu_vendor_ids_are_classified() {
        let profile = GpuDeviceProfile {
            name: "gpu".into(),
            backend: "Vulkan".into(),
            device_type: "DiscreteGpu".into(),
            vendor_id: Some(0x10de),
            device_id: Some(1),
            dedicated_memory_bytes: None,
        };
        assert_eq!(profile.vendor(), GpuVendor::Nvidia);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_meminfo_parser_reads_kib() {
        let sample = "MemTotal:       16384 kB\nMemAvailable:    8192 kB\n";
        assert_eq!(parse_meminfo_kib(sample, "MemTotal:"), Some(16_777_216));
        assert_eq!(parse_meminfo_kib(sample, "MemAvailable:"), Some(8_388_608));
    }
}
