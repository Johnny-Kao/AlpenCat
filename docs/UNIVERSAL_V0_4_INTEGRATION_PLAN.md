# Universal v0.4 Feature-Complete Alpha — Integration Gate

Status: implementation target, NOT completed. v0.1 preview remains immutable at tag `v0.1.0-preview`.

## Objective
Build the complete end-to-end runtime and SDK first, then optimize only after representative end-to-end evidence exists. No separate upstream PR and no premature production-readiness claim.

## Build sequence (one integrated delivery)
1. **v0.2 — measurement and lifecycle**: real comparable serial/CPU calibration; epoch-bound boundary invalidation; lazy bounded revalidation; explicit safe fallback; no global inference from local measurements.
2. **v0.3 — resource-event adapters**: portable application adapter plus OS-supported resource-change observation; adapter lifecycle, start/stop, event coalescing, recovery. Hardware-specific HFI/CPPC integration is optional unless a verified supported interface exists; never claim native hardware coverage from polling-only tests.
3. **v0.4 — public SDK integration**: minimal external Rust consumer; documented API and examples; diagnostics; graceful degradation on unsupported OS/hardware; thread-safe shutdown and bounded memory/work.
4. **Full integration gate**: Linux x64/ARM64, macOS Apple Silicon, Windows; single-thread and multithread; concurrent publication/reads; resource transition/recovery; long-running stress; fault injection; deterministic correctness checks and external consumer compilation.

## Acceptance criteria
- Correct results vs serial reference; no unbounded retries, blocking, allocations or work in the fast path.
- Resource change reaches epoch and triggers appropriate lazy local revalidation without unsafe extrapolation.
- Unsupported event source falls back safely to explicit notifications; no reliance on privileged APIs.
- External example builds and executes using pinned repository dependency.
- Cross-platform CI is green; all test URLs and measured limitations are recorded.
- Measure routing overhead, event overhead, revalidation cost and net benefit against fixed serial and fixed CPU. **Do not tune performance before the full system works.**

## Release discipline
Keep v0.1 tag unchanged. Integrate v0.2–v0.4 on the research branch with small correctness gates and one final full sign-off. Do not label v0.4 complete until all required evidence exists. No crates.io or production release without a separate decision.

## Evidence baseline
- v0.1 Release Gate: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38479856600
- Convergence Smoke: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38479844523
- Linux OS E2E: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38478288555
