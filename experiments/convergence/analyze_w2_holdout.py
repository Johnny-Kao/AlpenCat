#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib
import statistics

HORIZONS = (1_000, 10_000)
CONSISTENCY_THRESHOLDS = (0.70, 0.85, 1.00)
MARGIN_THRESHOLDS = (0.0, 5.0, 10.0)


def median(values):
    return float(statistics.median(values))


def load_jsonl(path):
    return [
        json.loads(line)
        for line in path.read_text().splitlines()
        if line.strip()
    ]


def route_cost(row, boundary):
    serial = median(row["serial_samples_ns"])
    cpu = row.get("cpu_samples_ns")
    if row["work_items"] <= boundary or cpu is None:
        return serial
    return median(cpu)


def oracle_cost(row):
    serial = median(row["serial_samples_ns"])
    cpu = row.get("cpu_samples_ns")
    return serial if cpu is None else min(serial, median(cpu))


def holdout_cycles(root, regime, static_boundary, candidate_boundary):
    rows = [
        row
        for row in load_jsonl(root / f"{regime}.jsonl")
        if row.get("record_type") == "point"
    ]
    if not rows:
        raise SystemExit(f"missing holdout points for {regime}")

    total_weight = sum(float(row.get("weight", 1.0)) for row in rows)
    static_cycle = sum(
        route_cost(row, static_boundary) * float(row.get("weight", 1.0))
        for row in rows
    )
    candidate_cycle = sum(
        route_cost(row, candidate_boundary) * float(row.get("weight", 1.0))
        for row in rows
    )
    oracle_cycle = sum(
        oracle_cost(row) * float(row.get("weight", 1.0))
        for row in rows
    )
    return total_weight, static_cycle, candidate_cycle, oracle_cycle


def discover(root):
    cases = []
    for escalation in sorted(root.glob("**/train/escalation-policy.json")):
        replica_root = escalation.parent.parent
        train = json.loads(escalation.read_text())
        holdout = replica_root / "holdout"
        if not holdout.exists():
            raise SystemExit(f"missing holdout directory under {replica_root}")

        manifest = replica_root / "train" / "manifest.txt"
        cpu = "unknown"
        if manifest.exists():
            for line in manifest.read_text().splitlines():
                if "Model name:" in line:
                    cpu = line.split("Model name:", 1)[1].strip()
                    break

        static_boundary = train["static_boundary"]
        for row in train["rows"]:
            regime = row["regime"]
            total_weight, static_cycle, candidate_cycle, oracle_cycle = (
                holdout_cycles(
                    holdout,
                    regime,
                    static_boundary,
                    row["candidate_boundary"],
                )
            )
            cases.append(
                {
                    "replica": replica_root.name,
                    "cpu": cpu,
                    "regime": regime,
                    "static_boundary": static_boundary,
                    "candidate_boundary": row["candidate_boundary"],
                    "train_break_even_calls": row.get("break_even_calls"),
                    "train_sentinel_winner": row["sentinel_winner"],
                    "train_sentinel_consistency": row[
                        "sentinel_consistency"
                    ],
                    "train_sentinel_margin_pct": row[
                        "sentinel_margin_pct"
                    ],
                    "train_candidate_capture_fraction": row.get(
                        "candidate_capture_fraction", 0.0
                    ),
                    "probe_cost_ns": row["total_probe_cost_ns"],
                    "holdout_weight": total_weight,
                    "holdout_static_cycle_ns": static_cycle,
                    "holdout_candidate_cycle_ns": candidate_cycle,
                    "holdout_oracle_cycle_ns": oracle_cycle,
                }
            )
    if not cases:
        raise SystemExit("no holdout replica evidence found")
    return cases


def evaluate(case, horizon, consistency_threshold, margin_threshold):
    break_even = case["train_break_even_calls"]
    act = (
        break_even is not None
        and break_even <= horizon
        and case["train_candidate_capture_fraction"] > 0.0
        and case["train_sentinel_consistency"] >= consistency_threshold
        and case["train_sentinel_margin_pct"] >= margin_threshold
    )

    static_per_call = (
        case["holdout_static_cycle_ns"] / case["holdout_weight"]
    )
    candidate_per_call = (
        case["holdout_candidate_cycle_ns"] / case["holdout_weight"]
    )
    oracle_per_call = (
        case["holdout_oracle_cycle_ns"] / case["holdout_weight"]
    )

    static_total = static_per_call * horizon
    oracle_total = oracle_per_call * horizon
    available = max(0.0, static_total - oracle_total)

    if act:
        policy_total = candidate_per_call * horizon + case["probe_cost_ns"]
    else:
        policy_total = static_total

    net = static_total - policy_total
    savings_pct = 0.0 if static_total <= 0 else 100.0 * net / static_total
    capture = (
        0.0
        if available <= 0
        else max(0.0, net) / available
    )
    return {
        "act": act,
        "net_saving_ns": net,
        "savings_pct": savings_pct,
        "available_regret_ns": available,
        "capture_fraction": capture,
    }


def analyze(cases):
    summaries = []
    details = []
    for horizon in HORIZONS:
        for consistency in CONSISTENCY_THRESHOLDS:
            for margin in MARGIN_THRESHOLDS:
                evaluated = []
                for case in cases:
                    result = evaluate(case, horizon, consistency, margin)
                    evaluated.append(result)
                    details.append(
                        {
                            "horizon": horizon,
                            "consistency_threshold": consistency,
                            "margin_threshold_pct": margin,
                            **case,
                            **result,
                        }
                    )

                total_available = sum(
                    row["available_regret_ns"] for row in evaluated
                )
                total_positive = sum(
                    max(0.0, row["net_saving_ns"]) for row in evaluated
                )
                summaries.append(
                    {
                        "horizon": horizon,
                        "consistency_threshold": consistency,
                        "margin_threshold_pct": margin,
                        "actions": sum(row["act"] for row in evaluated),
                        "positive_actions": sum(
                            row["act"] and row["net_saving_ns"] > 0
                            for row in evaluated
                        ),
                        "negative_actions": sum(
                            row["act"] and row["net_saving_ns"] < 0
                            for row in evaluated
                        ),
                        "mean_savings_pct": sum(
                            row["savings_pct"] for row in evaluated
                        )
                        / len(evaluated),
                        "min_case_savings_pct": min(
                            row["savings_pct"] for row in evaluated
                        ),
                        "available_regret_capture_fraction": (
                            0.0
                            if total_available <= 0
                            else total_positive / total_available
                        ),
                    }
                )
    return summaries, details


def write_csv(rows, path):
    if not rows:
        return
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=pathlib.Path)
    parser.add_argument(
        "--output",
        type=pathlib.Path,
        default=pathlib.Path("w2-holdout-summary"),
    )
    args = parser.parse_args()

    cases = discover(args.root)
    summaries, details = analyze(cases)
    args.output.mkdir(parents=True, exist_ok=True)

    (args.output / "holdout-policy.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "cases": cases,
                "summaries": summaries,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    write_csv(summaries, args.output / "holdout-grid.csv")
    write_csv(details, args.output / "holdout-cases.csv")

    lines = [
        "# W2 holdout policy validation",
        "",
        "Training evidence chooses the candidate and break-even. Independent holdout evidence prices the resulting action.",
        "",
        "| Horizon | Consistency | Margin | Actions | Positive | Negative | Mean saving | Worst case | Holdout regret captured |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in summaries:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row["horizon"]),
                    f"{100 * row['consistency_threshold']:.0f}%",
                    f"{row['margin_threshold_pct']:.0f}%",
                    str(row["actions"]),
                    str(row["positive_actions"]),
                    str(row["negative_actions"]),
                    f"{row['mean_savings_pct']:.3f}%",
                    f"{row['min_case_savings_pct']:.3f}%",
                    f"{100 * row['available_regret_capture_fraction']:.1f}%",
                ]
            )
            + " |"
        )
    report = "\n".join(lines) + "\n"
    (args.output / "holdout-policy.md").write_text(report)
    print(report)


if __name__ == "__main__":
    main()
