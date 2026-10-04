#!/usr/bin/env bash
set -uo pipefail

LISTEN_SECONDS=60

usage() {
  cat <<'USAGE'
Usage: alpencat-native-probe.sh [--listen-seconds N]

Passive native-validation probe for AlpenCat.
It collects CPU/kernel metadata and listens for Linux thermal Generic Netlink
CPU capability-change events. It does not modify platform settings.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --listen-seconds)
      LISTEN_SECONDS="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! [[ "$LISTEN_SECONDS" =~ ^[0-9]+$ ]] || [[ "$LISTEN_SECONDS" -lt 1 ]]; then
  echo "--listen-seconds must be a positive integer" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
HOST="$(hostname 2>/dev/null | tr -c 'A-Za-z0-9._-' '_' || echo unknown)"
OUT="alpencat-native-validation-${STAMP}-${HOST}"
mkdir -p "$OUT"

SYSTEM="$OUT/system.txt"
CPUID_OUT="$OUT/cpuid_hfi.txt"
KCONFIG="$OUT/kernel_config.txt"
DMESG_OUT="$OUT/dmesg_hfi.txt"
EVENTS="$OUT/thermal_events.jsonl"
LISTENER_ERR="$OUT/listener.stderr"
SUMMARY="$OUT/summary.txt"

{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "hostname=$(hostname 2>/dev/null || true)"
  echo "uid=$(id -u)"
  echo
  uname -a || true
  echo
  command -v lscpu >/dev/null 2>&1 && lscpu || true
  echo
  echo "--- /proc/cpuinfo first processor ---"
  awk 'BEGIN{RS=""} NR==1{print; exit}' /proc/cpuinfo 2>/dev/null || true
  echo
  echo "--- cgroup cpu.max ---"
  cat /sys/fs/cgroup/cpu.max 2>/dev/null || true
  echo
  echo "--- cpuset effective ---"
  cat /sys/fs/cgroup/cpuset.cpus.effective 2>/dev/null || true
} > "$SYSTEM"

CC_BIN="${CC:-cc}"

CPUID_STATUS="compile_failed"
if command -v "$CC_BIN" >/dev/null 2>&1; then
  if "$CC_BIN" -O2 -Wall -Wextra -Werror       "$SCRIPT_DIR/cpuid_hfi.c" -o "$OUT/cpuid_hfi"       >>"$CPUID_OUT" 2>&1; then
    CPUID_STATUS="ran"
    "$OUT/cpuid_hfi" >>"$CPUID_OUT" 2>&1 || true
  fi
else
  echo "C compiler not found: $CC_BIN" >"$CPUID_OUT"
fi

{
  echo "kernel=$(uname -r)"
  if [[ -r "/boot/config-$(uname -r)" ]]; then
    grep -E '^(CONFIG_INTEL_HFI_THERMAL|CONFIG_THERMAL_NETLINK|CONFIG_X86_THERMAL_VECTOR)='       "/boot/config-$(uname -r)" || true
  elif [[ -r /proc/config.gz ]]; then
    zgrep -E '^(CONFIG_INTEL_HFI_THERMAL|CONFIG_THERMAL_NETLINK|CONFIG_X86_THERMAL_VECTOR)='       /proc/config.gz || true
  else
    echo "kernel config not readable from /boot/config-* or /proc/config.gz"
  fi
} > "$KCONFIG"

if command -v dmesg >/dev/null 2>&1; then
  dmesg 2>&1 | grep -Ei 'hardware feedback|intel.*hfi|\bhfi\b|thermal.*capab'     >"$DMESG_OUT" || true
else
  echo "dmesg unavailable" >"$DMESG_OUT"
fi

LISTENER_STATUS="compile_failed"
: >"$EVENTS"
: >"$LISTENER_ERR"

if command -v "$CC_BIN" >/dev/null 2>&1; then
  if "$CC_BIN" -O2 -Wall -Wextra -Werror       "$SCRIPT_DIR/thermal_event_listener.c"       -o "$OUT/thermal_event_listener"       >>"$LISTENER_ERR" 2>&1; then
    LISTENER_STATUS="compiled"
    echo "Listening for CPU capability-change events for ${LISTEN_SECONDS}s..." >&2
    "$OUT/thermal_event_listener" "$LISTEN_SECONDS"       >"$EVENTS" 2>>"$LISTENER_ERR"
    RC=$?
    case "$RC" in
      0) LISTENER_STATUS="completed" ;;
      3) LISTENER_STATUS="thermal_netlink_unavailable" ;;
      4) LISTENER_STATUS="membership_failed" ;;
      *) LISTENER_STATUS="listener_error_${RC}" ;;
    esac
  fi
fi

HFI_SUPPORTED="unknown"
if grep -q '^hfi_supported=1$' "$CPUID_OUT" 2>/dev/null; then
  HFI_SUPPORTED="yes"
elif grep -q '^hfi_supported=0$' "$CPUID_OUT" 2>/dev/null; then
  HFI_SUPPORTED="no"
fi

EVENT_COUNT="$(wc -l <"$EVENTS" | tr -d ' ')"

if [[ "$EVENT_COUNT" -gt 0 ]]; then
  OVERALL="EVENT_OBSERVED"
elif [[ "$HFI_SUPPORTED" == "yes" && "$LISTENER_STATUS" == "completed" ]]; then
  OVERALL="SUPPORTED_NO_EVENT_OBSERVED"
elif [[ "$HFI_SUPPORTED" == "no" ]]; then
  OVERALL="CPU_HFI_UNSUPPORTED"
else
  OVERALL="INCOMPLETE_OR_UNSUPPORTED_ENVIRONMENT"
fi

{
  echo "AlpenCat native validation probe"
  echo "overall_status=$OVERALL"
  echo "hfi_cpuid_support=$HFI_SUPPORTED"
  echo "cpuid_probe_status=$CPUID_STATUS"
  echo "thermal_listener_status=$LISTENER_STATUS"
  echo "cpu_capability_events=$EVENT_COUNT"
  echo "listen_seconds=$LISTEN_SECONDS"
  echo
  echo "This probe is passive. It did not modify CPU, BIOS, BMC, SST, power, or thermal settings."
} >"$SUMMARY"

rm -f "$OUT/cpuid_hfi" "$OUT/thermal_event_listener"

ARCHIVE="$OUT.tar.gz"
tar -czf "$ARCHIVE" "$OUT"

cat "$SUMMARY"
echo
echo "Evidence directory: $OUT"
echo "Evidence archive:   $ARCHIVE"
