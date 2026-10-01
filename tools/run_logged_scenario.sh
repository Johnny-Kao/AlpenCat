#!/usr/bin/env bash
set -euo pipefail

NAME="${1:?scenario name required}"
OUT="${2:?output directory required}"
shift 2
mkdir -p "$OUT/scenarios/$NAME"

DIR="$OUT/scenarios/$NAME"
START_NS=$(date +%s%N)

# Sample host pressure while the scenario runs.
vmstat 1 > "$DIR/vmstat.log" 2>&1 &
VMSTAT_PID=$!

set +e
/usr/bin/time -v -o "$DIR/time.txt" "$@"   >"$DIR/stdout.log" 2>"$DIR/stderr.log"
STATUS=$?
set -e

kill "$VMSTAT_PID" >/dev/null 2>&1 || true
wait "$VMSTAT_PID" 2>/dev/null || true

END_NS=$(date +%s%N)
python3 - <<'PY' "$DIR/result.json" "$NAME" "$STATUS" "$START_NS" "$END_NS"
import json,sys
path,name,status,start,end=sys.argv[1:]
data={
  "scenario": name,
  "exit_code": int(status),
  "start_ns": int(start),
  "end_ns": int(end),
  "elapsed_ns": int(end)-int(start),
}
with open(path,"w") as f: json.dump(data,f,indent=2)
PY

exit "$STATUS"
