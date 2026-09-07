//! TCP-connect observation contract and userspace loader.
//!
//! The eBPF program emits bounded [`TcpConnectEvent`] records. Attribution and
//! verdicts happen in userspace (`/proc`, rules, NFQUEUE).
#![cfg_attr(not(feature = "loader"), no_std)]
#![cfg_attr(feature = "loader", forbid(unsafe_code))]

#[cfg(feature = "loader")]
mod loader;

#[cfg(feature = "loader")]
pub use loader::{EVENT_MAP, LoadError, Observer, ObserverStatus, PROGRAM_NAME};

/// Ring buffer capacity in bytes (power-of-two page multiple; 256 KiB).
pub const RINGBUF_BYTE_SIZE: u32 = 256 * 1024;

/// Bounded wire payload emitted by the TCP-connect program.
///
/// `pid` is the thread-group id (TGID) suitable for `/proc/<pid>`.
/// `process_start_ticks` is always zero from the kernel program; userspace fills
/// start ticks from `/proc` immediately after dequeue.
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
const _: () = assert!(core::mem::align_of::<TcpConnectEvent>() <= 8);

impl TcpConnectEvent {
    /// Destination as an IPv4 address in host byte order octets.
    #[must_use]
    pub const fn destination_octets(self) -> [u8; 4] {
        self.destination_ipv4.to_ne_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_layout_is_stable() {
        assert_eq!(core::mem::size_of::<TcpConnectEvent>(), 24);
        assert!(core::mem::align_of::<TcpConnectEvent>() <= 8);
    }

    #[test]
    fn destination_octets_match_host_order_bytes() {
        let event = TcpConnectEvent {
            pid: 1,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: 443,
            reserved: 0,
        };
        assert_eq!(event.destination_octets(), [127, 0, 0, 1]);
    }
}
