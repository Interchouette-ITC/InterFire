//! NFQUEUE bind and fail-closed verdict application.
#![forbid(unsafe_code)]

use std::sync::Arc;

use nfq::{Queue, Verdict};
use tracing::{debug, info, warn};

use crate::packet;
use crate::shared::Shared;

/// Default NFQUEUE number (matches the isolated spike).
pub const QUEUE_NUM: u16 = 4242;

/// Bind queue `QUEUE_NUM` and apply pending / default-deny verdicts.
///
/// # Errors
///
/// Returns I/O errors when the queue cannot be opened or bound.
pub fn run(shared: &Shared) -> std::io::Result<()> {
    let mut queue = Queue::open()?;
    queue.bind(QUEUE_NUM)?;
    shared.set_enforcement("nfqueue");
    info!(queue = QUEUE_NUM, "NFQUEUE bound");
    loop {
        let mut message = queue.recv()?;
        let verdict = lookup_verdict(&message, shared);
        message.set_verdict(verdict);
        queue.verdict(message)?;
    }
}

fn lookup_verdict(message: &nfq::Message, shared: &Shared) -> Verdict {
    let Some(key) = packet::tcp_destination(message.get_payload()) else {
        warn!("NFQUEUE packet not parseable as IPv4 TCP; dropping");
        return Verdict::Drop;
    };
    let Ok(mut pending) = shared.pending.lock() else {
        return Verdict::Drop;
    };
    pending.take(key).map_or_else(
        || {
            warn!(
                port = key.port,
                "NFQUEUE without pending decision; dropping"
            );
            Verdict::Drop
        },
        |verdict| {
            debug!(
                port = key.port,
                ?verdict,
                "NFQUEUE matched pending decision"
            );
            verdict
        },
    )
}

/// Attempt to bind; on failure mark enforcement degraded and return.
pub fn run_or_degrade(shared: &Arc<Shared>) {
    if let Err(error) = run(shared) {
        shared.set_enforcement("degraded");
        warn!(%error, "NFQUEUE unavailable; enforcement=degraded");
    }
}
