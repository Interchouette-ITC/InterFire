//! eBPF event contract placeholder.
//!
//! Program loading is intentionally deferred to the enforcement slice. Keeping
//! this crate dependency-free makes the workspace build on developer machines
//! before Aya and the BPF target toolchain are introduced.
#![forbid(unsafe_code)]

/// Bounded wire payload emitted by the future TCP-connect program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct TcpConnectEvent {
    pub pid: u32,
    pub process_start_ticks: u64,
    pub destination_ipv4: u32,
    pub destination_port: u16,
    pub reserved: u16,
}

const _: () = assert!(core::mem::size_of::<TcpConnectEvent>() == 24);
