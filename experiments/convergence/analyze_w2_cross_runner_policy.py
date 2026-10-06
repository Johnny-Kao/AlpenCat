#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib

HORIZONS = (100, 1_000, 10_000)
CONSISTENCY_THRESHOLDS = (0.70, 0.85, 1.00)
MARGIN_THRESHOLDS = (0.0, 5.0, 10.0)


def load_json(path):
    return json.loads(path.read_text())


def cpu_model(root):
    manifest = root / "manifest.txt"
    if not manifest.exists():
        return "unknown"
    for line in manifest.read_text().splitlines():
        if "Model name:" in line:
            return line.split("Model name:", 1)[1].strip()
    return "unknown"


def point_count(root, regime):
    path = root / f"{regime}.jsonl"
    if not path.exists():
        return 0
    count = 0
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if row.get("record_type") == "point":
            count += 1
    return count


def discover(root):
    rows = []
    for escalation in sorted(root.glob("**/escalation-policy.json")):
        replica_root = escalation.parent
        payload = load_json(escalation)
        cpu = cpu_model(replica_root)
        replica = replica_root.name
        for row in payload.get("rows", []):
            regime = row["regime"]
            count = point_count(replica_root, regime)
            if count <= 0:
                raise SystemExit(f"missing point evidence for {replica} {regime}")
            rows.append(
                {
                    "replica": replica,
                    "cpu": cpu,
                    "regime": regime,
                    "point_count": count,
                    **row,
                }
            )
    if not rows:
        raise SystemExit("no escalation-policy.json files found")
    return rows


def evaluate_case(row, horizon, consistency_threshold, margin_threshold):
    static_per_call = row["static_cycle_ns"] / row["point_count"]
    candidate_per_call = row["candidate_cycle_ns"] / row["point_count"]
    oracle_per_call = row["oracle_cycle_ns"] / row["point_count"]

    static_total = static_per_call * horizon
    oracle_total = oracle_per_call * horizon
    available = max(0.0, static_total - oracle_total)

    break_even = row.get("break_even_calls")
    confidence_ok = row["sentinel_consistency"] >= consistency_threshold
    margin_ok = row["sentinel_margin_pct"] >= margin_threshold
    economic_ok = break_even is not None and break_even <= horizon
    useful_candidate = row.get("candidate_capture_fraction", 0.0) > 0.0

    act = confidence_ok and margin_ok and economic_ok and useful_candidate

    if act:
        policy_total = candidate_per_call * horizon + row["total_probe_cost_ns"]
    else:
        policy_total = static_total

    net = static_total - policy_total
    recovered = max(0.0, net)
    capture = 0.0 if available <= 0 else recovered / available
    savings_pct = 0.0 if static_total <= 0 else 100.0 * net / static_total

    return {
        "act": act,
        "net_saving_ns": net,
        "savings_pct": savings_pct,
        "available_regret_ns": available,
        "capture_fraction": capture,
        "confidence_ok": confidence_ok,
        "margin_ok": margin_ok,
        "economic_ok": economic_ok,
    }


def summarize(rows):
    summaries = []
    details = []

    for horizon in HORIZONS:
        for consistency in CONSISTENCY_THRESHOLDS:
            for margin in MARGIN_THRESHOLDS:
                case_results = []
                for row in rows:
                    result = evaluate_case(row, horizon, consistency, margin)
                    case_results.append(result)
                    details.append(
                        {
                            "horizon": horizon,
                            "consistency_threshold": consistency,
                            "margin_threshold_pct": margin,
                            "replica": row["replica"],
                            "cpu": row["cpu"],
                            "regime": row["regime"],
                            "sentinel_winner": row["sentinel_winner"],
                            "sentinel_consistency": row["sentinel_consistency"],
                            "sentinel_margin_pct": row["sentinel_margin_pct"],
                            "break_even_calls": row.get("break_even_calls"),
                            "candidate_capture_fraction": row.get(
                                "candidate_capture_fraction", 0.0
                            ),
                            **result,
                        }
                    )

                total_available = sum(
                    r["available_regret_ns"] for r in case_results
                )
                total_net = sum(r["net_saving_ns"] for r in case_results)
                total_recovered = sum(
                    max(0.0, r["net_saving_ns"]) for r in case_results
                )
                summaries.append(
                    {
                        "horizon": horizon,
                        "consistency_threshold": consistency,
                        "margin_threshold_pct": margin,
                        "actions": sum(r["act"] for r in case_results),
                        "negative_actions": sum(
                            r["act"] and r["net_saving_ns"] < 0
                            for r in case_results
                        ),
                        "positive_actions": sum(
                            r["act"] and r["net_saving_ns"] > 0
                            for r in case_results
                        ),
                        "mean_savings_pct": sum(
                            r["savings_pct"] for r in case_results
                        )
                        / len(case_results),
                        "min_case_savings_pct": min(
                            r["savings_pct"] for r in case_results
                        ),
                        "total_net_saving_ns": total_net,
                        "available_regret_capture_fraction": (
                            0.0
                            if total_available <= 0
                            else total_recovered / total_available
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
        default=pathlib.Path("w2-cross-runner-policy"),
    )
    args = parser.parse_args()

    rows = discover(args.root)
    summaries, details = summarize(rows)
    args.output.mkdir(parents=True, exist_ok=True)

    payload = {
        "schema_version": 1,
        "horizons": list(HORIZONS),
        "consistency_thresholds": list(CONSISTENCY_THRESHOLDS),
        "margin_thresholds_pct": list(MARGIN_THRESHOLDS),
        "cases": rows,
        "summaries": summaries,
    }
    (args.output / "policy-simulation.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n"
    )
    write_csv(summaries, args.output / "policy-grid.csv")
    write_csv(details, args.output / "policy-cases.csv")

    lines = [
        "# W2 cross-runner policy simulation",
        "",
        "This is an offline research sensitivity analysis, not a production threshold recommendation.",
        "",
        "| Horizon | Consistency | Margin | Actions | Positive | Negative | Mean saving | Worst case | Available regret captured |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in summaries:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row["horizon"]),
                    f"{100.0 * row['consistency_threshold']:.0f}%",
                    f"{row['margin_threshold_pct']:.0f}%",
                    str(row["actions"]),
                    str(row["positive_actions"]),
                    str(row["negative_actions"]),
                    f"{row['mean_savings_pct']:.3f}%",
                    f"{row['min_case_savings_pct']:.3f}%",
                    f"{100.0 * row['available_regret_capture_fraction']:.1f}%",
                ]
            )
            + " |"
        )

    report = "\n".join(lines) + "\n"
    (args.output / "policy-simulation.md").write_text(report)
    print(report)


if __name__ == "__main__":
    main()
