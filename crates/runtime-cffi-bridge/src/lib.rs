//! C ABI bridge for experimental CFFI integration benchmarks.
//!
//! This is a control-plane adapter only. CFFI retains ownership of Python
//! conversion semantics and ffi_call; the bridge owns route selection.

use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const ROUTE_LEGACY: i32 = 0;
const ROUTE_FAST: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Replace,
    Shadow,
    Backup,
    Bypass,
}

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match env::var("CFFI_SWISS_MODE").ok().as_deref() {
        Some("shadow") => Mode::Shadow,
        Some("backup") => Mode::Backup,
        Some("bypass") => Mode::Bypass,
        _ => Mode::Replace,
    })
}

static ROUTE_CALLS: AtomicU64 = AtomicU64::new(0);
static FAST_ROUTES: AtomicU64 = AtomicU64::new(0);
static LEGACY_ROUTES: AtomicU64 = AtomicU64::new(0);
static SHADOW_CHECKS: AtomicU64 = AtomicU64::new(0);
static SHADOW_MISMATCHES: AtomicU64 = AtomicU64::new(0);
static BACKUP_FALLBACKS: AtomicU64 = AtomicU64::new(0);

fn backup_fail_every() -> u64 {
    static FAIL_EVERY: OnceLock<u64> = OnceLock::new();
    *FAIL_EVERY.get_or_init(|| {
        env::var("CFFI_SWISS_FAIL_EVERY")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(0)
    })
}

#[no_mangle]
pub extern "C" fn runtime_cffi_route(fast_candidate: i32) -> i32 {
    ROUTE_CALLS.fetch_add(1, Ordering::Relaxed);
    let host_fast = fast_candidate != 0;

    let route = match mode() {
        Mode::Bypass => ROUTE_LEGACY,
        Mode::Shadow => {
            if host_fast {
                ROUTE_FAST
            } else {
                ROUTE_LEGACY
            }
        }
        Mode::Replace => {
            if host_fast {
                ROUTE_FAST
            } else {
                ROUTE_LEGACY
            }
        }
        Mode::Backup => {
            let fail_every = backup_fail_every();
            let call = ROUTE_CALLS.load(Ordering::Relaxed);
            if host_fast && fail_every > 0 && call % fail_every == 0 {
                BACKUP_FALLBACKS.fetch_add(1, Ordering::Relaxed);
                ROUTE_LEGACY
            } else if host_fast {
                ROUTE_FAST
            } else {
                ROUTE_LEGACY
            }
        }
    };

    if route == ROUTE_FAST {
        FAST_ROUTES.fetch_add(1, Ordering::Relaxed);
    } else {
        LEGACY_ROUTES.fetch_add(1, Ordering::Relaxed);
    }
    route
}

#[no_mangle]
pub extern "C" fn runtime_cffi_route_calls() -> u64 {
    ROUTE_CALLS.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn runtime_cffi_fast_routes() -> u64 {
    FAST_ROUTES.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn runtime_cffi_legacy_routes() -> u64 {
    LEGACY_ROUTES.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn runtime_cffi_shadow_active() -> i32 {
    i32::from(matches!(mode(), Mode::Shadow))
}

#[no_mangle]
pub extern "C" fn runtime_cffi_shadow_record(mismatch: i32) {
    SHADOW_CHECKS.fetch_add(1, Ordering::Relaxed);
    if mismatch != 0 {
        SHADOW_MISMATCHES.fetch_add(1, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn runtime_cffi_shadow_checks() -> u64 {
    SHADOW_CHECKS.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn runtime_cffi_shadow_mismatches() -> u64 {
    SHADOW_MISMATCHES.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn runtime_cffi_backup_fallbacks() -> u64 {
    BACKUP_FALLBACKS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_counters_are_coherent() {
        let before = runtime_cffi_route_calls();
        let _ = runtime_cffi_route(1);
        assert_eq!(runtime_cffi_route_calls(), before + 1);
        assert_eq!(
            runtime_cffi_fast_routes() + runtime_cffi_legacy_routes(),
            runtime_cffi_route_calls()
        );
    }
}
