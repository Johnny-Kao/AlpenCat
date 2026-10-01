//! Replay measured FIR costs through the real M12 model; not a GPU adapter.
use runtime_core::BackendKind;
use runtime_cost_model::{CostModelContext, OnlineCostModel};
use runtime_machine::{GpuDeviceProfile, HostProfile, MachineProfile};
use runtime_selector::CalibrationProfile;
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};
use std::{env, fs, time::Duration};

#[derive(Clone, Copy)]
struct Row {
    n: usize,
    taps: usize,
    cpu: u64,
    gpu: u64,
}

fn empty() -> BackendTelemetrySnapshot {
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
fn main() {
    let path = env::args().nth(1).expect("CSV path");
    let rows: Vec<Row> = fs::read_to_string(path)
        .expect("CSV")
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let columns: Vec<&str> = line.split(',').collect();
            Row {
                n: columns[0].parse().expect("n"),
                taps: columns[1].parse().expect("taps"),
                cpu: columns[2].parse::<f64>().expect("CPU ns").round() as u64,
                gpu: columns[3].parse::<f64>().expect("GPU ns").round() as u64,
            }
        })
        .collect();
    assert_eq!(rows.len(), 15);
    let machine = MachineProfile {
        host: HostProfile {
            os: "macos",
            architecture: "aarch64",
            logical_cpus: 10,
            memory_total_bytes: None,
            memory_available_bytes: None,
        },
        gpus: vec![GpuDeviceProfile {
            name: "Apple M5".into(),
            backend: "Metal".into(),
            device_type: "IntegratedGpu".into(),
            vendor_id: None,
            device_id: None,
            dedicated_memory_bytes: None,
        }],
    };
    let telemetry = RuntimeTelemetrySnapshot {
        serial: empty(),
        cpu: empty(),
        gpu: empty(),
    };
    let context = CostModelContext {
        cpu_eligible: true,
        gpu_eligible: true,
        machine: &machine,
        telemetry,
        bootstrap: CalibrationProfile::default(),
    };
    let mut replay_correct = 0;
    let mut heldout_correct = 0;
    println!("n,taps,actual,initial,replan_with_cpu_preference,leave_one_size_out,confidence");
    for row in &rows {
        let model = OnlineCostModel::default();
        let heldout = OnlineCostModel::default();
        for sample in rows.iter().filter(|sample| sample.taps == row.taps) {
            for (backend, nanos) in [
                (BackendKind::Cpu, sample.cpu),
                (BackendKind::Gpu, sample.gpu),
            ] {
                model.observe("fir", backend, sample.n, Duration::from_nanos(nanos), true);
                if sample.n != row.n {
                    heldout.observe("fir", backend, sample.n, Duration::from_nanos(nanos), true);
                }
            }
        }
        let actual = if row.cpu <= row.gpu {
            BackendKind::Cpu
        } else {
            BackendKind::Gpu
        };
        let initial = model.decide("fir", row.n, context);
        let replan = model.decide_with_preference("fir", row.n, context, Some(BackendKind::Cpu));
        let unseen = heldout.decide("fir", row.n, context);
        replay_correct += usize::from(initial.backend == actual);
        heldout_correct += usize::from(unseen.backend == actual);
        println!(
            "{},{},{:?},{:?},{:?},{:?},{:.4}",
            row.n,
            row.taps,
            actual,
            initial.backend,
            replan.backend,
            unseen.backend,
            initial.confidence
        );
    }
    println!("measured_shape_replay={replay_correct}/15");
    println!("leave_one_size_out={heldout_correct}/15");
}
