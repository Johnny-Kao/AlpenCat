#!/usr/bin/env python3
import argparse
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "opp", HERE / "analyze_opportunistic_evidence.py"
)
OPP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OPP)


def load_json(path):
    return json.loads(path.read_text())


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def choose_strategy(row):
    losses = {
        "stay": row["stay_loss_ns"],
        "dedicated": row["dedicated_probe_cost_ns"],
        "opportunistic": row["opportunistic_loss_ns"],
    }
    return min(losses, key=losses.get)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("evidence_root", type=pathlib.Path)
    p.add_argument("--sample-rates", type=float, nargs="+", default=[0.01, 0.05, 0.10])
    p.add_argument("--samples-needed", type=int, nargs="+", default=[1, 3])
    args = p.parse_args()

    bounds = load_json(args.evidence_root / "demand-aware-bridge-bounds.json")
    evidence = load_jsonl(args.evidence_root / "evidence.jsonl")

    by_regime = {}
    for row in evidence:
        if row.get("record_type") == "point":
            by_regime.setdefault(row["regime"], []).append(row)

    out = {
        "schema_version": 1,
        "label": "unresolved-opportunistic-economics-v1",
        "warning": (
            "Offline scoring uses realized target-route timings only to evaluate strategies. "
            "Runtime policy inputs remain the prior observed-only bridge bounds."
        ),
        "cases": [],
    }

    for case in bounds.get("cases", []):
        regime = case["regime"]
        regime_points = by_regime.get(regime, [])
        for horizon_text, decision in case.get("horizons", {}).items():
            if decision.get("decision") != "unresolved":
                continue
            target_n = decision.get("target_work_items")
            if target_n is None:
                continue
            target = next(
                (row for row in regime_points if int(row["work_items"]) == int(target_n)),
                None,
            )
            if target is None:
                continue

            direction = decision.get("direction")
            stale_backend = "Cpu" if direction == "Serial" else "Serial"
            dedicated_cost = float(decision.get("estimated_bridge_cost_ns", 0.0))
            horizon = int(horizon_text)

            for rate in args.sample_rates:
                for needed in args.samples_needed:
                    row = OPP.simulate_strategies(
                        target,
                        regime_points,
                        horizon,
                        rate,
                        needed,
                        dedicated_cost,
                        stale_backend=stale_backend,
                    )
                    row.update(
                        {
                            "regime": regime,
                            "max_points": case.get("max_points"),
                            "horizon": horizon,
                            "target_work_items": int(target_n),
                            "direction": direction,
                            "bound_net_lower_ns": decision.get("net_value_lower_bound_ns"),
                            "bound_net_upper_ns": decision.get("net_value_upper_bound_ns"),
                        }
                    )
                    row["best_strategy"] = choose_strategy(row)
                    out["cases"].append(row)

    target = args.evidence_root / "unresolved-opportunistic-economics.json"
    target.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
