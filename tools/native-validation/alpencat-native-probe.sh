#!/usr/bin/env bash
set -uo pipefail

LISTEN_SECONDS=300
TRANSITION_NOTE=""

usage() {
  cat <<'USAGE'
Usage: alpencat-native-probe.sh [--listen-seconds N] [--transition-note TEXT]

Passive one-shot native-validation probe for AlpenCat.
It performs compilation and environment checks before listening. It collects
CPU/kernel/platform metadata and Linux thermal Generic Netlink CPU capability
change events. It does not modify platform settings.

Trigger the approved platform transition only after READY_FOR_TRANSITION appears.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --listen-seconds)
      LISTEN_SECONDS="${2:-}"
      shift 2
      ;;
    --transition-note)
      TRANSITION_NOTE="${2:-}"
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
DMESG_BEFORE="$OUT/dmesg_before.txt"
DMESG_AFTER="$OUT/dmesg_after.txt"
PLATFORM_BEFORE="$OUT/platform_before.txt"
PLATFORM_AFTER="$OUT/platform_after.txt"
EVENTS="$OUT/thermal_events.jsonl"
LISTENER_ERR="$OUT/listener.stderr"
SUMMARY="$OUT/summary.txt"
MANIFEST="$OUT/manifest.txt"

capture_platform_state() {
  local target="$1"
  {
    echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
    echo "hostname=$(hostname 2>/dev/null || true)"
    echo "uid=$(id -u)"
    echo "git_sha=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "transition_note=$TRANSITION_NOTE"
    echo
    uname -a || true
    echo

    if [[ -r /etc/os-release ]]; then
      echo "--- os-release ---"
      cat /etc/os-release
      echo
    fi

    if command -v lscpu >/dev/null 2>&1; then
      echo "--- lscpu ---"
      lscpu || true
      echo
      echo "--- lscpu topology ---"
      lscpu -e 2>/dev/null || true
      echo
    fi

    echo "--- kernel cmdline ---"
    cat /proc/cmdline 2>/dev/null || true
    echo
    echo "--- cgroup cpu.max ---"
    cat /sys/fs/cgroup/cpu.max 2>/dev/null || true
    echo
    echo "--- cpuset effective ---"
    cat /sys/fs/cgroup/cpuset.cpus.effective 2>/dev/null || true
    echo
    echo "--- cpu online/offline ---"
    cat /sys/devices/system/cpu/online 2>/dev/null || true
    cat /sys/devices/system/cpu/offline 2>/dev/null || true
    echo
    echo "--- smt ---"
    cat /sys/devices/system/cpu/smt/active 2>/dev/null || true
    cat /sys/devices/system/cpu/smt/control 2>/dev/null || true
    echo

    echo "--- virtualization ---"
    if command -v systemd-detect-virt >/dev/null 2>&1; then
      systemd-detect-virt || true
    else
      echo "systemd-detect-virt unavailable"
    fi
    echo

    echo "--- DMI (non-unique fields only) ---"
    for f in sys_vendor product_name product_version bios_vendor bios_version bios_date; do
      p="/sys/class/dmi/id/$f"
      printf '%s=' "$f"
      if [[ -r "$p" ]]; then cat "$p"; else echo "unavailable"; fi
    done
    echo

    echo "--- intel_pstate ---"
    if [[ -d /sys/devices/system/cpu/intel_pstate ]]; then
      for p in /sys/devices/system/cpu/intel_pstate/*; do
        [[ -f "$p" && -r "$p" ]] || continue
        printf '%s=' "$(basename "$p")"
        cat "$p" 2>/dev/null || true
      done
    else
      echo "unavailable"
    fi
    echo

    echo "--- cpufreq policies ---"
    if compgen -G "/sys/devices/system/cpu/cpufreq/policy*" >/dev/null; then
      for policy in /sys/devices/system/cpu/cpufreq/policy*; do
        echo "[$(basename "$policy")]"
        for f in affected_cpus scaling_driver scaling_governor scaling_min_freq scaling_max_freq cpuinfo_min_freq cpuinfo_max_freq; do
          if [[ -r "$policy/$f" ]]; then
            printf '%s=' "$f"
            cat "$policy/$f" 2>/dev/null || true
          fi
        done
      done
    else
      echo "unavailable"
    fi
    echo

    echo "--- thermal zones ---"
    if compgen -G "/sys/class/thermal/thermal_zone*" >/dev/null; then
      for zone in /sys/class/thermal/thermal_zone*; do
        printf '%s ' "$(basename "$zone")"
        for f in type policy mode temp; do
          if [[ -r "$zone/$f" ]]; then
            printf '%s=' "$f"
            tr '\n' ' ' < "$zone/$f"
          fi
        done
        echo
      done
    else
      echo "unavailable"
    fi
    echo

    echo "--- relevant loaded modules ---"
    if command -v lsmod >/dev/null 2>&1; then
      lsmod | grep -Ei 'intel|thermal|hfi|isst|pstate' || true
    else
      echo "lsmod unavailable"
    fi
    echo

    echo "--- tool availability ---"
    for tool in intel-speed-select turbostat cpupower; do
      if command -v "$tool" >/dev/null 2>&1; then
        echo "$tool=$(command -v "$tool")"
      else
        echo "$tool=unavailable"
      fi
    done
  } > "$target"
}

capture_filtered_dmesg() {
  local target="$1"
  if command -v dmesg >/dev/null 2>&1; then
    dmesg 2>&1 | grep -Ei 'hardware feedback|intel.*hfi|\bhfi\b|thermal.*capab|speed select|isst' > "$target" || true
  else
    echo "dmesg unavailable" > "$target"
  fi
}

capture_platform_state "$PLATFORM_BEFORE"
capture_filtered_dmesg "$DMESG_BEFORE"

CC_BIN=""
for candidate in "${CC:-}" cc gcc clang; do
  [[ -n "$candidate" ]] || continue
  if command -v "$candidate" >/dev/null 2>&1; then
    CC_BIN="$candidate"
    break
  fi
done

CPUID_STATUS="compiler_unavailable"
if [[ -n "$CC_BIN" ]]; then
  CPUID_STATUS="compile_failed"
  if "$CC_BIN" -O2 -Wall -Wextra -Werror       "$SCRIPT_DIR/cpuid_hfi.c" -o "$OUT/cpuid_hfi"       >>"$CPUID_OUT" 2>&1; then
    CPUID_STATUS="ran"
    "$OUT/cpuid_hfi" >>"$CPUID_OUT" 2>&1 || true
  fi
else
  echo "No C compiler found (tried CC, cc, gcc, clang)." >"$CPUID_OUT"
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

if [[ -n "$CC_BIN" ]]; then
  if "$CC_BIN" -O2 -Wall -Wextra -Werror       "$SCRIPT_DIR/thermal_event_listener.c"       -o "$OUT/thermal_event_listener"       >>"$LISTENER_ERR" 2>&1; then
    LISTENER_STATUS="compiled"
    echo "READY_FOR_TRANSITION" >&2
    echo "Listening passively for CPU capability-change events for ${LISTEN_SECONDS}s..." >&2
    echo "Trigger the approved platform transition now." >&2
    LISTEN_START="$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
    "$OUT/thermal_event_listener" "$LISTEN_SECONDS"       >"$EVENTS" 2>>"$LISTENER_ERR"
    RC=$?
    LISTEN_END="$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
    {
      echo "listen_start_utc=$LISTEN_START"
      echo "listen_end_utc=$LISTEN_END"
    } >>"$LISTENER_ERR"
    case "$RC" in
      0) LISTENER_STATUS="completed" ;;
      3) LISTENER_STATUS="thermal_netlink_unavailable" ;;
      4) LISTENER_STATUS="membership_failed" ;;
      *) LISTENER_STATUS="listener_error_${RC}" ;;
    esac
  fi
fi

capture_platform_state "$PLATFORM_AFTER"
capture_filtered_dmesg "$DMESG_AFTER"

HFI_SUPPORTED="unknown"
if grep -q '^hfi_supported=1$' "$CPUID_OUT" 2>/dev/null; then
  HFI_SUPPORTED="yes"
elif grep -q '^hfi_supported=0$' "$CPUID_OUT" 2>/dev/null; then
  HFI_SUPPORTED="no"
fi

EVENT_COUNT="$(wc -l <"$EVENTS" | tr -d ' ')"
LISTENER_READY="no"
if grep -q '^listening family=' "$LISTENER_ERR" 2>/dev/null; then
  LISTENER_READY="yes"
fi

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
  echo "thermal_listener_ready=$LISTENER_READY"
  echo "cpu_capability_events=$EVENT_COUNT"
  echo "listen_seconds=$LISTEN_SECONDS"
  echo "transition_note=$TRANSITION_NOTE"
  echo "git_sha=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo
  echo "This probe is passive. It did not modify CPU, BIOS, BMC, SST, power, or thermal settings."
} >"$SUMMARY"

{
  echo "probe_finished_utc=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
  echo "git_sha=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "probe_script_sha256=$(sha256sum "$SCRIPT_DIR/alpencat-native-probe.sh" 2>/dev/null | awk '{print $1}' || true)"
  echo "listener_source_sha256=$(sha256sum "$SCRIPT_DIR/thermal_event_listener.c" 2>/dev/null | awk '{print $1}' || true)"
  echo "cpuid_source_sha256=$(sha256sum "$SCRIPT_DIR/cpuid_hfi.c" 2>/dev/null | awk '{print $1}' || true)"
  echo "transition_note=$TRANSITION_NOTE"
} >"$MANIFEST"

rm -f "$OUT/cpuid_hfi" "$OUT/thermal_event_listener"

ARCHIVE="$OUT.tar.gz"
tar -czf "$ARCHIVE" "$OUT"

cat "$SUMMARY"
echo
echo "Evidence directory: $OUT"
echo "Evidence archive:   $ARCHIVE"
