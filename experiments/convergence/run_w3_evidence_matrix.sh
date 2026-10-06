#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ALPENCAT_W3_OUT:-$ROOT/evidence-w3-results}"
BIN="$ROOT/target/release/examples/evidence_mixed_workload"
REPEATS="${ALPENCAT_REPEATS:-3}"
CALLS_PER_POINT="${ALPENCAT_CALLS_PER_POINT:-100}"

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.jsonl "$OUT_DIR"/economics-summary.json "$OUT_DIR"/policy-summary.csv

echo "[W3] building mixed compute/memory workload"
cargo build --release -p runtime-api --example evidence_mixed_workload

mapfile -t CPUS < <(python3 - <<'PY'
import os
for cpu in sorted(os.sched_getaffinity(0)):
    print(cpu)
PY
)

if (("${#CPUS[@]}" == 0)); then
  echo "no schedulable CPUs detected" >&2
  exit 1
fi

ALL_CPUS="$(IFS=,; echo "${CPUS[*]}")"
HALF_COUNT=$(( ("${#CPUS[@]}" + 1) / 2 ))
HALF_CPUS="$(IFS=,; echo "${CPUS[*]:0:$HALF_COUNT}")"
ONE_CPU="${CPUS[0]}"

{
  echo "git_sha=$(git -C "$ROOT" rev-parse HEAD)"
  echo "utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "workload=mixed-gather-mix-u64"
  echo "uname=$(uname -a)"
  echo "effective_cpus=$ALL_CPUS"
  echo "half_cpus=$HALF_CPUS"
  echo "one_cpu=$ONE_CPU"
  echo "cpu_max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || true)"
  echo "cpuset=$(cat /sys/fs/cgroup/cpuset.cpus.effective 2>/dev/null || true)"
  lscpu 2>/dev/null || true
} > "$OUT_DIR/manifest.txt"

run_regime() {
  local index="$1"
  local total="$2"
  local regime="$3"
  local cpus="$4"
  local start_boundary="$5"
  local bootstrap="$6"
  local output="$OUT_DIR/${regime}.jsonl"
  local started
  started="$(date +%s)"

  echo "[W3 ${index}/${total}] regime=${regime} cpus=${cpus} start_boundary=${start_boundary} bootstrap=${bootstrap} start"
  ALPENCAT_REGIME="$regime" \
  ALPENCAT_REPEATS="$REPEATS" \
  ALPENCAT_WARMUP_PAIRS="${ALPENCAT_WARMUP_PAIRS:-2}" \
  ALPENCAT_START_BOUNDARY="$start_boundary" \
  ALPENCAT_BOOTSTRAP="$bootstrap" \
  ALPENCAT_REVALIDATION_POINTS="3" \
    taskset -c "$cpus" "$BIN" > "$output"

  local ended
  ended="$(date +%s)"
  echo "[W3 ${index}/${total}] regime=${regime} complete elapsed=$((ended-started))s records=$(wc -l < "$output")"
}

extract_boundary() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path
path = Path(sys.argv[1])
fallback = int(sys.argv[2])
for line in path.read_text().splitlines():
    row = json.loads(line)
    if row.get("record_type") == "revalidation":
        print(int(row.get("published_serial_max_items", fallback)))
        raise SystemExit
print(fallback)
PY
}

TOTAL=5
run_regime 1 "$TOTAL" "baseline-full" "$ALL_CPUS" "262144" "1"
BASELINE_BOUNDARY="$(extract_boundary "$OUT_DIR/baseline-full.jsonl" 262144)"
echo "[W3] baseline boundary=$BASELINE_BOUNDARY"

if (("${#CPUS[@]}" > 1)); then
  run_regime 2 "$TOTAL" "half" "$HALF_CPUS" "$BASELINE_BOUNDARY" "0"
else
  cp "$OUT_DIR/baseline-full.jsonl" "$OUT_DIR/half.jsonl"
  python3 - "$OUT_DIR/half.jsonl" <<'PY'
import json
import sys
from pathlib import Path
path = Path(sys.argv[1])
rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
for row in rows:
    row["regime"] = "half"
path.write_text("\n".join(json.dumps(row, separators=(",", ":")) for row in rows) + "\n")
PY
  echo "[W3 2/$TOTAL] regime=half synthetic-copy reason=single-cpu-host"
fi

run_regime 3 "$TOTAL" "one" "$ONE_CPU" "$BASELINE_BOUNDARY" "0"

echo "[W3 4/$TOTAL] regime=contention starting controlled background load"
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
  cpu="${CPUS[$i]}"
  taskset -c "$cpu" sh -c 'while :; do :; done' &
  BURNERS+=("$!")
done
sleep 1
run_regime 4 "$TOTAL" "contention" "$ALL_CPUS" "$BASELINE_BOUNDARY" "0"
CONTENTION_BOUNDARY="$(extract_boundary "$OUT_DIR/contention.jsonl" "$BASELINE_BOUNDARY")"
cleanup
BURNERS=()
trap - EXIT

sleep 1
run_regime 5 "$TOTAL" "recovery" "$ALL_CPUS" "$CONTENTION_BOUNDARY" "0"

cat "$OUT_DIR"/baseline-full.jsonl \
    "$OUT_DIR"/half.jsonl \
    "$OUT_DIR"/one.jsonl \
    "$OUT_DIR"/contention.jsonl \
    "$OUT_DIR"/recovery.jsonl \
    > "$OUT_DIR/evidence.jsonl"

echo "[W3] validating evidence records"
python3 - "$OUT_DIR/evidence.jsonl" <<'PY'
import json
import sys
from pathlib import Path

rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines() if line.strip()]
points = [row for row in rows if row.get("record_type") == "point"]
revals = [row for row in rows if row.get("record_type") == "revalidation"]
if len(points) != 45 or len(revals) != 5:
    raise SystemExit(f"unexpected W3 evidence counts: points={len(points)} revalidation={len(revals)}")
for row in points:
    if row["cpu_route_available"] and row["output_equivalent"] is not True:
        raise SystemExit("W3 route equivalence failure")
print(f"validated W3 evidence: {len(points)} point rows, {len(revals)} revalidation rows")
PY

echo "[W3] reconstructing Static / Periodic / AlpenCat / Oracle"
python3 "$ROOT/experiments/convergence/analyze_economics.py" \
  "$OUT_DIR" \
  --calls-per-point "$CALLS_PER_POINT" \
  | tee "$OUT_DIR/economics-summary.md"

echo "[W3] complete: $OUT_DIR"
