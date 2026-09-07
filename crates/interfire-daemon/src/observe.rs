//! Ring-buffer consumption and policy decisions.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use interfire_ebpf::{Observer, TcpConnectEvent};
use tracing::{debug, warn};

use crate::policy;
use crate::shared::Shared;

/// Poll the observer ring buffer until the process exits.
pub fn run(mut observer: Observer, shared: &Arc<Shared>) {
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
            handle_event(event, shared);
        }
        if !progressed {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn handle_event(event: TcpConnectEvent, shared: &Shared) {
    let Ok(rules) = shared.rules.lock() else {
        return;
    };
    let Ok(mut cache) = shared.process_cache.lock() else {
        return;
    };
    let Ok(mut prompts) = shared.prompts.lock() else {
        return;
    };
    let Ok(mut dns) = shared.dns.lock() else {
        return;
    };
    let decision = policy::decide(event, &rules, &mut cache, &mut prompts, &mut dns);
    drop(dns);
    drop(prompts);
    drop(cache);
    drop(rules);
    if let Ok(mut pending) = shared.pending.lock() {
        pending.insert(decision.key, decision.packet_verdict);
        debug!(
            attributed = decision.attributed,
            port = decision.key.port,
            prompt_id = ?decision.prompt_id,
            "pending verdict stored"
        );
    }
    if let Ok(mut audit) = shared.audit.lock() {
        let outcome = if decision.packet_verdict == nfq::Verdict::Accept {
            "allow"
        } else {
            "deny"
        };
        audit.append(format!(
            "connect port={} attributed={} outcome={outcome}",
            decision.key.port, decision.attributed
        ));
    }
}
