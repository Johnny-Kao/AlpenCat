#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${ALPENCAT_W2_STABILITY_OUT:-$ROOT/w2-stability-blocks}"
BIN="$ROOT/target/release/examples/evidence_memory_workload"
REPEATS="${ALPENCAT_STABILITY_REPEATS:-5}"
mkdir -p "$OUT"
rm -f "$OUT"/*.jsonl "$OUT"/stability-*.json "$OUT"/stability-*.csv "$OUT"/stability-*.md
cargo build --release -p runtime-api --example evidence_memory_workload

mapfile -t CPUS < <(python3 - <<'PY'
import os
for c in sorted(os.sched_getaffinity(0)): print(c)
PY
)
ALL="$(IFS=,; echo "${CPUS[*]}")"
HALF_N=$(( ("${#CPUS[@]}"+1)/2 ))
HALF="$(IFS=,; echo "${CPUS[*]:0:$HALF_N}")"

run_block() {
  local regime="$1" cpus="$2" block="$3" rot="$4" rev="$5"
  echo "[W2 stability] regime=$regime block=$block rotation=$rot reverse=$rev"
  ALPENCAT_REGIME="$regime" \
  ALPENCAT_REPEATS="$REPEATS" \
  ALPENCAT_WARMUP_PAIRS="2" \
  ALPENCAT_START_BOUNDARY="65536" \
  ALPENCAT_BOOTSTRAP="0" \
  ALPENCAT_REVALIDATION_POINTS="1" \
  ALPENCAT_SIZE_ROTATION="$rot" \
  ALPENCAT_SIZE_REVERSE="$rev" \
    taskset -c "$cpus" "$BIN" > "$OUT/${regime}-b${block}.jsonl"
}

for b in 0 1 2 3 4 5; do
  rot=$((b % 3)); rev=$((b / 3))
  run_block "half" "$HALF" "$b" "$rot" "$rev"
done

BURNERS=()
cleanup(){ for p in "${BURNERS[@]:-}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT
N=$(( "${#CPUS[@]}" > 2 ? 2 : 1 ))
for ((i=0;i<N;i++)); do taskset -c "${CPUS[$i]}" sh -c 'while :; do :; done' & BURNERS+=("$!"); done
sleep 1
for b in 0 1 2 3 4 5; do
  rot=$((b % 3)); rev=$((b / 3))
  run_block "contention" "$ALL" "$b" "$rot" "$rev"
done
cleanup
trap - EXIT

python3 "$ROOT/experiments/convergence/analyze_w2_stability.py" "$OUT"
