//! TCP-connect observation contract and userspace loader.
//!
//! The eBPF program emits bounded [`TcpConnectEvent`] records. Attribution and
//! verdicts happen in userspace (`/proc`, rules, NFQUEUE).
#![cfg_attr(not(feature = "loader"), no_std)]
#![cfg_attr(feature = "loader", forbid(unsafe_code))]

#[cfg(feature = "loader")]
mod loader;
#[cfg(all(feature = "loader", not(test)))]
mod loader_attach;

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

    /// Parse a ring-buffer record into a [`TcpConnectEvent`].
    ///
    /// Expects the 24-byte `repr(C)` layout used by the eBPF program.
    #[must_use]
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 {
            return None;
        }
        let pid = u32::from_ne_bytes(bytes[0..4].try_into().ok()?);
        let process_start_ticks = u64::from_ne_bytes(bytes[8..16].try_into().ok()?);
        let destination_ipv4 = u32::from_ne_bytes(bytes[16..20].try_into().ok()?);
        let destination_port = u16::from_ne_bytes(bytes[20..22].try_into().ok()?);
        let reserved = u16::from_ne_bytes(bytes[22..24].try_into().ok()?);
        Some(Self {
            pid,
            process_start_ticks,
            destination_ipv4,
            destination_port,
            reserved,
        })
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

    #[test]
    fn try_from_bytes_reads_repr_c_layout_with_padding() {
        let mut bytes = [0_u8; 24];
        bytes[0..4].copy_from_slice(&42_u32.to_ne_bytes());
        bytes[8..16].copy_from_slice(&99_u64.to_ne_bytes());
        bytes[16..20].copy_from_slice(&u32::from_ne_bytes([10, 0, 0, 2]).to_ne_bytes());
        bytes[20..22].copy_from_slice(&8080_u16.to_ne_bytes());
        let event = TcpConnectEvent::try_from_bytes(&bytes).unwrap();
        assert_eq!(event.pid, 42);
        assert_eq!(event.process_start_ticks, 99);
        assert_eq!(event.destination_octets(), [10, 0, 0, 2]);
        assert_eq!(event.destination_port, 8080);
        assert!(TcpConnectEvent::try_from_bytes(&bytes[..23]).is_none());
    }
}
