# Rust API documentation (rustdoc)

Generate with `make doc`, then open [`index.html`](index.html).

Workspace crates include `interfire-rules`, `interfire-proto`, `interfire-daemon`
(`interfired`), `interfirectl`, `interfire-tui`, and `interfire-ebpf` (loader).
`make doc` excludes `interfire-ui` (needs GPUI system libs; build with `make ui`)
and the BPF program crate (built with `make ebpf`, not rustdoc).
