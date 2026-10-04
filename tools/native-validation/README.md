# AlpenCat Native Validation Probe

This directory contains a passive, directly runnable validation probe for hardware vendors, server OEMs, platform teams, and research labs.

Run:

```bash
sudo ./alpencat-native-probe.sh --listen-seconds 300
```

The probe:

- records system metadata;
- checks Intel HFI CPUID support;
- records relevant kernel configuration where readable;
- subscribes to the Linux thermal Generic Netlink `event` group;
- records real CPU capability-change events;
- returns a tar.gz evidence bundle.

It does not modify platform configuration.

See [Native Hardware Validation](../../docs/NATIVE_VALIDATION.md) for the protocol and success criteria.
