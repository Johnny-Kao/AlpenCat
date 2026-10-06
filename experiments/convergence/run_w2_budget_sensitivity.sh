#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ALPENCAT_W2_SENSITIVITY_OUT:-$ROOT/w2-budget-sensitivity}"
BIN="$ROOT/target/release/examples/evidence_memory_workload"
REPEATS="${ALPENCAT_REPEATS:-3}"

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.jsonl "$OUT_DIR"/summary.csv

cargo build --release -p runtime-api --example evidence_memory_workload

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
  taskset -c "$ALL_CPUS" "$BIN" > "$OUT_DIR/baseline.jsonl"

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
echo "[W2 sensitivity] baseline boundary=$BASELINE_BOUNDARY"

extract_row() {
  python3 - "$1" "$2" "$3" <<'PY'
import json, sys
from pathlib import Path
path, regime, budget = sys.argv[1], sys.argv[2], sys.argv[3]
for line in Path(path).read_text().splitlines():
    row=json.loads(line)
    if row.get("record_type")=="revalidation":
        print(",".join([
            regime,
            budget,
            str(row["start_boundary"]),
            row["status"],
            str(row["measurement_count"]),
            str(row["published_serial_max_items"]),
            str(row["revalidation_elapsed_ns"]),
            str(row["boundary_stale"]).lower(),
        ]))
        break
PY
}

echo "regime,max_points,start_boundary,status,measurement_count,published_boundary,revalidation_elapsed_ns,boundary_stale" > "$OUT_DIR/summary.csv"

for budget in 3 4 5 6; do
  echo "[W2 sensitivity] half budget=$budget"
  file="$OUT_DIR/half-p${budget}.jsonl"
  ALPENCAT_REGIME="half" \
  ALPENCAT_REPEATS="$REPEATS" \
  ALPENCAT_START_BOUNDARY="$BASELINE_BOUNDARY" \
  ALPENCAT_BOOTSTRAP="0" \
  ALPENCAT_REVALIDATION_POINTS="$budget" \
    taskset -c "$HALF_CPUS" "$BIN" > "$file"
  extract_row "$file" "half" "$budget" >> "$OUT_DIR/summary.csv"
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

for budget in 3 4 5 6; do
  echo "[W2 sensitivity] contention budget=$budget"
  file="$OUT_DIR/contention-p${budget}.jsonl"
  ALPENCAT_REGIME="contention" \
  ALPENCAT_REPEATS="$REPEATS" \
  ALPENCAT_START_BOUNDARY="$BASELINE_BOUNDARY" \
  ALPENCAT_BOOTSTRAP="0" \
  ALPENCAT_REVALIDATION_POINTS="$budget" \
    taskset -c "$ALL_CPUS" "$BIN" > "$file"
  extract_row "$file" "contention" "$budget" >> "$OUT_DIR/summary.csv"
done
cleanup
trap - EXIT

cat "$OUT_DIR/summary.csv"
