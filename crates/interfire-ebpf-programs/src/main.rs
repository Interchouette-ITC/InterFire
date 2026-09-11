#![no_std]
#![no_main]

//! BPF program for `tcp_v4_connect` observation (`bpfel-unknown-none` only).
//!
//! This crate is its own Cargo workspace (not a member of the userspace
//! workspace). Build with `make ebpf`.

use aya_ebpf::helpers::{bpf_get_current_pid_tgid, bpf_probe_read_user};
use aya_ebpf::macros::{kprobe, map};
use aya_ebpf::maps::RingBuf;
use aya_ebpf::programs::ProbeContext;
use interfire_ebpf::{RINGBUF_BYTE_SIZE, TcpConnectEvent};

const AF_INET: u16 = 2;

#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
}

#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(RINGBUF_BYTE_SIZE, 0);

/// Observe IPv4 `tcp_v4_connect`; emit a bounded event. No verdict side effect.
#[kprobe]
pub fn interfire_tcp_connect(context: ProbeContext) -> u32 {
    match try_tcp_connect(context) {
        Ok(value) => value,
        Err(value) => value,
    }
}

fn try_tcp_connect(context: ProbeContext) -> Result<u32, u32> {
    let addr: *const SockAddrIn = context.arg(1).ok_or(1u32)?;
    let sockaddr = unsafe { bpf_probe_read_user(addr).map_err(|_| 1u32)? };
    if sockaddr.sin_family != AF_INET {
        return Ok(0);
    }

    let pid_tgid = bpf_get_current_pid_tgid();
    let tgid = (pid_tgid >> 32) as u32;
    let event = TcpConnectEvent {
        pid: tgid,
        process_start_ticks: 0,
        destination_ipv4: u32::from_be(sockaddr.sin_addr),
        destination_port: u16::from_be(sockaddr.sin_port),
        reserved: 0,
    };

    if let Some(mut entry) = EVENTS.reserve::<TcpConnectEvent>(0) {
        entry.write(event);
        entry.submit(0);
    }
    Ok(0)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
