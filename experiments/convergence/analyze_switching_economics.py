#!/usr/bin/env python3
import argparse
import importlib.util
import json
import math
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "adaptation", HERE / "analyze_adaptation_economics.py"
)
ADAPTATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ADAPTATION)

DEFAULT_HORIZONS = (10, 100, 1_000, 10_000)
DEFAULT_SWITCH_FACTORS = (0.0, 0.25, 0.5, 1.0, 2.0)


def candidate_gain_per_call(row):
    return max(0.0, float(row["realized_execution_gain_per_call_ns"]))


def confidence_proxy(row):
    value = row.get("near_boundary_preference_consistency_min")
    if value is None:
        return 0.0
    return max(0.0, min(1.0, float(value)))


def simulate_regime(row, horizons, switch_factors, verify_calls):
    revalidation = float(row["revalidation_cost_ns"])
    gain_per_call = candidate_gain_per_call(row)
    confidence = confidence_proxy(row)
    boundary_changed = row.get("published_boundary") != row.get("start_boundary")

    result = {
        "start_boundary": row.get("start_boundary"),
        "published_boundary": row.get("published_boundary"),
        "oracle_boundary": row.get("oracle_boundary"),
        "status": row.get("status"),
        "candidate_gain_per_call_ns": gain_per_call,
        "confidence_proxy": confidence,
        "boundary_changed": boundary_changed,
        "switch_sensitivity": {},
    }

    for factor in switch_factors:
        switch_cost = revalidation * factor if boundary_changed else 0.0
        verify_cost = gain_per_call * verify_calls
        rollback_cost = switch_cost
        expected_rollback = (1.0 - confidence) * rollback_cost

        policy_cost = {
            "revalidation_ns": revalidation,
            "switch_ns": switch_cost,
            "verify_ns": verify_cost,
            "expected_rollback_ns": expected_rollback,
        }
        upfront = sum(policy_cost.values())

        horizons_out = {}
        for horizon in horizons:
            gross = gain_per_call * horizon

            always_net = gross - revalidation - switch_cost

            break_even_action = gain_per_call > 0.0 and gross > revalidation + switch_cost
            break_even_net = always_net if break_even_action else 0.0

            verify_gate = (
                gain_per_call > 0.0
                and gross > upfront
            )
            verify_net = gross - upfront if verify_gate else 0.0

            confidence_gate = (
                confidence >= 0.5
                and gain_per_call > 0.0
                and gross * confidence > upfront
            )
            confidence_net = gross - upfront if confidence_gate else 0.0

            horizons_out[str(horizon)] = {
                "gross_candidate_gain_ns": gross,
                "always_adapt_net_ns": always_net,
                "break_even_action": break_even_action,
                "break_even_net_ns": break_even_net,
                "verify_action": verify_gate,
                "verify_net_ns": verify_net,
                "confidence_verify_action": confidence_gate,
                "confidence_verify_net_ns": confidence_net,
            }

        result["switch_sensitivity"][str(factor)] = {
            "switch_cost_factor": factor,
            "costs": policy_cost,
            "horizons": horizons_out,
        }

    return result


def analyze(root, horizons, switch_factors, verify_calls):
    records = ADAPTATION.load_directory(root)
    summary = ADAPTATION.analyze_main(records)
    return {
        "schema_version": 1,
        "model": {
            "horizons": list(horizons),
            "switch_cost_factors_of_revalidation": list(switch_factors),
            "verify_calls": verify_calls,
            "confidence_proxy": "minimum paired preference consistency near oracle boundary",
            "warning": (
                "Research-only sensitivity model. Switch/verification costs are not "
                "production constants until directly measured."
            ),
        },
        "static_boundary": summary["static_boundary"],
        "regimes": {
            regime: simulate_regime(row, horizons, switch_factors, verify_calls)
            for regime, row in summary["regimes"].items()
        },
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("evidence_root", type=pathlib.Path)
    parser.add_argument(
        "--verify-calls", type=int, default=3,
        help="research-only verification window size",
    )
    parser.add_argument(
        "--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS)
    )
    parser.add_argument(
        "--switch-factors",
        type=float,
        nargs="+",
        default=list(DEFAULT_SWITCH_FACTORS),
        help="switch cost as a multiple of measured revalidation cost",
    )
    args = parser.parse_args()

    if args.verify_calls < 0:
        raise SystemExit("--verify-calls must be non-negative")
    if any(value <= 0 for value in args.horizons):
        raise SystemExit("--horizons must be positive")
    if any(value < 0 for value in args.switch_factors):
        raise SystemExit("--switch-factors must be non-negative")

    output = analyze(
        args.evidence_root,
        tuple(args.horizons),
        tuple(args.switch_factors),
        args.verify_calls,
    )
    target = args.evidence_root / "switching-economics.json"
    target.write_text(json.dumps(ADAPTATION.json_safe(output), indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
