# Threat model

## Assets

- The host's outbound traffic policy and its persistent rules.
- Process identity data, command lines, audit records, and IPC credentials.
- The integrity of the privileged daemon, eBPF programs, and owned nftables state.

## Trust boundaries

- The unprivileged UI and CLI are separate from the privileged daemon.
- Unix-socket peer credentials and socket filesystem permissions authorize policy
  mutation.
- Kernel event data identifies a PID, but `/proc` enrichment is raced and must
  include a process start-time identity to defend against PID reuse.
- DNS supplies display and rule metadata; the actual address and port remain
  authoritative for an observed connection.

## Principal threats and controls

| Threat                               | Control                                                                                              |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| Unprivileged local policy changes    | peer-credential authorization; root-owned state and sockets                                          |
| PID reuse / short-lived process race | enrich immediately; compare start time; mark unresolved events unattributed and never silently allow |
| UI crash or OOM                      | daemon-owned bounded queues; prompt timeout deny; reconnect-safe subscriptions                       |
| Malformed IPC or rule file           | versioned parsing, input limits, atomic write, validation before replacement                         |
| Resource exhaustion                  | fixed queue/cache/log caps plus drop/expiry metrics                                                  |
| Firewall interference                | operate only an `interfire` nftables table; never rewrite unrelated tables                           |
| Kernel compatibility failure         | explicit degraded diagnosis; no claim of protection without a functioning verdict path               |

## What is and is not claimed today

NFQUEUE is the primary verdict path (see [`architecture.md`](architecture.md)).
The daemon can bind queue **4242** and apply allow/deny when the operator
installs the InterFire-owned nftables rule and capabilities are present. Do not
represent enforcement as production-reliable until latency, daemon-loss, and
coexistence measurements pass on supported kernels.
