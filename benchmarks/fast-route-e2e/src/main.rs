use std::hint::black_box;
use std::os::raw::{c_int, c_uint};
use std::time::Instant;

use runtime_broker::{BrokerCapacity, BrokerRequest};
use runtime_core::BackendKind;
use runtime_policy_cache::ExecutionPolicyCache;
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

#[repr(C)]
#[derive(Clone, Copy)]
struct ExactRouteEntry {
    key: u64,
    route: u8,
    _padding: [u8; 7],
}

unsafe extern "C" {
    fn alpencat_fast_route_exact_cached_probe8(
        table: *const ExactRouteEntry,
        table_size: usize,
        task_class: c_uint,
        work_items: usize,
    ) -> c_int;

    fn alpencat_fast_route_exact_publish_probe8(
        table: *mut ExactRouteEntry,
        table_size: usize,
        task_class: c_uint,
        work_items: usize,
        route: c_int,
    );

    fn alpencat_axpy_f32(
        n: usize,
        a: f32,
        x: *const f32,
        y: *mut f32,
    );
}

fn backend() -> BackendTelemetrySnapshot {
    BackendTelemetrySnapshot {
        in_flight: 0,
        completed: 0,
        failed: 0,
        work_items: 0,
        elapsed_nanos: 0,
        last_work_items: 0,
        last_elapsed_nanos: 0,
    }
}

fn telemetry() -> RuntimeTelemetrySnapshot {
    RuntimeTelemetrySnapshot {
        serial: backend(),
        cpu: backend(),
        gpu: backend(),
    }
}

fn route_code(backend: BackendKind) -> c_int {
    match backend {
        BackendKind::Serial => 0,
        BackendKind::Cpu => 1,
        BackendKind::Gpu => 2,
    }
}

fn iters_for(n: usize) -> usize {
    let target_elements = 20_000_000usize;
    (target_elements / n).clamp(20_000, 2_000_000)
}

fn bench_kernel_only(n: usize, iters: usize, x: &[f32], y: &mut [f32]) -> f64 {
    let start = Instant::now();
    for _ in 0..iters {
        unsafe {
            alpencat_axpy_f32(
                black_box(n),
                black_box(1.000_001),
                black_box(x.as_ptr()),
                black_box(y.as_mut_ptr()),
            );
        }
    }
    start.elapsed().as_nanos() as f64 / iters as f64
}

fn bench_rust_cache(n: usize, iters: usize, x: &[f32], y: &mut [f32]) -> f64 {
    let cache = ExecutionPolicyCache::default();
    let capacity = BrokerCapacity::new(8, 1);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };
    let snapshot = telemetry();

    let _ = cache.resolve_with(
        "axpy-f32",
        n,
        snapshot,
        capacity,
        request,
        || Some(BackendKind::Cpu),
    );

    let start = Instant::now();
    for _ in 0..iters {
        let policy = cache
            .resolve_with(
                black_box("axpy-f32"),
                black_box(n),
                snapshot,
                capacity,
                request,
                || Some(BackendKind::Serial),
            )
            .expect("cached policy");
        black_box(policy.backend);

        unsafe {
            alpencat_axpy_f32(
                black_box(n),
                black_box(1.000_001),
                black_box(x.as_ptr()),
                black_box(y.as_mut_ptr()),
            );
        }
    }
    start.elapsed().as_nanos() as f64 / iters as f64
}

fn bench_c_fast_route(n: usize, iters: usize, x: &[f32], y: &mut [f32]) -> f64 {
    const TABLE_SIZE: usize = 256;
    let mut table = vec![
        ExactRouteEntry {
            key: 0,
            route: 0,
            _padding: [0; 7],
        };
        TABLE_SIZE
    ];

    unsafe {
        alpencat_fast_route_exact_publish_probe8(
            table.as_mut_ptr(),
            TABLE_SIZE,
            1,
            n,
            route_code(BackendKind::Cpu),
        );
    }

    let start = Instant::now();
    for _ in 0..iters {
        let route = unsafe {
            alpencat_fast_route_exact_cached_probe8(
                black_box(table.as_ptr()),
                TABLE_SIZE,
                black_box(1),
                black_box(n),
            )
        };
        black_box(route);

        unsafe {
            alpencat_axpy_f32(
                black_box(n),
                black_box(1.000_001),
                black_box(x.as_ptr()),
                black_box(y.as_mut_ptr()),
            );
        }
    }
    start.elapsed().as_nanos() as f64 / iters as f64
}

fn main() {
    println!("# FastRoute end-to-end native-kernel benchmark");
    println!("| n | iters | kernel_ns | rust_cache_ns | c_fast_ns | c_vs_rust_speedup | c_total_gain |");
    println!("|---:|---:|---:|---:|---:|---:|---:|");

    let mut checksum = 0.0f32;

    for n in [8usize, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
        let iters = iters_for(n);
        let x = vec![0.25f32; n];
        let mut y_kernel = vec![1.0f32; n];
        let mut y_rust = vec![1.0f32; n];
        let mut y_c = vec![1.0f32; n];

        let kernel_ns = bench_kernel_only(n, iters, &x, &mut y_kernel);
        let rust_ns = bench_rust_cache(n, iters, &x, &mut y_rust);
        let c_ns = bench_c_fast_route(n, iters, &x, &mut y_c);

        let speedup = rust_ns / c_ns;
        let total_gain = 100.0 * (rust_ns - c_ns) / rust_ns;

        checksum += y_kernel[0] + y_rust[0] + y_c[0];

        println!(
            "| {n} | {iters} | {kernel_ns:.2} | {rust_ns:.2} | {c_ns:.2} | {speedup:.3}x | {total_gain:.2}% |"
        );
        println!(
            "e2e n={n} iters={iters} kernel_ns={kernel_ns:.4} rust_cache_ns={rust_ns:.4} c_fast_ns={c_ns:.4} speedup={speedup:.6} gain_pct={total_gain:.4}"
        );
    }

    println!("checksum={checksum}");
}
