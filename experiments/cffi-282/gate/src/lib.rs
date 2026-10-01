use std::ffi::c_uint;

pub const ROUTE_LEGACY: c_uint = 0;
pub const ROUTE_FAST: c_uint = 1;
pub const ROUTE_SHADOW_FAST: c_uint = 2;
pub const ROUTE_SHADOW_LEGACY: c_uint = 3;

fn supported_plan(plan: c_uint) -> bool {
    matches!(plan, 1 | 5 | 21 | 341 | 42)
}

#[no_mangle]
pub extern "C" fn swiss_cffi_route(plan: c_uint, mode: c_uint) -> c_uint {
    match mode {
        // replacement/primary: Swiss owns the signature gate
        1 => u32::from(supported_plan(plan)),
        // shadow: caller compares Swiss support with native support
        2 => {
            if supported_plan(plan) {
                ROUTE_SHADOW_FAST
            } else {
                ROUTE_SHADOW_LEGACY
            }
        }
        // backup: same eligibility, but unsupported signatures explicitly
        // remain on the mature legacy path.
        3 => u32::from(supported_plan(plan)),
        // bypass: measure the cost of having Swiss at the junction while
        // preserving legacy execution.
        _ => ROUTE_LEGACY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_fixed_plans_route_fast() {
        for plan in [1, 5, 21, 341, 42] {
            assert_eq!(swiss_cffi_route(plan, 1), ROUTE_FAST);
            assert_eq!(swiss_cffi_route(plan, 3), ROUTE_FAST);
            assert_eq!(swiss_cffi_route(plan, 2), ROUTE_SHADOW_FAST);
        }
    }

    #[test]
    fn unsupported_plans_route_legacy() {
        for plan in [0, 2, 3, 4, 99, u32::MAX] {
            assert_eq!(swiss_cffi_route(plan, 1), ROUTE_LEGACY);
            assert_eq!(swiss_cffi_route(plan, 2), ROUTE_SHADOW_LEGACY);
            assert_eq!(swiss_cffi_route(plan, 3), ROUTE_LEGACY);
        }
    }

    #[test]
    fn bypass_always_routes_legacy() {
        for plan in [0, 1, 5, 21, 341, 42] {
            assert_eq!(swiss_cffi_route(plan, 0), ROUTE_LEGACY);
        }
    }
}
