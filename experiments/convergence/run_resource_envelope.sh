#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ALPENCAT_CONVERGENCE_OUT:-$ROOT/convergence-results}"
BIN="$ROOT/target/release/examples/convergence_workload"

mkdir -p "$OUT_DIR"
: > "$OUT_DIR/results.jsonl"

cargo build --release -p runtime-api --example convergence_workload

{
  echo "git_sha=$(git -C "$ROOT" rev-parse HEAD)"
  echo "utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "uname=$(uname -a)"
  echo "nproc=$(nproc)"
  echo "affinity=$(taskset -pc $$ 2>/dev/null || true)"
  echo "cpu_max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || true)"
  echo "cpuset=$(cat /sys/fs/cgroup/cpuset.cpus.effective 2>/dev/null || true)"
  lscpu 2>/dev/null || true
} > "$OUT_DIR/manifest.txt"

"$BIN" | tee -a "$OUT_DIR/results.jsonl"

if command -v taskset >/dev/null 2>&1; then
  affinity="$(taskset -pc $$ | awk -F: '{gsub(/ /, "", $2); print $2}')"
  first_cpu="${affinity%%[-,]*}"
  taskset -c "$first_cpu" "$BIN" | tee -a "$OUT_DIR/results.jsonl"
fi

python3 - "$OUT_DIR/results.jsonl" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
if not rows:
    raise SystemExit("no convergence workload results")
required = {
    "workload",
    "parallelism",
    "status",
    "measurements",
    "serial_max_items",
    "probe_items",
    "selected_backend",
    "boundary_stale",
    "checksum",
}
for row in rows:
    missing = required.difference(row)
    if missing:
        raise SystemExit(f"missing result fields: {sorted(missing)}")
print(f"validated {len(rows)} resource-envelope result(s)")
PY
