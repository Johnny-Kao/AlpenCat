//! Real HTTP service using tiny_http, separate from AlpenCat's runtime.
//! Linux cgroup quota changes externally; dynamic policies observe but never control them.
#![cfg(target_os = "linux")]
use alpencat_resource::{snapshot, Reading, ResourceSnapshot, ResourceState};
use std::{
    env,
    hint::black_box,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tiny_http::{Response, Server};

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
unsafe extern "C" {
    fn clock_gettime(clock_id: i32, tp: *mut Timespec) -> i32;
}
fn cpu_ns() -> u128 {
    let mut t = Timespec { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(2, &mut t) }, 0);
    t.sec as u128 * 1_000_000_000 + t.nsec as u128
}

#[derive(Clone, Copy)]
enum Mode { Fixed1, Fixed2, Fixed4, Fixed8, DirectQuota, ResourceQuota, DirectGuard, ResourceGuard }
impl Mode {
    fn parse(s: &str) -> Self {
        match s {
            "fixed1" => Self::Fixed1, "fixed2" => Self::Fixed2,
            "fixed4" => Self::Fixed4, "fixed8" => Self::Fixed8,
            "direct_quota" => Self::DirectQuota, "alpencat_quota" => Self::ResourceQuota,
            "direct_guard" => Self::DirectGuard, "alpencat_guard" => Self::ResourceGuard,
            _ => panic!("unsupported mode {s}"),
        }
    }
    fn observe_direct(self) -> bool { matches!(self, Self::DirectQuota | Self::DirectGuard) }
    fn observe_state(self) -> bool { matches!(self, Self::ResourceQuota | Self::ResourceGuard) }
    fn guard(self) -> bool { matches!(self, Self::DirectGuard | Self::ResourceGuard) }
}
fn quota(s: &ResourceSnapshot) -> Option<f64> {
    match &s.effective_cpu_capacity {
        Reading::Known(x) if x.is_finite() && *x > 0.0 => Some(*x),
        _ => None,
    }
}
fn worker_plan(mode: Mode, capacity: f64) -> usize {
    match mode {
        Mode::Fixed1 => 1, Mode::Fixed2 => 2, Mode::Fixed4 => 4, Mode::Fixed8 => 8,
        _ if mode.guard() => if capacity < 1.0 { 2 } else { 4 },
        _ => (capacity.ceil() as usize).clamp(1,8),
    }
}
fn compute(id: u64, rounds: usize) -> u64 {
    let mut state = id.wrapping_mul(0x9e3779b97f4a7c15) ^ 0xd1b54a32d192ed03;
    for n in 0..rounds {
        state ^= (n as u64).wrapping_add(id << 19).wrapping_mul(0xbf58476d1ce4e5b9);
        state = state.rotate_left(17).wrapping_mul(0x94d049bb133111eb);
    }
    black_box(state)
}
fn response_for(path: &str) -> Option<String> {
    let mut fields = path.trim_start_matches('/').split('/');
    if fields.next()? != "work" { return None; }
    let id: u64 = fields.next()?.parse().ok()?;
    let rounds: usize = fields.next()?.parse().ok()?;
    if fields.next().is_some() || !(1..=2_000_000).contains(&rounds) { return None; }
    Some(format!("{:016x}",compute(id,rounds)))
}
fn main() {
    let a: Vec<_> = env::args().collect();
    assert_eq!(a.len(),4,"pilot <mode> <port> <control socket>");
    let mode = Mode::parse(&a[1]);
    let port: u16 = a[2].parse().expect("TCP port");
    let server=Arc::new(Server::http(format!("127.0.0.1:{port}")).expect("real HTTP bind"));
    let mut control = {
        let started=Instant::now();
        loop {
            match UnixStream::connect(&a[3]) {
                Ok(s)=>break s,
                Err(e) if started.elapsed()<Duration::from_secs(5)=>{
                    thread::sleep(Duration::from_millis(10));
                    black_box(e);
                },
                Err(e)=>panic!("cannot connect controller: {e}"),
            }
        }
    };
    control.set_read_timeout(Some(Duration::from_secs(45))).unwrap();
    let mut reader=BufReader::new(control.try_clone().unwrap());
    let active=Arc::new(AtomicUsize::new(1));
    let running=Arc::new(AtomicBool::new(true));
    // Persistent worker pool. Fixed and dynamic arms create the SAME 8 threads.
    // At most the active first N workers call recv_timeout; others park briefly.
    let mut joins=Vec::new();
    for worker_id in 0..8usize {
        let srv=Arc::clone(&server);
        let active=Arc::clone(&active);
        let running=Arc::clone(&running);
        joins.push(thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                if worker_id>=active.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                match srv.recv_timeout(Duration::from_millis(10)) {
                    Ok(Some(req)) => {
                        let body=response_for(req.url());
                        let (body,status): (String, u16)=match body{
                            Some(b)=>(b,200),None=>("invalid request".to_string(),400)
                        };
                        let _=req.respond(Response::from_string(body).with_status_code(status));
                    },
                    Ok(None)=>{},
                    Err(err)=>panic!("http receiver error: {err}"),
                }
            }
        }));
    }
    let state=ResourceState::new();
    writeln!(control,"READY pid={} port={port}",std::process::id()).unwrap();
    for phase in 0..3 {
        let mut cmd=String::new();
        assert!(reader.read_line(&mut cmd).unwrap()>0,"control socket closed");
        let expected: u32=cmd.trim().strip_prefix("GO ").expect("GO quota").parse().unwrap();
        // Equal environment integrity read excluded for all arms.
        let observed=quota(&snapshot()).expect("cgroup v2 cpu quota unavailable");
        assert_eq!((observed*1000.0).round() as u32,expected,"real quota mismatch");
        let cpu_before=cpu_ns();
        let start=Instant::now();
        let observed_capacity=if mode.observe_direct(){
            quota(&snapshot()).expect("direct quota unavailable")
        }else if mode.observe_state(){
            state.invalidate();
            let current=state.refresh().expect("cgroup changed during read");
            quota(current.fresh().expect("no fresh snapshot")).expect("resource state quota missing")
        }else{observed};
        assert_eq!((observed_capacity*1000.0).round() as u32,expected);
        let workers=worker_plan(mode,observed_capacity);
        active.store(workers,Ordering::Release);
        let decision_ns=start.elapsed().as_nanos();
        writeln!(control,"PHASE phase={phase} workers={workers} quota_milli={expected} decision_ns={decision_ns}").unwrap();
        let mut done=String::new();
        assert!(reader.read_line(&mut done).unwrap()>0);
        assert_eq!(done.trim(),"DONE");
        let cpu_total=cpu_ns().saturating_sub(cpu_before);
        let wall_total=start.elapsed().as_nanos();
        writeln!(control,"END phase={phase} cpu_ns={cpu_total} wall_ns={wall_total}").unwrap();
    }
    running.store(false,Ordering::Relaxed);
    for j in joins { j.join().expect("HTTP worker panic"); }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn deterministic_handler(){
        assert_eq!(response_for("/work/17/400"),response_for("/work/17/400"));
        assert_ne!(response_for("/work/18/400"),response_for("/work/17/400"));
        assert!(response_for("/work/17/0").is_none());
        assert!(response_for("/other/12/100").is_none());
    }
    #[test]
    fn bounded_worker_controls(){
        assert_eq!(worker_plan(Mode::ResourceQuota,0.5),1);
        assert_eq!(worker_plan(Mode::DirectGuard,0.5),2);
        assert_eq!(worker_plan(Mode::ResourceGuard,2.0),4);
        assert_eq!(worker_plan(Mode::Fixed8,0.5),8);
    }
}
