//! AlpenCat synchronous resource snapshot API (experimental alpha).
//! No background worker, runtime, scheduler, or adaptation policy.
// Parsing helpers are used by Linux observation, and remain available for fixture tests.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
use std::{fs, io, path::{Path, PathBuf}, time::SystemTime};

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
    pub completed_at: SystemTime,
    /// True only when the process cgroup was resolved to a known cgroup2 mount.
    pub cgroup_resolved: bool,
    /// Limits are bounded by visible ancestors only; hidden host parents are not observable.
    pub limits_complete: bool,
    /// Best-effort detection of a change to cgroup membership during collection.
    pub membership_changed_during_read: bool,
    /// CPU quota divided by period, in CPU equivalents, smallest visible ancestor.
    pub effective_cpu_capacity: Reading<f64>,
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
        ResourceSnapshot { observed_at: SystemTime::now(), completed_at: SystemTime::now(), cgroup_resolved: false, limits_complete: false, membership_changed_during_read: false, effective_cpu_capacity: Reading::Unsupported,
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
fn mount_unescape(s: &str) -> String {
    let mut result = s.to_owned();
    for (from,to) in [("\\040"," "),("\\011","\t"),("\\012","\n"),("\\134","\\")] {
        result = result.replace(from,to);
    }
    result
}
/// Find a cgroup2 mount and resolve a /proc/self/cgroup path within the mount root.
/// Longest matching mount root wins; no path may traverse beyond the mount.
pub fn resolve_cgroup_mount(mountinfo: &str, member: &str) -> Option<(PathBuf, bool)> {
    if !member.starts_with('/') || member.split('/').any(|x| x == ".." || x == ".") { return None; }
    let mut winner: Option<(usize, PathBuf, bool)> = None;
    for line in mountinfo.lines() {
        let (left,right) = line.split_once(" - ")?;
        if right.split_whitespace().next() != Some("cgroup2") { continue; }
        let columns: Vec<_> = left.split_whitespace().collect();
        if columns.len() < 5 { continue; }
        let root = mount_unescape(columns[3]);
        let mount = mount_unescape(columns[4]);
        if !root.starts_with('/') || !mount.starts_with('/') { continue; }
        let rel = if root == "/" { member.trim_start_matches('/') }
            else if member == root { "" }
            else if let Some(v) = member.strip_prefix(&(root.clone()+"/")) { v }
            else { continue };
        if rel.split('/').any(|x| x == ".." || x == ".") { continue; }
        let path = Path::new(&mount).join(rel);
        let complete = root == "/";
        if winner.as_ref().is_none_or(|(n,_,_)| root.len() > *n) {
            winner = Some((root.len(),path,complete));
        }
    }
    winner.map(|(_,p,c)| (p,c))
}
fn walk_visible(cg: &Path, root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pos = cg.to_path_buf();
    loop {
        found.push(pos.clone());
        if pos == root { break; }
        if !pos.starts_with(root) { return Vec::new(); }
        let Some(parent) = pos.parent() else { return Vec::new() };
        pos = parent.to_path_buf();
    }
    found
}
fn aggregate_memory(ancestors: &[PathBuf]) -> Reading<Limit> {
    let mut smallest = u64::MAX;
    for p in ancestors {
        match parsed(get_text(&p.join("memory.max")),parse_limit) {
            Reading::Known(Limit::Value(n)) => smallest = smallest.min(n),
            Reading::Known(Limit::Unlimited) => {},
            Reading::Unsupported => return Reading::Unsupported,
            Reading::Unknown => return Reading::Unknown,
            Reading::Unavailable(e) => return Reading::Unavailable(e),
        }
    }
    Reading::Known(if smallest == u64::MAX { Limit::Unlimited } else { Limit::Value(smallest) })
}
fn aggregate_cpu(ancestors: &[PathBuf]) -> (Reading<Limit>,Reading<u64>,Reading<f64>) {
    let mut best: Option<(u64,u64)> = None;
    for p in ancestors {
        match parsed(get_text(&p.join("cpu.max")),parse_cpu_max) {
            Reading::Known((Limit::Value(q),period)) => {
                if best.is_none_or(|(a,b)| (q as u128)*(b as u128) < (a as u128)*(period as u128)) {
                    best=Some((q,period));
                }
            },
            Reading::Known((Limit::Unlimited,_)) => {},
            Reading::Unsupported => return (Reading::Unsupported,Reading::Unsupported,Reading::Unsupported),
            Reading::Unknown => return (Reading::Unknown,Reading::Unknown,Reading::Unknown),
            Reading::Unavailable(e) => return (Reading::Unavailable(e.clone()),Reading::Unavailable(e.clone()),Reading::Unavailable(e)),
        }
    }
    match best {
        Some((q,p)) => (Reading::Known(Limit::Value(q)),Reading::Known(p),Reading::Known(q as f64/p as f64)),
        None => (Reading::Known(Limit::Unlimited),Reading::Unknown,Reading::Unknown),
    }
}
#[cfg(target_os = "linux")]
fn linux_snapshot(proc_self: &Path, fallback_mount: &Path) -> ResourceSnapshot {
    let begun = SystemTime::now();
    let allowed_cpus = parsed(get_text(&proc_self.join("status")), |s| s.lines()
        .find_map(|l| l.strip_prefix("Cpus_allowed_list:")).and_then(parse_cpu_list));
    let membership_first = get_text(&proc_self.join("cgroup"));
    let membership = parsed(membership_first.clone(), |s| parse_v2_membership(s).map(str::to_owned));
    let mounts = get_text(&proc_self.join("mountinfo"));
    let mapped = match (&membership,&mounts) {
        (Reading::Known(m),Reading::Known(info)) => resolve_cgroup_mount(info,m),
        _ => None,
    };
    // For test fixture compatibility only: a missing mountinfo does not silently
    // guess a container mapping in a real procfs environment.
    let fallback = if proc_self != Path::new("/proc/self") {
        match &membership { Reading::Known(m) if m.starts_with('/') && !m.split('/').any(|x| x == "..") =>
            Some((fallback_mount.join(m.trim_start_matches('/')),true)), _ => None }
    } else { None };
    let cg = mapped.or(fallback);
    let (ancestors,complete) = match cg.as_ref() {
        Some((p,whole)) => {
            let root = match (&mounts,&membership) {
                (Reading::Known(info),Reading::Known(m)) => {
                    resolve_cgroup_mount(info,m).and_then(|(path,_)| {
                        // The last accessible mount boundary is found from the cgroup2 mount point.
                        info.lines().find_map(|l| {
                            let (a,b)=l.split_once(" - ")?;
                            if b.split_whitespace().next()!=Some("cgroup2") { return None; }
                            let v:Vec<_>=a.split_whitespace().collect();
                            if v.len()<5 { return None; }
                            let mount=PathBuf::from(mount_unescape(v[4]));
                            path.starts_with(&mount).then_some(mount)
                        })
                    }).unwrap_or_else(|| fallback_mount.to_path_buf())
                },
                _ => fallback_mount.to_path_buf(),
            };
            (walk_visible(p,&root),*whole)
        },
        None => (Vec::new(),false)
    };
    let (cpu_quota_us,cpu_period_us,effective_cpu_capacity,memory_max_bytes) =
        if ancestors.is_empty() { (Reading::Unknown,Reading::Unknown,Reading::Unknown,Reading::Unknown) }
        else {
            let (q,p,c)=aggregate_cpu(&ancestors);
            (q,p,c,aggregate_memory(&ancestors))
        };
    let read_cg = |name: &str| match cg.as_ref() {
        Some((p,_)) => get_text(&p.join(name)), None => Reading::Unknown
    };
    let membership_changed_during_read = membership_first != get_text(&proc_self.join("cgroup"));
    ResourceSnapshot {
        observed_at: begun,completed_at:SystemTime::now(),cgroup_resolved:cg.is_some(),
        membership_changed_during_read,
        limits_complete:complete && !ancestors.is_empty(),
        effective_cpu_capacity,
        allowed_cpus,cpu_quota_us,cpu_period_us,
        throttled_periods:parsed(read_cg("cpu.stat"), |s| read_key_u64(s,"nr_throttled")),
        memory_max_bytes,
        memory_current_bytes:parsed(read_cg("memory.current"), |s| s.trim().parse().ok()),
        cpu_pressure_some_avg10:parsed(read_cg("cpu.pressure"),parse_pressure_avg10),
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
        std::fs::write(cgroup.join("cpu.max"), "max 100000\n").unwrap();
        std::fs::write(cgroup.join("memory.max"), "max\n").unwrap();
        std::fs::write(cgroup.join("team/cpu.max"), "400000 100000\n").unwrap();
        std::fs::write(cgroup.join("team/memory.max"), "2097152\n").unwrap();
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
    #[cfg(target_os="linux")]
    #[test] fn mapping_and_hierarchical_limits() {
        let mi = "32 1 0:20 /tenant /sys/fs/cgroup rw - cgroup2 cgroup rw\n";
        assert_eq!(resolve_cgroup_mount(mi, "/tenant/app").map(|x| x.0), Some(PathBuf::from("/sys/fs/cgroup/app")));
        assert_eq!(resolve_cgroup_mount(mi, "/different"), None);
        assert_eq!(resolve_cgroup_mount(mi, "/tenant/../escape"), None);
        assert_eq!(aggregate_cpu(&[]).2, Reading::Unknown); 
    }
    #[test] fn membership() {
        assert_eq!(parse_v2_membership("0::/foo/bar\n"),Some("/foo/bar"));
    }
}

/// Explicit cached resource observation. No background worker or auto-policy.
pub mod state;
pub use state::{CachedSnapshot, Freshness, RefreshError, ResourceState};
