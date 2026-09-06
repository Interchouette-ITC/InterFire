# OpenSnitch interaction study

OpenSnitch is a behavioral reference for interactive Linux application firewall
workflows. Interfire does not ship its code or eBPF object files.

## Keep

- Show the executable path and process details at prompt time.
- Let a user turn a decision into a narrowly scoped rule.
- Distinguish a one-time verdict, a temporary rule, and a persistent rule.
- Make daemon state and connection history inspectable without root UI access.

## Reject

- Retaining an unlimited event stream in the UI or daemon.
- Making enforcement depend on the graphical client remaining connected.
- Ambiguous application identity based on PID alone.
- Treating DNS labels as stronger evidence than the connection IP and port.

## Resource contract

The daemon has bounded prompt, event, DNS, process, and subscriber queues. The
UI keeps a fixed-size, virtualized log viewport and explicitly unsubscribes on
window close or reconnect. A killed UI cannot change the daemon's timeout-deny
policy.

