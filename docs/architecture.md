# Architecture and feasibility record

## Current status: feasibility gate open

Interfire uses eBPF for connection and process lifecycle events, then resolves
the executable, command line, uid, cgroup, and start-time through `/proc` in
userspace. eBPF observation alone is not selected as the verdict path.

The primary verdict path is NFQUEUE, because a queued outbound packet can
receive an explicit userspace accept/drop verdict. The isolated loopback spike
passed on the Debian 6.12.107 kernel: one TCP connection was accepted and a
second was dropped by a userspace NFQUEUE listener. nftables remains the
coarse, Interfire-owned packet-filter mechanism. Production rollout still needs
latency, daemon-loss, and coexistence measurements.

## Event and identity flow

1. A bounded eBPF event supplies PID, network tuple, protocol, and timestamp.
2. The daemon immediately reads `/proc/<pid>/exe`, `cmdline`, `status`, cgroup,
   and start time, and compares it with its bounded process table.
3. The rules engine returns allow, deny, or prompt.
4. The chosen verdict backend acts on the connection. On expiry, absent UI, or
   an unattributed event, policy is deny and a diagnostic is recorded.

## Failure behavior

No UI connection must block the daemon indefinitely. A full prompt queue,
backend error, incompatible kernel, or missing eBPF capability produces a
visible degraded state. Whether an unavailable verdict backend can safely
fail closed is part of the spike acceptance criteria.

## Comparison with the reference model

Like OpenSnitch, Interfire separates kernel observations from `/proc`-based
application attribution and handles process lifecycle events separately. Unlike
the reference implementation, Interfire will reimplement those components in
Rust and will not link or distribute its Go daemon or object files.

## Supported baseline

Initial target: Debian/Ubuntu, x86_64, systemd, a kernel with BTF and usable
eBPF features, and NFQUEUE support. The exact oldest kernel is deliberately
uncommitted until the capability probe and controlled verdict test pass there.

## Isolated NFQUEUE verdict command

After installing `libnetfilter-queue-dev`, run
`sudo scripts/phase0-nfqueue-spike.sh`. The test creates a temporary network
namespace, a loopback-only HTTP server, and an nftables table solely inside that
namespace. It verifies an NFQUEUE listener can accept one connection and drop
one connection, then removes the namespace and all its firewall state.
