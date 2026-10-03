#!/usr/bin/env python3
import argparse
import os
import select
import time

parser = argparse.ArgumentParser()
parser.add_argument("--resource", choices=["cpu", "memory", "io"], required=True)
parser.add_argument("--threshold-us", type=int, required=True)
parser.add_argument("--window-us", type=int, default=2_000_000)
parser.add_argument("--output", required=True)
args = parser.parse_args()

path = f"/proc/pressure/{args.resource}"
trigger = f"some {args.threshold_us} {args.window_us}\0".encode()

with open(args.output, "w", buffering=1) as out:
    out.write(
        f"psi_monitor_start resource={args.resource} wall_ns={time.time_ns()} "
        f"threshold_us={args.threshold_us} window_us={args.window_us}\n"
    )
    try:
        fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
        os.write(fd, trigger)
    except OSError as exc:
        out.write(
            f"psi_monitor_unavailable resource={args.resource} wall_ns={time.time_ns()} "
            f"errno={getattr(exc, 'errno', None)} error={exc}\n"
        )
        raise SystemExit(0)

    poller = select.poll()
    poller.register(fd, select.POLLPRI | select.POLLERR)

    try:
        while True:
            for _, event in poller.poll(1000):
                now = time.time_ns()
                if event & select.POLLERR:
                    out.write(
                        f"psi_monitor_error resource={args.resource} wall_ns={now} event={event}\n"
                    )
                    raise SystemExit(0)
                if event & select.POLLPRI:
                    out.write(
                        f"psi_event resource={args.resource} wall_ns={now} event={event}\n"
                    )
    finally:
        os.close(fd)
