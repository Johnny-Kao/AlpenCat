#!/usr/bin/env python3
"""Real quota service; same-workload bounded policy vs fixed/OS controls."""
import argparse
import csv
import json
import os
import random
import socket
import statistics
import subprocess
import tempfile
import time

MODES = ("fixed1", "fixed2", "fixed4", "fixed8", "direct", "alpencat", "direct_search", "alpencat_search")
SIZES = ("short", "long")
QUOTA = (2000, 500, 2000)
ROUNDS = 5


def command(*args):
    r = subprocess.run(args, capture_output=True, text=True, check=True, timeout=20)
    return r.stdout.strip()


def read_stat(path):
    d = {}
    for line in (path / "cpu.stat").read_text().splitlines():
        parts = line.split()
        if len(parts) == 2:
            d[parts[0]] = int(parts[1])
    return d


def assert_quota(path, expected):
    fields = (path / "cpu.max").read_text().split()
    assert len(fields) == 2 and fields[0] != "max", f"not a finite CPU quota: {fields}"
    capacity = round(1000 * int(fields[0]) / int(fields[1]))
    if abs(capacity - expected) > 1:
        raise AssertionError(f"quota in cgroup not applied: {capacity} vs {expected} milli-CPU")
    return capacity


def timed_unit(binary, mode, size, unit_index, uid):
    import pathlib
    name = f"aq-{os.environ.get('GITHUB_RUN_ID', 'manual')}-{unit_index}"
    unit = name + ".service"
    phase_rows = []
    with tempfile.TemporaryDirectory(prefix="aq-") as temp:
        sock_path = os.path.join(temp, "control.sock")
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.settimeout(20)
        listener.bind(sock_path)
        listener.listen(1)
        running = False
        try:
            command("sudo", "-n", "systemd-run", "--quiet", "--unit", name,
                    "--uid", uid, "--property=CPUQuota=200%",
                    "--property=KillMode=control-group",
                    os.path.abspath(binary), mode, size, sock_path)
            running = True
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(30)
                lines = conn.makefile("r", encoding="utf-8")
                hello = lines.readline().strip()
                if not hello.startswith("READY pid="):
                    raise RuntimeError(f"unexpected worker handshake: {hello}")
                pid = int(hello.partition("=")[2])
                cg = command("sudo", "systemctl", "show", unit, "-P", "ControlGroup")
                if not cg.startswith("/"):
                    raise RuntimeError(f"transient service has no cgroup: {cg}")
                cgroup_path = pathlib.Path("/sys/fs/cgroup" + cg)
                if not cgroup_path.is_dir():
                    raise RuntimeError(f"cgroup does not exist: {cgroup_path}")
                # Confirm the PID is in our own transient service's cgroup:
                membership = pathlib.Path(f"/proc/{pid}/cgroup").read_text()
                if cg not in membership:
                    raise AssertionError(f"worker not attached to owned cgroup: {membership!r} / {cg}")
                for phase, target in enumerate(QUOTA):
                    # Switch only this dedicated service; the runner and other jobs remain untouched.
                    command("sudo", "systemctl", "set-property", "--runtime", unit,
                            f"CPUQuota={target/10:g}%")
                    applied = assert_quota(cgroup_path, target)
                    before = read_stat(cgroup_path)
                    # Controller transition cost intentionally excluded from request latency;
                    # equal external notification timing for every mode.
                    conn.sendall(f"GO {target}\n".encode())
                    response = lines.readline().strip()
                    if not response.startswith("PHASE "):
                        raise RuntimeError(f"no phase response: {mode} {size} phase={phase}: {response!r}")
                    result = dict(part.split("=", 1) for part in response.split()[1:])
                    if (int(result["phase"]) != phase
                            or result["size"] != size or result["mode"] != mode
                            or int(result["quota_milli"]) != applied):
                        raise AssertionError(f"service did not observe real quota {result}")
                    after = read_stat(cgroup_path)
                    quota_throttles = after.get("nr_throttled", 0) - before.get("nr_throttled", 0)
                    if quota_throttles < 0:
                        raise AssertionError("nonmonotonic throttling counter")
                    phase_rows.append({
                        "phase": phase, "quota_milli": target, "workers": int(result["workers"]),
                        "requests": int(result["requests"]),
                        "avg_workers": float(result["avg_workers"]),
                        "switches": int(result["switches"]),
                        "rollbacks": int(result["rollbacks"]),
                        "probes": int(result["probes"]),
                        "checksum": result["checksum"],
                        "wall_ms": int(result["wall_ns"]) / 1e6,
                        "cpu_ms": int(result["cpu_ns"]) / 1e6,
                        "p95_ms": int(result["p95_ns"]) / 1e6,
                        "observe_ms": int(result["observe_ns"]) / 1e6,
                        "nr_throttled_delta": quota_throttles,
                        "throttled_ms": (after.get("throttled_usec", 0)
                                         - before.get("throttled_usec", 0)) / 1000,
                        "cgroup_usage_ms": (after.get("usage_usec", 0)
                                            - before.get("usage_usec", 0)) / 1000,
                    })
                # A real throttled period must be seen under the restricted 0.5 CPU phase.
                if phase_rows[1]["nr_throttled_delta"] <= 0:
                    raise AssertionError("quota 0.5 CPU produced no throttling: unsuitable workload")
                if phase_rows[0]["quota_milli"] != phase_rows[2]["quota_milli"]:
                    raise AssertionError("failed to restore quota")
                lines.close()
                # The process terminates when the socket closes; no persistent service.
        except Exception:
            try:
                logs = command("sudo", "journalctl", "-u", unit, "-n", "12", "--no-pager")
                print("WORKER_LOGS", name, logs, flush=True)
            except Exception:
                pass
            raise
        finally:
            if running:
                subprocess.run(["sudo", "-n", "systemctl", "stop", unit],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
            listener.close()
    return phase_rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", default="quota-results.json")
    args = parser.parse_args()
    user = command("id", "-un")
    rng = random.Random(20261024)
    rows = []
    serial = 0
    for size in SIZES:
        # Minimal warmup once per mode; all real cgroup quotas and computations.
        for mode in MODES:
            timed_unit(args.binary, mode, size, serial, user)
            serial += 1
        for round_index in range(ROUNDS):
            sequence = list(MODES)
            rng.shuffle(sequence)
            block = {}
            for mode in sequence:
                phases = timed_unit(args.binary, mode, size, serial, user)
                serial += 1
                block[mode] = phases
                for p in phases:
                    rows.append(dict(size=size, round=round_index, mode=mode, **p))
            for phase in range(3):
                checks = {block[mode][phase]["checksum"] for mode in MODES}
                if len(checks) != 1:
                    raise AssertionError(f"nondeterministic result {size} round={round_index} phase={phase}: {checks}")
            for mode in ("direct", "alpencat"):
                if [x["workers"] for x in block[mode]] != [2, 1, 2]:
                    raise AssertionError(f"quota-only worker adjustment incorrect: {mode} {size} {block[mode]}")
            for mode in ("direct_search", "alpencat_search"):
                if [x["probes"] for x in block[mode]] != [8,8,8]:
                    raise AssertionError(f"bounded policy did not evaluate eight real request windows: {mode}")
                if any(not 1 <= x["avg_workers"] <= 8 for x in block[mode]):
                    raise AssertionError("impossible worker count")
    report = {
        "experiment": "bounded-p95-cpu-policy-real-cgroup-2-to-0.5-to-2",
        "rounds": ROUNDS,
        "quota_milli": QUOTA,
        "runner": os.uname().release,
        "policy": {"candidate_workers": [1,2,4,8], "exploration_windows":8,
                   "total_windows":12,"cpu_overhead_allowance_pct":4,
                   "switch_hysteresis_pct":5,"rollback_threshold_pct":25},
        "raw": rows,
        "scenarios": {}
    }
    for size in SIZES:
        selected = [r for r in rows if r["size"] == size]
        totals = {}
        for mode in MODES:
            mine = [r for r in selected if r["mode"] == mode]
            totals[mode] = {}
            for metric in ("wall_ms", "cpu_ms", "p95_ms", "observe_ms", "nr_throttled_delta", "switches", "rollbacks"):
                values = [sum(r[metric] for r in mine if r["round"] == i) for i in range(ROUNDS)]
                totals[mode][metric] = {
                    "median": round(statistics.median(values), 4), "by_round": values
                }
        comparisons = {}
        for baseline in ("fixed1", "fixed2", "fixed4", "fixed8", "direct", "alpencat", "direct_search", "best_fixed"):
            comparisons[baseline] = {}
            for metric in ("wall_ms", "cpu_ms", "p95_ms"):
                al = totals["alpencat_search"][metric]["by_round"]
                b = ([min(totals[m][metric]["by_round"][i] for m in ("fixed1", "fixed2", "fixed4", "fixed8"))
                      for i in range(ROUNDS)] if baseline == "best_fixed" else totals[baseline][metric]["by_round"])
                gains = [(bb - aa) / bb * 100 for aa, bb in zip(al, b)]
                comparisons[baseline][metric] = {
                    "median_gain_pct": round(statistics.median(gains), 3),
                    "gain_by_round_pct": [round(g, 2) for g in gains],
                    "above_five_pct": sum(g > 5 for g in gains),
                    "above_two_pct": sum(g > 2 for g in gains)
                }
        # User-facing proposal: p95 improvement >5% 4/5 paired rounds
        # versus best fixed, no CPU regression >2%, and independent
        # incremental win vs same-policy direct snapshot implementation.
        best = comparisons["best_fixed"]
        direct = comparisons["direct_search"]
        stable = (best["p95_ms"]["above_five_pct"] >= 4
                  and best["cpu_ms"]["median_gain_pct"] >= -2.0
                  and direct["p95_ms"]["above_two_pct"] >= 4
                  and direct["cpu_ms"]["median_gain_pct"] >= -2.0)
        report["scenarios"][size] = {
            "median_by_mode": {mode: {k: v["median"] for k, v in value.items()}
                               for mode, value in totals.items()},
            "comparisons": comparisons,
            "stable_distinct_gain": stable,
            "stable_policy_gain_vs_quota": (
                comparisons["alpencat"]["p95_ms"]["above_five_pct"] >= 4
                and comparisons["alpencat"]["cpu_ms"]["median_gain_pct"] >= -2.0),
            "restricted_phase_throttling": {
                mode: [
                    r["nr_throttled_delta"] for r in selected
                    if r["mode"] == mode and r["phase"] == 1
                ] for mode in MODES
            }
        }
        print(f"SCENARIO size={size} real_quota=2->0.5->2", flush=True)
        for mode in MODES:
            m = report["scenarios"][size]["median_by_mode"][mode]
            print(f"MEDIAN size={size} mode={mode} wall_ms={m['wall_ms']:.3f} "
                  f"cpu_ms={m['cpu_ms']:.3f} p95_sum_ms={m['p95_ms']:.3f} "
                  f"throttled_periods={m['nr_throttled_delta']:.1f} "
                  f"switches={m['switches']:.1f} rollbacks={m['rollbacks']:.1f}", flush=True)
        for baseline, metrics in comparisons.items():
            print(f"GAIN size={size} baseline={baseline} "
                  f"wall_pct={metrics['wall_ms']['median_gain_pct']:.2f} "
                  f"cpu_pct={metrics['cpu_ms']['median_gain_pct']:.2f} "
                  f"p95_pct={metrics['p95_ms']['median_gain_pct']:.2f} "
                  f"p95_above5={metrics['p95_ms']['above_five_pct']}/5", flush=True)
        print(f"VERDICT size={size} distinct_stable_gain={stable} "
              f"policy_gain_vs_quota={report['scenarios'][size]['stable_policy_gain_vs_quota']}", flush=True)
    overall = all(v["stable_distinct_gain"] for v in report["scenarios"].values())
    report["overall_distinct_stable_gain"] = overall
    print(f"OVERALL_DISTINCT_STABLE_GAIN={str(overall).upper()}", flush=True)
    print("NO-GO is a valid scientific finding, not a CI failure.", flush=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
    with open(args.output.replace(".json", ".csv"), "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
