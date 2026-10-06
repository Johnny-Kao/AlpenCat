#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ALPENCAT_W2_INTERVAL_OUT:-$ROOT/w2-interval-probe}"
BASE_BIN="$ROOT/target/release/examples/evidence_memory_workload"
PROBE_BIN="$ROOT/target/release/examples/w2_interval_probe"
REPEATS="${ALPENCAT_REPEATS:-3}"

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.jsonl "$OUT_DIR"/summary.csv

cargo build --release -p runtime-api --example evidence_memory_workload --example w2_interval_probe

mapfile -t CPUS < <(python3 - <<'PY'
import os
for cpu in sorted(os.sched_getaffinity(0)):
    print(cpu)
PY
)
ALL_CPUS="$(IFS=,; echo "${CPUS[*]}")"
HALF_COUNT=$(( ("${#CPUS[@]}" + 1) / 2 ))
HALF_CPUS="$(IFS=,; echo "${CPUS[*]:0:$HALF_COUNT}")"

ALPENCAT_REGIME="baseline-full" \
ALPENCAT_REPEATS="$REPEATS" \
ALPENCAT_START_BOUNDARY="262144" \
ALPENCAT_BOOTSTRAP="1" \
  taskset -c "$ALL_CPUS" "$BASE_BIN" > "$OUT_DIR/baseline.jsonl"

BASELINE_BOUNDARY="$(python3 - "$OUT_DIR/baseline.jsonl" <<'PY'
import json, sys
from pathlib import Path
for line in Path(sys.argv[1]).read_text().splitlines():
    row=json.loads(line)
    if row.get("record_type")=="revalidation":
        print(row["published_serial_max_items"])
        break
PY
)"
echo "[W2 interval] baseline boundary=$BASELINE_BOUNDARY"

run_probe() {
  local regime="$1"
  local cpus="$2"
  local budget="$3"
  local file="$OUT_DIR/${regime}-p${budget}.jsonl"
  echo "[W2 interval] regime=$regime budget=$budget"
  ALPENCAT_REGIME="$regime" \
  ALPENCAT_REPEATS="$REPEATS" \
  ALPENCAT_START_BOUNDARY="$BASELINE_BOUNDARY" \
  ALPENCAT_INTERVAL_MAX_POINTS="$budget" \
    taskset -c "$cpus" "$PROBE_BIN" > "$file"
}

for budget in 5 7 9; do
  run_probe "half" "$HALF_CPUS" "$budget"
done

BURNERS=()
cleanup() {
  for pid in "${BURNERS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT
BURNER_COUNT=$(( "${#CPUS[@]}" > 2 ? 2 : 1 ))
for ((i=0; i<BURNER_COUNT; i++)); do
  taskset -c "${CPUS[$i]}" sh -c 'while :; do :; done' &
  BURNERS+=("$!")
done
sleep 1
for budget in 5 7 9; do
  run_probe "contention" "$ALL_CPUS" "$budget"
done
cleanup
trap - EXIT

python3 - "$OUT_DIR" <<'PY'
import csv, json, sys
from pathlib import Path
root=Path(sys.argv[1])
rows=[]
for path in sorted(root.glob("*-p*.jsonl")):
    for line in path.read_text().splitlines():
        row=json.loads(line)
        if row.get("record_type")=="interval_summary":
            rows.append(row)
            break
fields=[
    "regime","max_points","start_boundary","status","probe_count",
    "probe_elapsed_ns","lower_serial_max","upper_cpu_max",
    "min_probe_consistency","min_probe_margin_pct",
    "best_scalar_gap_pct","inferred_interval_gap_pct",
]
with (root/"summary.csv").open("w", newline="") as handle:
    writer=csv.DictWriter(handle, fieldnames=fields)
    writer.writeheader()
    for row in rows:
        writer.writerow({key:row.get(key) for key in fields})
print("regime,max_points,status,probe_count,probe_ms,lower,upper,consistency,scalar_gap_pct,interval_gap_pct")
for row in rows:
    gap=row.get("inferred_interval_gap_pct")
    print(",".join([
        row["regime"],str(row["max_points"]),row["status"],str(row["probe_count"]),
        f"{row['probe_elapsed_ns']/1e6:.3f}",str(row.get("lower_serial_max")),
        str(row.get("upper_cpu_max")),f"{100*row['min_probe_consistency']:.1f}%",
        f"{row['best_scalar_gap_pct']:.3f}", "n/a" if gap is None else f"{gap:.3f}",
    ]))
PY
