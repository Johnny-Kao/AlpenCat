#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?repo root required}"
OUT="${2:?log output required}"
LIB="${3:?bench shared library required}"
GATE_DIR="${4:?Swiss gate library dir required}"

LOGGER="$ROOT/runtime-framework/tools/run_logged_scenario.sh"
BENCH="$ROOT/runtime-framework/experiments/cffi-282/benchmark.py"

run_one() {
  local round="$1"
  local scenario="$2"
  local py mode
  case "$scenario" in
    S1) py=/tmp/cffi-venv-s1/bin/python; mode="" ;;
    S2) py=/tmp/cffi-venv-s2/bin/python; mode="" ;;
    S3) py=/tmp/cffi-venv-swiss-fast/bin/python; mode=1 ;;
    S4) py=/tmp/cffi-venv-swiss-fast/bin/python; mode=2 ;;
    S5) py=/tmp/cffi-venv-swiss-fast/bin/python; mode=3 ;;
    S6) py=/tmp/cffi-venv-swiss-base/bin/python; mode=0 ;;
    *) echo "unknown scenario $scenario" >&2; exit 2 ;;
  esac

  local name
  name=$(printf "r%02d-%s" "$round" "$scenario")
  if [[ -n "$mode" ]]; then
    "$LOGGER" "$name" "$OUT" env       SWISS_CFFI_MODE="$mode" \
      SWISS_CFFI_GATE_LIB="$GATE_DIR/libruntime_cffi_gate.so" \
      LD_LIBRARY_PATH="$GATE_DIR:${LD_LIBRARY_PATH:-}" \
      "$py" "$BENCH" --scenario "$scenario" --matrix-round "$round" --library "$LIB"
  else
    "$LOGGER" "$name" "$OUT"       "$py" "$BENCH" --scenario "$scenario" --matrix-round "$round" --library "$LIB"
  fi
}

orders=(
  "S1 S2 S3 S4 S5 S6"
  "S6 S5 S4 S3 S2 S1"
  "S3 S4 S5 S6 S1 S2"
)

round=1
for order in "${orders[@]}"; do
  for scenario in $order; do
    run_one "$round" "$scenario"
  done
  round=$((round+1))
done
