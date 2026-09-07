//! Minimal IPv4/TCP destination parsing for NFQUEUE payloads.
#![forbid(unsafe_code)]

use crate::pending::DestKey;

const IPPROTO_TCP: u8 = 6;

/// Parse destination IPv4 and TCP port from a raw IPv4 packet.
#[must_use]
pub fn tcp_destination(payload: &[u8]) -> Option<DestKey> {
    if payload.len() < 20 {
        return None;
    }
    let version_ihl = payload[0];
    if version_ihl >> 4 != 4 {
        return None;
    }
    let ihl = usize::from(version_ihl & 0x0f) * 4;
    if ihl < 20 || payload.len() < ihl + 4 {
        return None;
    }
    if payload[9] != IPPROTO_TCP {
        return None;
    }
    let ipv4 = u32::from_be_bytes(payload[16..20].try_into().ok()?);
    let port = u16::from_be_bytes(payload[ihl + 2..ihl + 4].try_into().ok()?);
    Some(DestKey { ipv4, port })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_tcp_syn_like_header() {
        let mut packet = vec![0_u8; 40];
        packet[0] = 0x45;
        packet[9] = IPPROTO_TCP;
        packet[16..20].copy_from_slice(&[127, 0, 0, 1]);
        packet[22..24].copy_from_slice(&443_u16.to_be_bytes());
        let key = tcp_destination(&packet).unwrap();
        assert_eq!(key.ipv4, u32::from_be_bytes([127, 0, 0, 1]));
        assert_eq!(key.port, 443);
    }

    #[test]
    fn rejects_non_tcp_and_short_buffers() {
        assert!(tcp_destination(&[]).is_none());
        let mut packet = vec![0_u8; 40];
        packet[0] = 0x45;
        packet[9] = 17;
        assert!(tcp_destination(&packet).is_none());
    }
}
