#!/usr/bin/env bash
set -u
OUT="${1:-artifacts}"
mkdir -p "$OUT"
{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "uname=$(uname -a)"
  echo "runner_name=${RUNNER_NAME:-}"
  echo "runner_os=${RUNNER_OS:-}"
  echo "runner_arch=${RUNNER_ARCH:-}"
  echo "github_run_id=${GITHUB_RUN_ID:-}"
  echo "github_sha=${GITHUB_SHA:-}"
  echo "scenario=${SCENARIO:-}"
  echo "cffi_ref=${CFFI_REF:-}"
  echo "cffi_repo=${CFFI_REPO:-}"
  python --version 2>&1
  gcc --version | head -1
  rustc --version 2>/dev/null || true
} > "$OUT/environment.log"
(lscpu || true) > "$OUT/cpu.log" 2>&1
(cat /proc/meminfo || true) > "$OUT/memory-before.log" 2>&1
(free -b || true) >> "$OUT/memory-before.log" 2>&1
(ulimit -a || true) > "$OUT/limits.log" 2>&1
{
  find /sys/fs/cgroup -maxdepth 1 -type f -print | sort | while read f; do
    echo "### $f"; cat "$f" 2>/dev/null || true
  done
} > "$OUT/cgroup.log" 2>&1
(ps -eo pid,ppid,psr,pcpu,pmem,rss,vsz,comm,args --sort=-pcpu | head -80 || true) > "$OUT/processes-before.log"
(lspci -nn || true) > "$OUT/gpu.log" 2>&1
(vulkaninfo --summary || true) >> "$OUT/gpu.log" 2>&1
