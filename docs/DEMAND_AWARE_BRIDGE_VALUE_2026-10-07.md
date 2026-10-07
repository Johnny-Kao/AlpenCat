# Demand-Aware Bridge Value — 2026-10-07

> Scope: Generic policy research
> Runtime hot-path change: none
> Status: candidate estimator under falsification

## Motivation

Two simpler observed-only EVSI proxies exposed opposite failure modes:

- call-mass exposure was too conservative;
- work-mass exposure was too aggressive.

The W2 p3 -> p4 -> p5 shape suggests a more specific explanation:

- p3 reaches an observed edge;
- the next geometric sentinel may contain no actual demand mass;
- that sentinel can still have option value because it bridges to the next real demand region;
- therefore the cost of continuing should be compared with the value reachable after crossing the bridge, not with the immediate value of one empty geometric step.

## Generic estimator

For one-sided evidence:

1. Find the farthest observed sentinel in the preferred direction.
2. Find the nearest unresolved point in the known/observed demand support.
3. Count geometric sentinel steps required to reach that demand point.
4. Estimate bridge cost as:
   ```
   bridge_cost = estimated_probe_cost_per_step * bridge_steps
   ```
5. Build a conservative lower envelope for wrong-route regret density from already observed sentinels:
   ```
   regret_density_lb =
       min(abs(serial_cost - cpu_cost) / work_items)
   ```
6. Estimate the value at the next unresolved demand point:
   ```
   target_regret_lb =
       regret_density_lb * target_work_items

   gross_target_value =
       direction_consistency
       * target_call_fraction
       * target_regret_lb
       * expected_remaining_calls
   ```
7. Continue only when:
   ```
   gross_target_value > bridge_cost
   ```

## Why this is different from work-mass EVSI

Work-mass EVSI prices the entire unresolved region at once and can overvalue large requests.

Bridge value instead asks:

> How much does it cost to reach the next decision-relevant demand support, and is the conservatively estimated value at that support sufficient to pay for the whole bridge?

This naturally assigns option value to an intermediate sentinel that has no immediate demand, without treating all unresolved work as already recoverable.

## Current W2 retrospective check

Using the previous authoritative W2 artifact and only p3-observed sentinel measurements:

- contention:
  - 100 calls: negative bridge value -> stop
  - 1,000 calls: positive bridge value -> continue
  - 10,000 calls: positive bridge value -> continue
- half:
  - 100 calls: negative bridge value -> stop
  - 1,000 calls: positive bridge value -> continue
  - 10,000 calls: positive bridge value -> continue

The next unresolved W2 demand support is 4,194,304 work items while p3 observes through 1,048,576, so the bridge contains two geometric probe steps.

This retrospective result is encouraging but is not yet sufficient evidence.

## Required falsification

The same estimator must be tested on W3.

W3 is a negative control because boundary movement exists but measured recoverable regret is approximately zero under the observed demand weighting.

A generic bridge estimator should not interpret mere boundary movement as permission to buy more observations.

If W3 over-explores, likely causes include:

- regret-density extrapolation is too optimistic;
- demand support alone is insufficient;
- direction consistency is not enough to bound decision value.

In that case the next candidate should use explicit confidence/value bounds or opportunistic evidence rather than adding tuned constants.

## Portability constraint

The estimator must remain vendor-agnostic.

Allowed inputs:
- observed route timings;
- observed/known demand support and weights;
- observed search edge;
- measured probe cost;
- expected horizon;
- resource-state class.

Forbidden in the Generic layer:
- CPU vendor/model;
- model-specific probe budgets;
- Intel/AMD/Apple thresholds.

## Current decision

Do not implement this estimator in Runtime Core yet.

Gate for further consideration:

1. W2 positive-case behavior is reproduced on fresh CI evidence.
2. W3 negative control remains conservative.
3. W1 low-opportunity control remains conservative or is explicitly tested next.
4. Cross-runner replication is performed before any hot-path integration.
