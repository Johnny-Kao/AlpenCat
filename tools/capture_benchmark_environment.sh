#!/usr/bin/env bash
set -euo pipefail

OUT="${1:?output directory required}"
mkdir -p "$OUT"

python3 - <<'PY' "$OUT/environment.json"
import json, os, platform, subprocess, sys, time
out=sys.argv[1]
def cmd(*args):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
    except Exception as e:
        return f"<unavailable: {e}>"
data={
  "timestamp_unix": time.time(),
  "github": {k: os.environ.get(k) for k in [
    "GITHUB_RUN_ID","GITHUB_RUN_ATTEMPT","GITHUB_SHA","GITHUB_REF",
    "GITHUB_REPOSITORY","RUNNER_NAME","RUNNER_OS","RUNNER_ARCH"
  ]},
  "platform": {
    "system": platform.system(),
    "release": platform.release(),
    "machine": platform.machine(),
    "python": platform.python_version(),
  },
  "cpu": {
    "nproc": os.cpu_count(),
    "lscpu": cmd("lscpu"),
  },
  "memory": {
    "free": cmd("free","-b"),
    "meminfo": open("/proc/meminfo").read() if os.path.exists("/proc/meminfo") else None,
  },
  "limits": cmd("bash","-lc","ulimit -a"),
  "cgroup": {
    "cpu_max": open("/sys/fs/cgroup/cpu.max").read().strip() if os.path.exists("/sys/fs/cgroup/cpu.max") else None,
    "memory_max": open("/sys/fs/cgroup/memory.max").read().strip() if os.path.exists("/sys/fs/cgroup/memory.max") else None,
    "pids_max": open("/sys/fs/cgroup/pids.max").read().strip() if os.path.exists("/sys/fs/cgroup/pids.max") else None,
  },
}
with open(out,"w") as f:
    json.dump(data,f,indent=2)
PY

{
  echo "=== uname ==="; uname -a
  echo "=== lscpu ==="; lscpu || true
  echo "=== free -h ==="; free -h || true
  echo "=== df -h ==="; df -h || true
  echo "=== ulimit ==="; ulimit -a || true
  echo "=== /proc/loadavg ==="; cat /proc/loadavg || true
  echo "=== top snapshot ==="; top -b -n1 | head -80 || true
} > "$OUT/system.txt"

if command -v vulkaninfo >/dev/null 2>&1; then
  vulkaninfo --summary > "$OUT/vulkan.txt" 2>&1 || true
else
  echo "vulkaninfo unavailable" > "$OUT/vulkan.txt"
fi
