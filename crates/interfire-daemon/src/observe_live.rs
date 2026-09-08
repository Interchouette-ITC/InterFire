//! Live eBPF ring polling (ignored by `make coverage` upload filters).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use interfire_ebpf::{Observer, TcpConnectEvent};
use tracing::warn;

use crate::shared::Shared;

/// Poll the observer ring buffer until the process exits.
pub fn poll_ring(mut observer: Observer, shared: &Arc<Shared>) {
    let mut ring = match observer.ring_buf() {
        Ok(ring) => ring,
        Err(error) => {
            warn!(%error, "ring buffer unavailable; observation thread exiting");
            return;
        }
    };
    loop {
        let mut progressed = false;
        while let Some(item) = ring.next() {
            progressed = true;
            let Some(event) = TcpConnectEvent::try_from_bytes(item.as_ref()) else {
                warn!(len = item.len(), "dropping malformed ringbuf record");
                continue;
            };
            crate::observe::handle_event_for_live(event, shared);
        }
        if !progressed {
            thread::sleep(Duration::from_millis(1));
        }
    }
}
