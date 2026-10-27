//! An independently compiled, real HTTP adopter of AlpenCat Guard.
//! Eight server threads remain alive. Guard controls *admission*, never
//! the OS Cgroup, global Rayon pool, or an implicit background poller.
use alpencat_guard::{AdmissionGate, Guard, GuardConfig, Mode};
use alpencat_resource::ResourceState;
use std::{
    env,
    hint::black_box,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tiny_http::{Response, Server};

fn compute(id: u64, rounds: usize) -> u64 {
    let mut x = id.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0xd1b5_4a32_d192_ed03;
    for n in 0..rounds {
        x ^= (n as u64).wrapping_add(id << 19)
            .wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = x.rotate_left(17).wrapping_mul(0x94d0_49bb_1331_11eb);
    }
    black_box(x)
}
fn execute(path: &str) -> Option<String> {
    let mut tokens = path.trim_start_matches('/').split('/');
    if tokens.next()? != "work" {
        return None;
    }
    let id = tokens.next()?.parse::<u64>().ok()?;
    let rounds = tokens.next()?.parse::<usize>().ok()?;
    if tokens.next().is_some() || !(1..=4_000_000).contains(&rounds) {
        return None;
    }
    Some(format!("{:016x}", compute(id, rounds)))
}
fn mode_from(s: &str) -> Option<Mode> {
    match s {
        "observe" => Some(Mode::Observe),
        "recommend" => Some(Mode::Recommend),
        "enforce" => Some(Mode::Enforce),
        _ => None,
    }
}
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "guard-http-consumer <observe|recommend|enforce|fixed8> <port>");
    let fixed8 = args[1] == "fixed8";
    let mode = if fixed8 {
        Mode::Observe
    } else {
        mode_from(&args[1]).expect("invalid mode")
    };
    let guard = Arc::new(Guard::new(GuardConfig {
        mode,
        upward_cooldown: Duration::ZERO, // phase changes are orchestrated and explicit
        max_observation_age: Duration::from_secs(30),
        ..GuardConfig::default()
    }).unwrap());
    let source = Arc::new(ResourceState::new());
    let gate = if fixed8 {
        AdmissionGate::new(8).unwrap()
    } else {
        guard.gate()
    };
    let server = Arc::new(Server::http(format!("127.0.0.1:{}", args[2]))
        .expect("HTTP listen"));
    let running = Arc::new(AtomicBool::new(true));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let server = Arc::clone(&server);
        let gate = gate.clone();
        let guard = Arc::clone(&guard);
        let source = Arc::clone(&source);
        let running = Arc::clone(&running);
        threads.push(thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                let Some(request) = server.recv_timeout(Duration::from_millis(30))
                    .expect("HTTP receive")
                else {
                    continue;
                };
                let path = request.url().to_string();
                if path == "/event" {
                    // External caller explicitly declares a resource-change event.
                    // No implicit per-request OS reads.
                    source.invalidate();
                    let result = source.refresh();
                    let decision = guard.evaluate(&source);
                    let response = format!(
                        "refresh_ok={} mode={mode:?} known_cpu={:?} active_limit={} recommendation={:?} freshness={:?} reason={:?}",
                        result.is_ok(), decision.known_cpu_equivalents,
                        if fixed8 { 8 } else { decision.active_limit },
                        decision.recommended_limit, decision.freshness, decision.reason
                    );
                    request.respond(Response::from_string(response)).ok();
                } else if path == "/health" {
                    let message = format!(
                        "healthy=1 mode={mode:?} active_limit={} in_flight={}",
                        gate.limit(), gate.in_flight()
                    );
                    request.respond(Response::from_string(message)).ok();
                } else if path == "/shutdown" {
                    // Used only by the isolated test controller.
                    running.store(false, Ordering::Relaxed);
                    request.respond(Response::from_string("bye")).ok();
                } else if path.starts_with("/work/") {
                    match gate.acquire_timeout(Duration::from_secs(6)) {
                        Some(_permit) => {
                            let (body, status) = match execute(&path) {
                                Some(body) => (body, 200),
                                None => (String::from("invalid"), 400),
                            };
                            request.respond(Response::from_string(body).with_status_code(status)).ok();
                        }
                        None => {
                            request.respond(
                                Response::from_string("admission timeout").with_status_code(503)
                            ).ok();
                        }
                    }
                } else {
                    request.respond(Response::from_string("not found").with_status_code(404)).ok();
                }
            }
        }));
    }
    println!("GUARD_HTTP_READY mode={} port={}", args[1], args[2]);
    for join in threads {
        join.join().unwrap();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handler_correctness_and_bounds() {
        assert_eq!(execute("/work/33/500"), execute("/work/33/500"));
        assert_ne!(execute("/work/33/500"), execute("/work/34/500"));
        assert!(execute("/work/33/0").is_none());
        assert!(execute("/wrong/0/100").is_none());
    }
}
