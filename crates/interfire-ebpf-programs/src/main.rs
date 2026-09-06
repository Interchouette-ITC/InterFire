#![no_std]
#![no_main]

use aya_ebpf::{macros::kprobe, programs::ProbeContext};

/// Attachment smoke hook. The next change replaces this with the bounded TCP
/// tuple event written to a ring buffer. It intentionally has no enforcement
/// side effect: NFQUEUE remains the only verdict path.
#[kprobe]
pub fn interfire_tcp_connect(context: ProbeContext) -> u32 {
    let _ = context;
    0
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
