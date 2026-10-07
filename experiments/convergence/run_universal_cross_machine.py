#!/usr/bin/env python3
import argparse
import json
import math
import multiprocessing as mp
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import time

WORKLOADS = [
    ("w1", "evidence_workload", 32768),
    ("w2", "evidence_memory_workload", 262144),
    ("w3", "evidence_mixed_workload", 262144),
]

REGIMES = [
    ("baseline-full", "none"),
    ("cpu-pressure", "cpu"),
    ("memory-light", "memory-light"),
    ("memory-heavy", "memory-heavy"),
    ("combined", "combined"),
    ("recovery", "none"),
]


def total_memory_bytes():
    if os.name == "nt":
        import ctypes
        class MEMORYSTATUSEX(ctypes.Structure):
            _fields_ = [
                ("dwLength", ctypes.c_ulong),
                ("dwMemoryLoad", ctypes.c_ulong),
                ("ullTotalPhys", ctypes.c_ulonglong),
                ("ullAvailPhys", ctypes.c_ulonglong),
                ("ullTotalPageFile", ctypes.c_ulonglong),
                ("ullAvailPageFile", ctypes.c_ulonglong),
                ("ullTotalVirtual", ctypes.c_ulonglong),
                ("ullAvailVirtual", ctypes.c_ulonglong),
                ("sullAvailExtendedVirtual", ctypes.c_ulonglong),
            ]
        status = MEMORYSTATUSEX()
        status.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
        ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status))
        return int(status.ullTotalPhys)
    try:
        return int(os.sysconf("SC_PAGE_SIZE")) * int(os.sysconf("SC_PHYS_PAGES"))
    except (ValueError, OSError, AttributeError):
        return 0


def cpu_burner():
    x = 0x123456789ABCDEF
    while True:
        x ^= (x << 13) & ((1 << 64) - 1)
        x ^= x >> 7
        x ^= (x << 17) & ((1 << 64) - 1)


def memory_burner(mebibytes):
    size = max(1, int(mebibytes)) * 1024 * 1024
    buf = bytearray(size)
    page = 4096
    while True:
        for i in range(0, size, page):
            buf[i] = (buf[i] + 1) & 0xFF
        time.sleep(0.05)


def pressure_memory_mib(kind, total_bytes):
    total_mib = total_bytes / (1024 * 1024) if total_bytes else 0.0
    if kind == "memory-light":
        return int(max(64, min(384, total_mib * 0.05)))
    if kind == "memory-heavy":
        return int(max(128, min(1024, total_mib * 0.15)))
    if kind == "combined":
        return int(max(96, min(768, total_mib * 0.10)))
    return 0


def start_pressure(kind, total_bytes):
    procs = []
    cpu_count = max(1, os.cpu_count() or 1)
    if kind in ("cpu", "combined"):
        burners = max(1, cpu_count // 2)
        for _ in range(burners):
            p = mp.Process(target=cpu_burner)
            p.start()
            procs.append(p)
    mem_mib = pressure_memory_mib(kind, total_bytes)
    if mem_mib > 0:
        p = mp.Process(target=memory_burner, args=(mem_mib,))
        p.start()
        procs.append(p)
    if procs:
        time.sleep(1.0)
    return procs, mem_mib


def stop_pressure(procs):
    for p in procs:
        p.terminate()
    for p in procs:
        p.join(timeout=5)


def executable(root, example):
    suffix = ".exe" if os.name == "nt" else ""
    return root / "target" / "release" / "examples" / f"{example}{suffix}"


def build_examples(root):
    cmd = ["cargo", "build", "--release", "-p", "runtime-api"]
    for _, example, _ in WORKLOADS:
        cmd += ["--example", example]
    subprocess.run(cmd, cwd=root, check=True)


def parse_boundary(path, fallback):
    for line in path.read_text().splitlines():
        row = json.loads(line)
        if row.get("record_type") == "revalidation":
            return int(row.get("published_serial_max_items", fallback))
    return fallback


def run_example(binary, out_path, regime, start_boundary, bootstrap, repeats):
    env = os.environ.copy()
    env.update({
        "ALPENCAT_REGIME": regime,
        "ALPENCAT_REPEATS": str(repeats),
        "ALPENCAT_WARMUP_PAIRS": "1",
        "ALPENCAT_START_BOUNDARY": str(start_boundary),
        "ALPENCAT_BOOTSTRAP": "1" if bootstrap else "0",
        "ALPENCAT_REVALIDATION_POINTS": "3",
    })
    with out_path.open("w", encoding="utf-8") as handle:
        subprocess.run([str(binary)], env=env, stdout=handle, check=True)


def command_output(cmd):
    try:
        return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except Exception:
        return ""


def hardware_manifest(total_bytes):
    system = platform.system()
    cpu_model = os.environ.get("PROCESSOR_IDENTIFIER", "")
    if system == "Linux":
        cpu_model = command_output(["bash", "-lc", "grep -m1 'model name' /proc/cpuinfo | cut -d: -f2-"]) or cpu_model
    elif system == "Darwin":
        cpu_model = command_output(["sysctl", "-n", "machdep.cpu.brand_string"]) or platform.processor()
    return {
        "runner_name": os.environ.get("RUNNER_NAME"),
        "runner_os": os.environ.get("RUNNER_OS"),
        "runner_arch": os.environ.get("RUNNER_ARCH"),
        "image_os": os.environ.get("ImageOS"),
        "image_version": os.environ.get("ImageVersion"),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "cpu_model": cpu_model.strip(),
        "logical_cpus": os.cpu_count(),
        "total_memory_bytes": total_bytes,
        "total_memory_gib": total_bytes / (1024 ** 3) if total_bytes else None,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--calls-per-point", type=int, default=100)
    args = parser.parse_args()

    root = args.root.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    total_bytes = total_memory_bytes()

    manifest = hardware_manifest(total_bytes)
    manifest["pressure_profiles"] = {
        kind: {"memory_mib": pressure_memory_mib(kind, total_bytes)}
        for kind in ("memory-light", "memory-heavy", "combined")
    }
    (out / "hardware-manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")

    build_examples(root)

    for short, example, fallback_boundary in WORKLOADS:
        workload_dir = out / short
        workload_dir.mkdir(parents=True, exist_ok=True)
        binary = executable(root, example)

        current_boundary = fallback_boundary
        combined_boundary = fallback_boundary

        for regime, pressure_kind in REGIMES:
            bootstrap = regime == "baseline-full"
            if regime == "recovery":
                start_boundary = combined_boundary
            else:
                start_boundary = current_boundary

            procs = []
            try:
                procs, mem_mib = start_pressure(pressure_kind, total_bytes)
                print(
                    f"[universal] workload={short} regime={regime} pressure={pressure_kind} "
                    f"mem_mib={mem_mib} start_boundary={start_boundary}",
                    flush=True,
                )
                output = workload_dir / f"{regime}.jsonl"
                run_example(binary, output, regime, start_boundary, bootstrap, args.repeats)
                published = parse_boundary(output, start_boundary)
                if regime == "baseline-full":
                    current_boundary = published
                if regime == "combined":
                    combined_boundary = published
            finally:
                stop_pressure(procs)
            time.sleep(0.5)

        with (workload_dir / "evidence.jsonl").open("w", encoding="utf-8") as merged:
            for regime, _ in REGIMES:
                merged.write((workload_dir / f"{regime}.jsonl").read_text())

        subprocess.run(
            [
                sys.executable,
                str(root / "experiments" / "convergence" / "analyze_economics.py"),
                str(workload_dir),
                "--calls-per-point",
                str(args.calls_per_point),
            ],
            cwd=root,
            check=True,
        )

    print(out)


if __name__ == "__main__":
    mp.freeze_support()
    main()
