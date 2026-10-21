//! AlpenCat resource-only, synchronous observation prototype.
//! No background worker, runtime, scheduler, or adaptation policy.
use std::{fs, io, path::Path, time::SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading<T> {
    Known(T),
    Unknown,
    Unsupported,
    Unavailable(String),
}
impl<T> Reading<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reading<U> {
        match self {
            Self::Known(v) => Reading::Known(f(v)),
            Self::Unknown => Reading::Unknown,
            Self::Unsupported => Reading::Unsupported,
            Self::Unavailable(s) => Reading::Unavailable(s),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Limit { Unlimited, Value(u64) }

#[derive(Debug)]
pub struct ResourceSnapshot {
    pub observed_at: SystemTime,
    pub allowed_cpus: Reading<Vec<usize>>,
    pub cpu_quota_us: Reading<Limit>,
    pub cpu_period_us: Reading<u64>,
    pub throttled_periods: Reading<u64>,
    pub memory_max_bytes: Reading<Limit>,
    pub memory_current_bytes: Reading<u64>,
    pub cpu_pressure_some_avg10: Reading<f64>,
}
/// Every field has its own status; one denied metric never invalidates the rest.
/// Scope: Linux process permitted CPU list and its unified cgroup v2.
pub fn snapshot() -> ResourceSnapshot {
    #[cfg(target_os = "linux")]
    { linux_snapshot(Path::new("/proc/self"), Path::new("/sys/fs/cgroup")) }
    #[cfg(not(target_os = "linux"))]
    {
        ResourceSnapshot { observed_at: SystemTime::now(),
            allowed_cpus: Reading::Unsupported, cpu_quota_us: Reading::Unsupported,
            cpu_period_us: Reading::Unsupported, throttled_periods: Reading::Unsupported,
            memory_max_bytes: Reading::Unsupported, memory_current_bytes: Reading::Unsupported,
            cpu_pressure_some_avg10: Reading::Unsupported }
    }
}
fn get_text(path: &Path) -> Reading<String> {
    match fs::read_to_string(path) {
        Ok(v) => Reading::Known(v),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Reading::Unsupported,
        Err(e) => Reading::Unavailable(e.kind().to_string()),
    }
}
fn parsed<T>(v: Reading<String>, f: impl FnOnce(&str) -> Option<T>) -> Reading<T> {
    match v {
        Reading::Known(s) => f(&s).map(Reading::Known).unwrap_or(Reading::Unknown),
        Reading::Unknown => Reading::Unknown, Reading::Unsupported => Reading::Unsupported,
        Reading::Unavailable(e) => Reading::Unavailable(e),
    }
}
pub fn parse_cpu_list(raw: &str) -> Option<Vec<usize>> {
    let mut v = Vec::new();
    for segment in raw.trim().split(',') {
        let segment = segment.trim();
        if segment.is_empty() { return None; }
        if let Some((a,b)) = segment.split_once('-') {
            let a: usize = a.parse().ok()?;
            let b: usize = b.parse().ok()?;
            if b < a || b.saturating_sub(a) > 1_000_000 { return None; }
            v.extend(a..=b);
        } else { v.push(segment.parse().ok()?); }
    }
    v.sort_unstable(); v.dedup(); Some(v)
}
pub fn parse_cpu_max(s: &str) -> Option<(Limit,u64)> {
    let mut it = s.split_whitespace();
    let quota = match it.next()? {
        "max" => Limit::Unlimited,
        v => Limit::Value(v.parse().ok()?),
    };
    let period = it.next()?.parse().ok()?;
    if period == 0 || it.next().is_some() { return None; }
    Some((quota,period))
}
fn parse_limit(s: &str) -> Option<Limit> {
    match s.trim() { "max" => Some(Limit::Unlimited), n => Some(Limit::Value(n.parse().ok()?)) }
}
fn read_key_u64(s: &str, key: &str) -> Option<u64> {
    s.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        if it.next()? == key { it.next()?.parse().ok() } else { None }
    })
}
fn parse_pressure_avg10(s: &str) -> Option<f64> {
    let line = s.lines().find(|l| l.starts_with("some "))?;
    line.split_whitespace().find_map(|p| p.strip_prefix("avg10=")?.parse().ok())
}
/// Resolve the v2 membership path from proc/self/cgroup. No v1 fallback.
pub fn parse_v2_membership(s: &str) -> Option<&str> {
    s.lines().find_map(|l| l.strip_prefix("0::"))
}
#[cfg(target_os = "linux")]
fn linux_snapshot(proc_self: &Path, cgroup_mount: &Path) -> ResourceSnapshot {
    let status = get_text(&proc_self.join("status"));
    let allowed_cpus = parsed(status, |s| s.lines()
       .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
       .and_then(parse_cpu_list));
    let member = parsed(get_text(&proc_self.join("cgroup")), |s| {
        let v = parse_v2_membership(s)?;
        if !v.starts_with('/') || v.split('/').any(|piece| piece == "..") { return None; }
        Some(v.to_owned())
    });
    let cg = match member { Reading::Known(rel) => Some(cgroup_mount.join(rel.trim_start_matches('/'))), _=>None };
    let read_cg = |name: &str| match &cg { Some(base) => get_text(&base.join(name)), None => Reading::Unknown };
    let cpu_max = parsed(read_cg("cpu.max"), parse_cpu_max);
    let (cpu_quota_us,cpu_period_us) = match cpu_max {
       Reading::Known((q,p)) => (Reading::Known(q),Reading::Known(p)),
       Reading::Unsupported => (Reading::Unsupported,Reading::Unsupported),
       Reading::Unavailable(e) => (Reading::Unavailable(e.clone()),Reading::Unavailable(e)),
       Reading::Unknown => (Reading::Unknown,Reading::Unknown),
    };
    ResourceSnapshot { observed_at: SystemTime::now(),
      allowed_cpus, cpu_quota_us,cpu_period_us,
      throttled_periods: parsed(read_cg("cpu.stat"), |s| read_key_u64(s,"nr_throttled")),
      memory_max_bytes: parsed(read_cg("memory.max"),parse_limit),
      memory_current_bytes: parsed(read_cg("memory.current"), |s| s.trim().parse().ok()),
      cpu_pressure_some_avg10: parsed(read_cg("cpu.pressure"),parse_pressure_avg10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn cpus() {
        assert_eq!(parse_cpu_list("3,1-2,2"), Some(vec![1,2,3]));
        assert_eq!(parse_cpu_list("4-2"),None);
        assert_eq!(parse_cpu_list("1,,2"),None);
    }
    #[test] fn limits() {
        assert_eq!(parse_cpu_max("max 100000\n"),Some((Limit::Unlimited,100000)));
        assert_eq!(parse_cpu_max("200000 100000"),Some((Limit::Value(200000),100000)));
        assert_eq!(parse_cpu_max("oops 100000"),None);
        assert_eq!(parse_limit("max"),Some(Limit::Unlimited));
    }
    #[test] fn pressure() {
        assert_eq!(parse_pressure_avg10("some avg10=0.24 avg60=0.03 avg300=0.01 total=55\nfull avg10=0.00"),Some(0.24));
    }
    #[test] fn reading_propagates_absence_and_parse_errors() {
        assert_eq!(parsed::<u64>(Reading::Unsupported, |s| s.parse().ok()), Reading::Unsupported);
        assert_eq!(parsed::<u64>(Reading::Known("bad".to_owned()), |s| s.parse().ok()), Reading::Unknown);
        assert_eq!(parsed::<u64>(Reading::Unavailable("permission".to_owned()), |s| s.parse().ok()), Reading::Unavailable("permission".to_owned()));
    }
    #[cfg(target_os = "linux")]
    #[test] fn isolated_linux_fixture() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("alpencat-snapshot-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let proc = root.join("proc");
        let cgroup = root.join("cgroup");
        let group = cgroup.join("team").join("job");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::create_dir_all(&group).unwrap();
        std::fs::write(proc.join("status"), "Cpus_allowed_list:\t1-3\n").unwrap();
        std::fs::write(proc.join("cgroup"), "0::/team/job\n").unwrap();
        std::fs::write(group.join("cpu.max"), "200000 100000\n").unwrap();
        std::fs::write(group.join("cpu.stat"), "nr_periods 10\nnr_throttled 3\n").unwrap();
        std::fs::write(group.join("memory.max"), "1048576\n").unwrap();
        std::fs::write(group.join("memory.current"), "2048\n").unwrap();
        std::fs::write(group.join("cpu.pressure"), "some avg10=0.32 avg60=0.10 avg300=0.01 total=98\n").unwrap();
        let got = linux_snapshot(&proc, &cgroup);
        assert_eq!(got.allowed_cpus, Reading::Known(vec![1,2,3]));
        assert_eq!(got.cpu_quota_us, Reading::Known(Limit::Value(200000)));
        assert_eq!(got.cpu_period_us, Reading::Known(100000));
        assert_eq!(got.throttled_periods, Reading::Known(3));
        assert_eq!(got.memory_max_bytes, Reading::Known(Limit::Value(1048576)));
        assert_eq!(got.memory_current_bytes, Reading::Known(2048));
        assert_eq!(got.cpu_pressure_some_avg10, Reading::Known(0.32));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn membership() {
        assert_eq!(parse_v2_membership("0::/foo/bar\n"),Some("/foo/bar"));
    }
}
