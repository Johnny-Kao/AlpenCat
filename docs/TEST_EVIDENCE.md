# Validation evidence and test inventory

## Released v0.4.0-alpha: signed-off evidence

| Check | Result | Run |
| --- | --- | --- |
| Five-platform API + SDK integration gate | **PASS** | [#38575886948](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575886948) |
| Convergence Smoke, Rust checks and economics analysis | **PASS** | [#38575887316](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575887316) |

Both runs correspond to release commit `f775b528dca13024c915bcde698f7fafa04e7f5d`, not merely a nearby branch head. The five platform jobs and aggregate gate succeeded:

| Runner | Job |
| --- | --- |
| Linux x64 | `runtime-linux-x64` — PASS |
| Linux ARM64 | `runtime-linux-arm64` — PASS |
| macOS Apple Silicon | `runtime-macos-apple-silicon` — PASS |
| macOS Intel | `runtime-macos-intel` — PASS |
| Windows x64 | `runtime-windows-x64` — PASS |
| Aggregate | `universal-v04-gate` — PASS |

### What was executed

Each integration platform ran `cargo fmt --all -- --check`, `cargo clippy -p runtime-api --all-targets -- -D warnings`, `cargo test -p runtime-api -- --nocapture`, the elapsed SDK and universal quickstart examples, `cargo run --manifest-path tests/external-consumer/Cargo.toml`, and the `v04_stress` test. The smoke workflow additionally checks Rust convergence and executes the E0/W2 evidence matrix and associated Python analyzers. Exact steps and artifacts are in the two GitHub Actions runs.

- Independent consumer: `tests/external-consumer/src/main.rs` exercises public API, initial calibration, notification, revalidation and result equality.
- Concurrent stress: `crates/runtime-api/tests/v04_stress.rs` creates four workers × 250 iterations, with routing, notifications and bounded revalidation.
- Notification tests: `crates/runtime-api/tests/resource_notifications.rs` checks epoch transitions, concurrent notifications and Linux watcher stop semantics.
- Initial calibration tests: `crates/runtime-api/tests/initial_calibration.rs` checks publication, missing CPU route and invalidation during calibration.
- Real Linux affinity transition research: `crates/runtime-api/tests/linux_affinity_adapter.rs`; hardware/privilege preconditions affect whether a real OS transition can run on any particular host.

## Older frozen evidence — different revisions

| Version / focus | Evidence |
| --- | --- |
| v0.1 preview release | [#38479856600](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38479856600) |
| Production-core 12-job concurrency matrix | [#37186182010](https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186182010) |
| Local no-extrapolation economics | [#37186607338](https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186607338) |
| x64 production hot-path measurement | [#37186703335](https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186703335) |

Older historical counts and timing ranges are detailed in [Validation Status](VALIDATION_STATUS.md) and are **not** measurements of the v0.4 release.

## Reproduce

```bash
cargo fmt --all -- --check
cargo clippy -p runtime-api --all-targets -- -D warnings
cargo test -p runtime-api
cargo run -p runtime-api --example elapsed_sdk
cargo run --manifest-path tests/external-consumer/Cargo.toml
cargo test -p runtime-api --test v04_stress
```

## Interpretation

PASS means the described tests completed successfully on the referenced revision. It does **not** imply every system event, GPU device, CPU architecture, long-running workload or performance distribution was verified. Elapsed measurements are workload-dependent; they may not produce an observed crossover. Native Intel HFI/AMD CPPC integration and field performance economics remain unvalidated.
