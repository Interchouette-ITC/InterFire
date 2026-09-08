//! Live NFQUEUE socket bind (ignored by `make coverage` upload filters).
#![forbid(unsafe_code)]

use nfq::{Queue, Verdict};
use tracing::{debug, info, warn};

use interfire_proto::NFQUEUE_NUM;

use crate::packet;
use crate::pending::DestKey;
use crate::shared::Shared;

/// Open queue [`NFQUEUE_NUM`] and apply pending / default-deny verdicts.
///
/// # Errors
///
/// Returns I/O errors when the queue cannot be opened or bound.
pub fn bind_and_serve(shared: &Shared) -> std::io::Result<()> {
    let mut queue = Queue::open()?;
    queue.bind(NFQUEUE_NUM)?;
    shared.set_enforcement("nfqueue");
    info!(queue = NFQUEUE_NUM, "NFQUEUE bound");
    loop {
        let mut message = queue.recv()?;
        let verdict = lookup_verdict(message.get_payload(), shared);
        message.set_verdict(verdict);
        queue.verdict(message)?;
    }
}

fn lookup_verdict(payload: &[u8], shared: &Shared) -> Verdict {
    let Some(key) = packet::tcp_destination(payload) else {
        warn!("NFQUEUE packet not parseable as IPv4 TCP; dropping");
        return Verdict::Drop;
    };
    for attempt in 0..50 {
        let Ok(mut pending) = shared.pending.lock() else {
            return Verdict::Drop;
        };
        if let Some(verdict) = pending.take(key) {
            debug!(
                port = key.port,
                attempt,
                ?verdict,
                "NFQUEUE matched pending decision"
            );
            return verdict;
        }
        drop(pending);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let DestKey { port, .. } = key;
    warn!(port, "NFQUEUE without pending decision; dropping");
    Verdict::Drop
}
