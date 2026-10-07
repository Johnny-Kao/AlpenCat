#!/usr/bin/env python3
import argparse
import json
import math
import pathlib


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def median(values):
    xs = sorted(float(x) for x in values)
    return xs[len(xs) // 2]


def route_costs(point):
    serial = point.get("serial_samples_ns")
    cpu = point.get("cpu_samples_ns")
    if not isinstance(serial, list) or not isinstance(cpu, list) or not serial or not cpu:
        return None, None
    return median(serial), median(cpu)


def stale_regret_and_alternate_cost(point, stale_backend="Cpu"):
    serial, cpu = route_costs(point)
    if serial is None or cpu is None:
        return 0.0, 0.0
    if stale_backend == "Cpu":
        return max(0.0, cpu - serial), serial
    return max(0.0, serial - cpu), cpu


def normalized_call_fraction(point, regime_points):
    total = sum(float(row.get("weight", 1.0)) for row in regime_points)
    if total <= 0.0:
        return 0.0
    return float(point.get("weight", 1.0)) / total


def expected_calls_to_k_samples(sample_rate, samples_needed):
    if sample_rate <= 0.0:
        return math.inf
    return float(samples_needed) / sample_rate


def simulate_strategies(
    point,
    regime_points,
    horizon,
    sample_rate,
    samples_needed,
    dedicated_probe_cost_ns,
    stale_backend="Cpu",
):
    call_fraction = normalized_call_fraction(point, regime_points)
    target_calls = float(horizon) * call_fraction
    regret_per_call, alternate_cost = stale_regret_and_alternate_cost(
        point, stale_backend=stale_backend
    )

    stay_loss = target_calls * regret_per_call

    dedicated_loss = float(dedicated_probe_cost_ns)
    dedicated_net_vs_stay = stay_loss - dedicated_loss

    target_calls_to_evidence = min(
        target_calls,
        expected_calls_to_k_samples(sample_rate, samples_needed),
    )
    acquired = (
        target_calls >= expected_calls_to_k_samples(sample_rate, samples_needed)
        and samples_needed > 0
    )

    if acquired:
        opportunistic_sampling_cost = float(samples_needed) * alternate_cost
        opportunistic_stale_loss = target_calls_to_evidence * regret_per_call
    else:
        opportunistic_sampling_cost = target_calls * sample_rate * alternate_cost
        opportunistic_stale_loss = stay_loss

    opportunistic_loss = opportunistic_stale_loss + opportunistic_sampling_cost
    opportunistic_net_vs_stay = stay_loss - opportunistic_loss

    return {
        "target_call_fraction": call_fraction,
        "expected_target_calls": target_calls,
        "stale_regret_per_target_call_ns": regret_per_call,
        "alternate_route_cost_per_sample_ns": alternate_cost,
        "samples_needed": samples_needed,
        "sample_rate": sample_rate,
        "evidence_acquired": acquired,
        "expected_target_calls_to_evidence": target_calls_to_evidence,
        "stay_loss_ns": stay_loss,
        "dedicated_probe_cost_ns": dedicated_loss,
        "dedicated_net_vs_stay_ns": dedicated_net_vs_stay,
        "opportunistic_sampling_cost_ns": opportunistic_sampling_cost,
        "opportunistic_stale_loss_ns": opportunistic_stale_loss,
        "opportunistic_loss_ns": opportunistic_loss,
        "opportunistic_net_vs_stay_ns": opportunistic_net_vs_stay,
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("evidence_root", type=pathlib.Path)
    p.add_argument("--horizons", type=int, nargs="+", default=[100, 1_000, 10_000])
    p.add_argument("--sample-rates", type=float, nargs="+", default=[0.01, 0.05, 0.10])
    p.add_argument("--samples-needed", type=int, nargs="+", default=[1, 3])
    p.add_argument("--dedicated-probe-cost-ns", type=float, default=0.0)
    args = p.parse_args()

    rows = load_jsonl(args.evidence_root / "evidence.jsonl")
    by_regime = {}
    for row in rows:
        if row.get("record_type") == "point":
            by_regime.setdefault(row["regime"], []).append(row)

    out = {
        "schema_version": 2,
        "label": "opportunistic-evidence-economics-v2",
        "warning": (
            "Offline scoring uses realized route costs to compare strategy economics. "
            "Those realized costs are evaluation truth, not runtime estimator inputs."
        ),
        "cases": [],
    }
    for regime, points in sorted(by_regime.items()):
        for point in sorted(points, key=lambda x: int(x["work_items"])):
            for horizon in args.horizons:
                for rate in args.sample_rates:
                    for needed in args.samples_needed:
                        sim = simulate_strategies(
                            point,
                            points,
                            horizon,
                            rate,
                            needed,
                            args.dedicated_probe_cost_ns,
                        )
                        sim.update(
                            {
                                "regime": regime,
                                "work_items": int(point["work_items"]),
                                "horizon": horizon,
                            }
                        )
                        out["cases"].append(sim)

    target = args.evidence_root / "opportunistic-evidence.json"
    target.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
