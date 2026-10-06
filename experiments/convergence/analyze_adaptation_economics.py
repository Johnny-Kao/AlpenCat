#!/usr/bin/env python3
import argparse
import csv
import json
import math
import pathlib
import statistics
from collections import defaultdict


HORIZONS = (10, 100, 1_000, 10_000)


def median(values):
    return float(statistics.median(values))


def load_jsonl(path):
    rows = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def load_directory(root):
    records = []
    for path in sorted(root.glob("*.jsonl")):
        if path.name == "evidence.jsonl":
            continue
        records.extend(load_jsonl(path))
    if not records:
        raise SystemExit(f"no JSONL evidence found under {root}")
    return records


def derive_boundary(points):
    ordered = sorted(points, key=lambda row: row["work_items"])
    previous = 0
    winners = []
    for row in ordered:
        serial = median(row["serial_samples_ns"])
        cpu_samples = row.get("cpu_samples_ns")
        if cpu_samples is None:
            winners.append("Serial")
            previous = row["work_items"]
            continue
        cpu = median(cpu_samples)
        winner = "Cpu" if cpu < serial else "Serial"
        winners.append(winner)
        if winner == "Cpu":
            return previous, winners
        previous = row["work_items"]
    return ordered[-1]["work_items"], winners


def route_cost(row, boundary):
    serial = median(row["serial_samples_ns"])
    cpu_samples = row.get("cpu_samples_ns")
    if row["work_items"] <= boundary or cpu_samples is None:
        return serial, "Serial"
    return median(cpu_samples), "Cpu"


def oracle_cost(row):
    serial = median(row["serial_samples_ns"])
    cpu_samples = row.get("cpu_samples_ns")
    if cpu_samples is None:
        return serial, "Serial"
    cpu = median(cpu_samples)
    if cpu < serial:
        return cpu, "Cpu"
    return serial, "Serial"


def auto_route_cost(row):
    backend = row["auto_backend"]
    if backend == "Cpu" and row.get("cpu_samples_ns") is not None:
        return median(row["cpu_samples_ns"]), "Cpu"
    return median(row["serial_samples_ns"]), "Serial"


def point_preference_consistency(row):
    cpu_samples = row.get("cpu_samples_ns")
    if cpu_samples is None:
        return 1.0
    serial_samples = row["serial_samples_ns"]
    pairs = list(zip(serial_samples, cpu_samples))
    if not pairs:
        return 0.0
    median_winner = oracle_cost(row)[1]
    agreement = 0
    for serial, cpu in pairs:
        winner = "Cpu" if cpu < serial else "Serial"
        if winner == median_winner:
            agreement += 1
    return agreement / len(pairs)


def point_margin_pct(row):
    cpu_samples = row.get("cpu_samples_ns")
    if cpu_samples is None:
        return None
    serial = median(row["serial_samples_ns"])
    cpu = median(cpu_samples)
    best = min(serial, cpu)
    if best <= 0:
        return 0.0
    return 100.0 * abs(serial - cpu) / best


def preference_switches(points):
    winners = []
    for row in sorted(points, key=lambda item: item["work_items"]):
        if row.get("cpu_samples_ns") is None:
            continue
        winners.append(oracle_cost(row)[1])
    switches = sum(a != b for a, b in zip(winners, winners[1:]))
    return switches, winners


def nearest_boundary_points(points, boundary):
    ordered = sorted(points, key=lambda row: row["work_items"])
    if not ordered:
        return []
    left = [row for row in ordered if row["work_items"] <= boundary]
    right = [row for row in ordered if row["work_items"] > boundary]
    selected = []
    if left:
        selected.append(left[-1])
    if right:
        selected.append(right[0])
    if not selected:
        selected.append(ordered[0])
    return selected


def summarize_regime(points, revalidation, static_boundary):
    points = sorted(points, key=lambda row: row["work_items"])
    oracle_boundary, _ = derive_boundary(points)

    total_weight = sum(float(row.get("weight", 1.0)) for row in points)
    static_cycle = 0.0
    oracle_cycle = 0.0
    alpencat_cycle = 0.0
    wrong_penalties = []

    for row in points:
        weight = float(row.get("weight", 1.0))
        static_cost, static_route = route_cost(row, static_boundary)
        best_cost, best_route = oracle_cost(row)
        alpencat_cost, _ = auto_route_cost(row)

        static_cycle += static_cost * weight
        oracle_cycle += best_cost * weight
        alpencat_cycle += alpencat_cost * weight

        if static_route != best_route and best_cost > 0:
            wrong_penalties.append(100.0 * (static_cost - best_cost) / best_cost)

    recoverable_cycle = max(0.0, static_cycle - oracle_cycle)
    realized_cycle = max(0.0, static_cycle - alpencat_cycle)
    regret_per_call = (
        0.0 if total_weight <= 0 else recoverable_cycle / total_weight
    )
    realized_gain_per_call = (
        0.0 if total_weight <= 0 else realized_cycle / total_weight
    )
    revalidation_cost = float(revalidation.get("revalidation_elapsed_ns", 0.0))

    break_even_calls_oracle = (
        math.inf if regret_per_call <= 0 else revalidation_cost / regret_per_call
    )
    break_even_calls_realized = (
        math.inf
        if realized_gain_per_call <= 0
        else revalidation_cost / realized_gain_per_call
    )

    near = nearest_boundary_points(points, oracle_boundary)
    near_margins = [
        value
        for value in (point_margin_pct(row) for row in near)
        if value is not None
    ]
    near_consistency = [point_preference_consistency(row) for row in near]

    switches, winners = preference_switches(points)
    horizons = {}
    for calls in HORIZONS:
        theoretical_gain = regret_per_call * calls - revalidation_cost
        realized_gain = realized_gain_per_call * calls - revalidation_cost
        horizons[str(calls)] = {
            "oracle_net_gain_ns": theoretical_gain,
            "realized_net_gain_ns": realized_gain,
        }

    return {
        "start_boundary": revalidation.get("start_boundary", static_boundary),
        "published_boundary": revalidation.get("published_serial_max_items"),
        "status": revalidation.get("status"),
        "oracle_boundary": oracle_boundary,
        "total_weight_per_cycle": total_weight,
        "static_cost_per_cycle_ns": static_cycle,
        "oracle_cost_per_cycle_ns": oracle_cycle,
        "alpencat_execution_cost_per_cycle_ns": alpencat_cycle,
        "recoverable_regret_per_cycle_ns": recoverable_cycle,
        "realized_execution_gain_per_cycle_ns": realized_cycle,
        "recoverable_regret_per_call_ns": regret_per_call,
        "realized_execution_gain_per_call_ns": realized_gain_per_call,
        "revalidation_cost_ns": revalidation_cost,
        "break_even_calls_oracle": break_even_calls_oracle,
        "break_even_calls_realized": break_even_calls_realized,
        "wrong_route_points": len(wrong_penalties),
        "max_wrong_route_penalty_pct": max(wrong_penalties, default=0.0),
        "near_boundary_margin_pct_min": min(near_margins, default=None),
        "near_boundary_margin_pct_max": max(near_margins, default=None),
        "near_boundary_preference_consistency_min": min(
            near_consistency, default=None
        ),
        "near_boundary_preference_consistency_mean": (
            None
            if not near_consistency
            else sum(near_consistency) / len(near_consistency)
        ),
        "preference_switch_count": switches,
        "preference_sequence": winners,
        "non_monotonic_preference": switches > 1,
        "horizons": horizons,
    }


def analyze_main(records):
    points = [row for row in records if row.get("record_type") == "point"]
    revalidations = {
        row["regime"]: row
        for row in records
        if row.get("record_type") == "revalidation"
    }
    grouped = defaultdict(list)
    for row in points:
        grouped[row["regime"]].append(row)

    if "baseline-full" not in grouped:
        raise SystemExit("missing baseline-full evidence")

    static_boundary, _ = derive_boundary(grouped["baseline-full"])
    result = {
        "schema_version": 1,
        "static_boundary": static_boundary,
        "regimes": {},
    }

    for regime in sorted(grouped):
        result["regimes"][regime] = summarize_regime(
            grouped[regime],
            revalidations.get(regime, {}),
            static_boundary,
        )

    return result


def analyze_sensitivity(root):
    rows = []
    for path in sorted(root.glob("half-p*.jsonl")) + sorted(
        root.glob("contention-p*.jsonl")
    ):
        records = load_jsonl(path)
        points = [row for row in records if row.get("record_type") == "point"]
        revalidation = next(
            (
                row
                for row in records
                if row.get("record_type") == "revalidation"
            ),
            {},
        )
        if not points:
            continue
        budget = int(path.stem.split("-p")[-1])
        start_boundary = int(
            revalidation.get("start_boundary", derive_boundary(points)[0])
        )
        summary = summarize_regime(points, revalidation, start_boundary)
        rows.append(
            {
                "regime": revalidation.get("regime", path.stem.split("-p")[0]),
                "max_points": budget,
                **summary,
            }
        )
    return rows


def finite_or_none(value):
    return None if math.isinf(value) or math.isnan(value) else value


def json_safe(value):
    if isinstance(value, dict):
        return {key: json_safe(item) for key, item in value.items()}
    if isinstance(value, list):
        return [json_safe(item) for item in value]
    if isinstance(value, float) and (math.isinf(value) or math.isnan(value)):
        return None
    return value


def write_main_csv(summary, path):
    fields = [
        "regime",
        "start_boundary",
        "published_boundary",
        "oracle_boundary",
        "status",
        "recoverable_regret_per_call_ns",
        "realized_execution_gain_per_call_ns",
        "revalidation_cost_ns",
        "break_even_calls_oracle",
        "break_even_calls_realized",
        "wrong_route_points",
        "max_wrong_route_penalty_pct",
        "near_boundary_margin_pct_min",
        "near_boundary_preference_consistency_min",
        "preference_switch_count",
        "non_monotonic_preference",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for regime, row in summary["regimes"].items():
            writer.writerow(
                {
                    "regime": regime,
                    **{
                        key: finite_or_none(row.get(key))
                        for key in fields
                        if key != "regime"
                    },
                }
            )


def write_sensitivity_csv(rows, path):
    fields = [
        "regime",
        "max_points",
        "start_boundary",
        "published_boundary",
        "oracle_boundary",
        "status",
        "revalidation_cost_ns",
        "break_even_calls_oracle",
        "break_even_calls_realized",
        "near_boundary_margin_pct_min",
        "near_boundary_preference_consistency_min",
        "non_monotonic_preference",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    key: finite_or_none(row.get(key))
                    for key in fields
                }
            )


def fmt_calls(value):
    if value is None or math.isinf(value):
        return "n/a"
    if value < 10:
        return f"{value:.1f}"
    return f"{value:.0f}"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("evidence_root", type=pathlib.Path)
    parser.add_argument(
        "--sensitivity-root",
        type=pathlib.Path,
        default=None,
    )
    args = parser.parse_args()

    summary = analyze_main(load_directory(args.evidence_root))
    sensitivity = (
        analyze_sensitivity(args.sensitivity_root)
        if args.sensitivity_root is not None
        else []
    )

    output = {
        "main": summary,
        "budget_sensitivity": sensitivity,
    }
    (args.evidence_root / "adaptation-economics.json").write_text(
        json.dumps(json_safe(output), indent=2, sort_keys=True) + "\n"
    )
    write_main_csv(
        summary,
        args.evidence_root / "adaptation-economics.csv",
    )
    if sensitivity:
        write_sensitivity_csv(
            sensitivity,
            args.evidence_root / "adaptation-budget-sensitivity.csv",
        )

    lines = [
        "# W2 adaptation economics",
        "",
        "| Regime | Start | Published | Oracle | Revalidation | Break-even (oracle) | Break-even (realized) | Wrong points | Near-boundary consistency | Non-monotonic |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for regime, row in summary["regimes"].items():
        consistency = row["near_boundary_preference_consistency_min"]
        lines.append(
            "| "
            + " | ".join(
                [
                    regime,
                    str(row["start_boundary"]),
                    str(row["published_boundary"]),
                    str(row["oracle_boundary"]),
                    f"{row['revalidation_cost_ns'] / 1e6:.3f} ms",
                    fmt_calls(row["break_even_calls_oracle"]),
                    fmt_calls(row["break_even_calls_realized"]),
                    str(row["wrong_route_points"]),
                    (
                        "n/a"
                        if consistency is None
                        else f"{100.0 * consistency:.1f}%"
                    ),
                    "yes" if row["non_monotonic_preference"] else "no",
                ]
            )
            + " |"
        )

    if sensitivity:
        lines.extend(
            [
                "",
                "## Budget sensitivity",
                "",
                "| Regime | Points | Published | Oracle | Revalidation | Break-even (oracle) | Near-boundary consistency |",
                "|---|---:|---:|---:|---:|---:|---:|",
            ]
        )
        for row in sensitivity:
            consistency = row["near_boundary_preference_consistency_min"]
            lines.append(
                "| "
                + " | ".join(
                    [
                        row["regime"],
                        str(row["max_points"]),
                        str(row["published_boundary"]),
                        str(row["oracle_boundary"]),
                        f"{row['revalidation_cost_ns'] / 1e6:.3f} ms",
                        fmt_calls(row["break_even_calls_oracle"]),
                        (
                            "n/a"
                            if consistency is None
                            else f"{100.0 * consistency:.1f}%"
                        ),
                    ]
                )
                + " |"
            )

    report = "\n".join(lines) + "\n"
    (args.evidence_root / "adaptation-economics.md").write_text(report)
    print(report)


if __name__ == "__main__":
    main()
