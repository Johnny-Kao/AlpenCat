# Native Hardware Validation

> Goal: validate one real hardware / OS transition path without requiring a partner to integrate AlpenCat first.

## Why this exists

AlpenCat's control policy and hot-path economics have already been tested independently.

The remaining architecture question is whether a real platform transition can be delivered to a tiny userspace adapter with usable semantics and latency.

The first target is Intel Hardware Feedback Interface (HFI).

Linux can expose HFI CPU performance / efficiency capability changes through the thermal Generic Netlink event family when the relevant kernel support is enabled.

## Preferred first platform

A physical Intel Xeon server with:

- HFI-capable Intel CPU;
- Linux;
- `CONFIG_INTEL_HFI_THERMAL=y`;
- `CONFIG_THERMAL_NETLINK=y`;
- access to a reproducible platform performance / power transition if available;
- preferably Intel Speed Select / platform profile control.

Sapphire Rapids or later Xeon platforms are the preferred starting point.

A virtual machine is not sufficient unless HFI is explicitly passed through to the guest.

## One-command partner workflow

Clone the repository and run:

```bash
sudo ./tools/native-validation/alpencat-native-probe.sh
```

Optional listener duration:

```bash
sudo ./tools/native-validation/alpencat-native-probe.sh --listen-seconds 300
```

The script does **not** modify CPU power limits, BIOS settings, SST profiles, or thermal controls.

It only:

1. records system / CPU metadata;
2. checks the Intel HFI CPUID capability bit;
3. checks relevant kernel configuration where accessible;
4. compiles the minimal Generic Netlink listener;
5. subscribes to the Linux thermal `event` multicast group;
6. records real `THERMAL_GENL_EVENT_CPU_CAPABILITY_CHANGE` messages;
7. packages the evidence into a tarball.

## During the listening window

A platform engineer may independently trigger a known, approved capability transition using normal vendor tooling.

Preferred examples:

- Intel Speed Select profile transition;
- BMC / BIOS-backed performance profile transition;
- another controlled platform action already used by the vendor to change CPU capability.

The AlpenCat probe itself intentionally does not perform these actions.

This keeps the validation safe, auditable, and vendor-controlled.

## Expected output

The script creates a directory and a compressed evidence bundle containing:

```text
summary.txt
system.txt
cpuid_hfi.txt
kernel_config.txt
dmesg_hfi.txt
thermal_events.jsonl
listener.stderr
```

A successful event looks conceptually like:

```json
{
  "timestamp": "2026-10-04T12:34:56.123456789Z",
  "event_cmd": 13,
  "cpu_capabilities": [
    {"cpu": 17, "performance": 812, "efficiency": 921}
  ]
}
```

The exact capability values are platform-defined normalized values exposed by the Linux thermal interface.

## Validation levels

| Level | Requirement |
| --- | --- |
| L1 | CPU + kernel environment supports the required HFI / thermal path |
| L2 | Userspace receives a real CPU capability-change event |
| L3 | The event updates an AlpenCat ResourceEpoch / stale state |
| L4 | A real workload demonstrates reduced stale-boundary regret end-to-end |

The external probe in this repository is designed to establish L1 and L2.

L3 and L4 are AlpenCat integration tests and remain under project control.

## Data requested from validation partners

The generated evidence bundle is sufficient.

No proprietary BIOS configuration, firmware source, BMC credentials, or internal tooling is required.

If a partner is willing to provide additional context, the most useful metadata are:

- server model;
- CPU SKU;
- BIOS / firmware version;
- Linux distribution and kernel;
- action used to produce the capability transition;
- whether the machine is bare metal or virtualized.

## Safety / non-invasiveness

The probe:

- does not write MSRs;
- does not alter CPU frequency / power settings;
- does not change BIOS or BMC state;
- does not stress the system;
- does not install a kernel module;
- does not require a custom kernel.

It is a passive evidence collector.

## Research question

A successful L2 result answers:

> Can a real platform capability transition reach a tiny userspace observer through an existing native interface, without continuous polling?

That is the final native-signal prerequisite before connecting the event to AlpenCat's already-tested ResourceEpoch mechanism.
