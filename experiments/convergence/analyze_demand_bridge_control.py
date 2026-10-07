#!/usr/bin/env python3
import argparse
import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "bridge", HERE / "analyze_demand_bridge_value.py"
)
BRIDGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BRIDGE)

DEFAULT_HORIZONS = (100, 1_000, 10_000)


def load_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("main_root", type=pathlib.Path)
    parser.add_argument("--horizons", type=int, nargs="+", default=list(DEFAULT_HORIZONS))
    args = parser.parse_args()

    evidence = load_jsonl(args.main_root / "evidence.jsonl")
    demand_by_regime = {}
    for row in evidence:
        if row.get("record_type") == "point":
            demand_by_regime.setdefault(row["regime"], []).append(row)

    output = {
        "schema_version": 1,
        "label": "demand-aware-bridge-control",
        "cases": [],
    }

    for path in sorted(args.main_root.glob("*.jsonl")):
        if path.name == "evidence.jsonl":
            continue
        rows = load_jsonl(path)
        reval = next((row for row in rows if row.get("record_type") == "revalidation"), None)
        if reval is None:
            continue
        regime = reval["regime"]
        case = {"regime": regime, "horizons": {}}
        for horizon in args.horizons:
            case["horizons"][str(horizon)] = BRIDGE.bridge_value(
                reval, demand_by_regime.get(regime, []), horizon
            )
        output["cases"].append(case)

    target = args.main_root / "demand-aware-bridge-control.json"
    target.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    print(target)


if __name__ == "__main__":
    main()
