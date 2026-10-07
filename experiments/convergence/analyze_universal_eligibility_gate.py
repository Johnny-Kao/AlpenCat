#!/usr/bin/env python3
import argparse
import csv
import json
import pathlib


def load_cases(path):
    with path.open(newline="") as f:
        return list(csv.DictReader(f))


def as_int(row, key):
    return int(float(row[key]))


def as_float(row, key):
    return float(row[key])


def has_opportunity(row):
    return as_float(row, "recoverable_ns") > 0.0


def evaluate(rows, name, eligible_fn):
    eligible = [row for row in rows if eligible_fn(row)]
    skipped = [row for row in rows if not eligible_fn(row)]
    total_opp = sum(has_opportunity(row) for row in rows)
    retained_opp = sum(has_opportunity(row) for row in eligible)
    skipped_opp = total_opp - retained_opp
    skipped_no_opp = sum(not has_opportunity(row) for row in skipped)
    false_positive_eligible = sum(not has_opportunity(row) for row in eligible)
    return {
        "gate": name,
        "case_count": len(rows),
        "eligible_case_count": len(eligible),
        "skipped_case_count": len(skipped),
        "total_opportunity_cases": total_opp,
        "retained_opportunity_cases": retained_opp,
        "missed_opportunity_cases": skipped_opp,
        "skipped_no_opportunity_cases": skipped_no_opp,
        "eligible_no_opportunity_cases": false_positive_eligible,
        "opportunity_recall": 0.0 if total_opp == 0 else retained_opp / total_opp,
        "skip_fraction": 0.0 if not rows else len(skipped) / len(rows),
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("cases_csv", type=pathlib.Path)
    p.add_argument("--out", type=pathlib.Path, required=True)
    args = p.parse_args()

    rows = load_cases(args.cases_csv)

    gates = [
        (
            "multi-core-only",
            lambda row: as_int(row, "logical_cpus") > 1,
        ),
        (
            "resource-transition-only",
            lambda row: row["regime"] != "baseline-full",
        ),
        (
            "universal-zero-cost-gate-v1",
            lambda row: (
                as_int(row, "logical_cpus") > 1
                and row["regime"] != "baseline-full"
            ),
        ),
    ]

    output = {
        "schema_version": 1,
        "warning": (
            "Offline eligibility evaluation. Gate inputs are intentionally limited "
            "to cheap platform-independent signals available before dedicated probing."
        ),
        "gates": [evaluate(rows, name, fn) for name, fn in gates],
    }

    args.out.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
