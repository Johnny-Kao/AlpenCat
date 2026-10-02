use std::hint::black_box;
use std::os::raw::{c_int, c_uint};
use std::time::Instant;

#[repr(C)]
struct FastPolicy {
    serial_max: usize,
    gpu_min: usize,
    required_gpu_state: c_uint,
}

#[link(name = "alpencat_fast_route", kind = "static")]
extern "C" {
    fn alpencat_fast_route(
        policy: *const FastPolicy,
        work_items: usize,
        state: c_uint,
    ) -> c_int;
    fn alpencat_fast_route_constant() -> c_int;
    fn alpencat_fast_route_cached(table: *const u8, work_items: usize) -> c_int;
}

fn main() {
    let iterations = 50_000_000usize;
    let policy = FastPolicy {
        serial_max: 1024,
        gpu_min: 262_144,
        required_gpu_state: 0,
    };
    let mut table = [3u8; usize::BITS as usize];
    for (bucket, value) in table.iter_mut().enumerate() {
        *value = if bucket <= 10 { 0 } else if bucket < 18 { 1 } else { 2 };
    }

    let mut sink: i64 = 0;

    let start = Instant::now();
    for _ in 0..iterations {
        unsafe {
            sink += black_box(alpencat_fast_route_constant()) as i64;
        }
    }
    let constant = start.elapsed().as_nanos() as f64 / iterations as f64;

    let start = Instant::now();
    for _ in 0..iterations {
        unsafe {
            sink += black_box(alpencat_fast_route(
                &policy,
                black_box(65_536),
                black_box(1 | 2),
            )) as i64;
        }
    }
    let threshold = start.elapsed().as_nanos() as f64 / iterations as f64;

    let start = Instant::now();
    for i in 0..iterations {
        let size = 1usize << ((i % 21) as u32);
        unsafe {
            sink += black_box(alpencat_fast_route_cached(
                table.as_ptr(),
                black_box(size),
            )) as i64;
        }
    }
    let cached = start.elapsed().as_nanos() as f64 / iterations as f64;

    println!("rust_to_c_constant_ns={constant:.4}");
    println!("rust_to_c_threshold_ns={threshold:.4}");
    println!("rust_to_c_cached_ns={cached:.4}");
    println!("sink={sink}");
}
