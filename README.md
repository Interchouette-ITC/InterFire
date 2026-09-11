# Rust API documentation (rustdoc)

Generate with `make doc`, then open [`index.html`](index.html).

Workspace crates include `interfire-rules`, `interfire-proto`, `interfire-daemon`
(`interfired`), `interfirectl`, `interfire-tui`, and `interfire-ebpf` (loader). The BPF program
crate is a separate Cargo workspace under `crates/interfire-ebpf-programs/` (built with `make ebpf`,
not rustdoc).
