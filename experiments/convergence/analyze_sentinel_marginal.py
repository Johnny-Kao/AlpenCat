#!/usr/bin/env python3
import argparse
import json
import pathlib


def marginal(rows):
    out = []
    for regime in sorted({row["regime"] for row in rows}):
        cases = sorted(
            [row for row in rows if row["regime"] == regime],
            key=lambda row: int(row["max_points"]),
        )
        previous = None
        for row in cases:
            if previous is None:
                previous = row
                continue
            delta_gain = (
                float(row["candidate_gain_per_call_ns"])
                - float(previous["candidate_gain_per_call_ns"])
            )
            delta_probe = (
                float(row["probe_cost_ns"])
                - float(previous["probe_cost_ns"])
            )
            break_even = None
            if delta_gain > 0.0 and delta_probe > 0.0:
                break_even = delta_probe / delta_gain

            out.append({
                "regime": regime,
                "from_points": previous["max_points"],
                "to_points": row["max_points"],
                "delta_gain_per_call_ns": delta_gain,
                "delta_probe_cost_ns": delta_probe,
                "marginal_break_even_calls": break_even,
                "adds_value": delta_gain > 0.0,
            })
            previous = row
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sentinel_json", type=pathlib.Path)
    args = parser.parse_args()
    data = json.loads(args.sentinel_json.read_text())
    rows = marginal(data["cases"])
    target = args.sentinel_json.with_name("sentinel-marginal-economics.json")
    target.write_text(json.dumps({"schema_version": 1, "marginals": rows}, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
