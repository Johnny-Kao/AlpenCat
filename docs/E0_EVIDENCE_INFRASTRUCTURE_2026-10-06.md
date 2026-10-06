# E0 Evidence Infrastructure Checkpoint — 2026-10-06

## Status

E0 is complete on branch `research/convergence-adaptive-compute-sdk`.

Authoritative validation run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37456566642
- conclusion: SUCCESS
- branch head: `af994384ffbf2e352139874f8f9238197a03f6f2`

E0 is evidence infrastructure, not a product-performance claim.

## What is now implemented

### Shared evidence schema

Machine-readable JSONL records now capture:

- workload;
- regime;
- workload size;
- demand weight;
- effective parallelism;
- raw Serial timing samples;
- raw CPU timing samples or explicit route unavailability;
- output-equivalence status;
- Auto execution timing samples;
- actual Auto backend;
- boundary stale/fresh state;
- revalidation status;
- revalidation elapsed cost;
- published boundary;
- starting boundary.

Schema:

`experiments/convergence/evidence_schema.json`

### One evidence source, four reconstructed policies

The economics analyzer reconstructs all policy comparisons from the same underlying route evidence:

1. Static;
2. Periodic/eager recalibration;
3. AlpenCat;
4. Oracle.

This avoids comparing policies from separately timed experiment runs.

Auto execution is used to prove which backend AlpenCat actually selected. Policy execution cost is priced using the same Serial/CPU raw route timings used by Static, Periodic, and Oracle; AlpenCat revalidation cost is added separately.

### Resource-regime protocol

The initial E0 matrix is:

1. `baseline-full`;
2. `half`;
3. `one`;
4. `contention`;
5. `recovery`.

The runner prints foreground progress for every regime and produces one evidence bundle.

### Bootstrap vs adaptation semantics

E0 originally exposed a harness error: every regime started from an arbitrary boundary of 32,768, forcing AlpenCat to relearn the machine from scratch.

The corrected protocol is:

```text
baseline-full
  -> one-time bootstrap
  -> baseline boundary

resource regime changes
  -> inherit previously valid boundary
  -> invalidate
  -> bounded 3-point local revalidation
  -> publish only justified local result
```

For recovery, the starting boundary is carried from the preceding contention state.

This better matches the intended runtime semantics.

## Validation coverage

The authoritative run passed:

- rustfmt;
- Clippy with warnings denied;
- full runtime-api test suite;
- economics analyzer regressions;
- E0 evidence matrix;
- evidence validation;
- policy reconstruction;
- artifact upload.

The matrix produced:

- 30 point records;
- 5 revalidation records;
- raw JSONL evidence;
- machine manifest;
- economics summary JSON;
- policy summary CSV;
- human-readable economics Markdown.

## W1 first-matrix observation

Workload:

`compute-mix-128`

Runner:

GitHub-hosted Ubuntu x64 with 4 effective CPUs at baseline.

Baseline bootstrap found:

`serial_max_items = 256`

Important: these are smoke observations on one runner, not generalized performance claims.

### Dynamic revalidation cost after carry-over fix

| Regime | Start boundary | Result | Published boundary | Revalidation cost |
|---|---:|---|---:|---:|
| half | 256 | Published | 256 | ~1.57 ms |
| one | 256 | RouteUnavailable(Cpu) | 256 stale | ~0.57 ms |
| contention | 256 | Published | 128 | ~0.89 ms |
| recovery | 128 | Published | 128 | ~0.83 ms |

The old erroneous fresh-start harness had measured roughly 65–104 ms because it forced every regime to relearn from 32,768. That result is superseded.

## First economics smoke

The analyzer used a uniform synthetic demand assumption of 100 calls per measured size. This is only an amortization smoke and must not be interpreted as fleet economics.

| Regime | AlpenCat vs static | Oracle benefit captured |
|---|---:|---:|
| half | -0.094% | 0% |
| one | -0.019% | 0% |
| contention | +0.034% | 39.2% |
| recovery | +0.072% | 42.8% |

For this W1/grid:

- Static was already equal or extremely close to Oracle in all regimes.
- Therefore there was almost no recoverable regret.
- AlpenCat correctly cannot create large savings when there is no meaningful stale-route loss to recover.
- contention/recovery exposed small positive recoverable regret;
- half/one did not justify adaptation cost.

The baseline-full row includes one-time bootstrap calibration and should not be interpreted as dynamic adaptation economics.

## Interpretation

This is a useful negative/small-positive result.

E0 has demonstrated that the evidence system can distinguish:

- no economic opportunity;
- route unavailability;
- small recoverable regret;
- revalidation cost exceeding benefit;
- revalidation cost being partially amortized by avoided regret.

The infrastructure is therefore suitable for the next decision batch.

## Next action: E1

Do not modify the core architecture first.

E1 should increase the probability of observing meaningful boundary movement rather than simply adding more random cases.

Priority:

1. keep W1 as the compute-bound control;
2. add W2 memory-sensitive;
3. add W3 mixed compute/memory;
4. retain the five resource regimes;
5. preserve the shared-evidence policy reconstruction;
6. add repeated/interleaved runs for variance control;
7. move from uniform synthetic demand weights toward a declared workload-demand distribution.

Only after E1 should CPU quota, NUMA, memory pressure, or additional cloud environments be added, unless W2/W3 require them directly.

## Current decision

E0: PASS.

W1 economic result: NEUTRAL / LOW OPPORTUNITY.

AlpenCat product thesis: NOT YET DECIDED.

The project now has the infrastructure required to let W2/W3 and stronger resource regimes decide the next step.
