#!/usr/bin/env bash
set -u
OUT="$1"
PID="$2"
: > "$OUT/resource-timeseries.log"
while kill -0 "$PID" 2>/dev/null; do
  ts=$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)
  load=$(cat /proc/loadavg 2>/dev/null || true)
  mem=$(awk '/MemAvailable:|MemFree:|SwapFree:/{printf "%s=%s ",$1,$2}' /proc/meminfo 2>/dev/null)
  proc=$(ps -o pid=,psr=,pcpu=,pmem=,rss=,vsz= -p "$PID" 2>/dev/null | xargs || true)
  echo "$ts load=[$load] mem=[$mem] proc=[$proc]" >> "$OUT/resource-timeseries.log"
  sleep 1
done
