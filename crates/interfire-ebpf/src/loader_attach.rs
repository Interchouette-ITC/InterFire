//! Live eBPF attach path (ignored by `make coverage` upload filters).

use aya::Ebpf;
use aya::programs::KProbe;

use crate::loader::{EVENT_MAP, LoadError, Observer, PROGRAM_NAME};

/// Attach a loaded object to `tcp_v4_connect`.
///
/// # Errors
///
/// Returns [`LoadError`] when symbols are missing or attach fails.
pub fn attach_loaded(mut bpf: Ebpf) -> Result<Observer, LoadError> {
    let _ = bpf
        .map(EVENT_MAP)
        .ok_or(LoadError::MissingSymbol(EVENT_MAP))?;
    let program: &mut KProbe = bpf
        .program_mut(PROGRAM_NAME)
        .ok_or(LoadError::MissingSymbol(PROGRAM_NAME))?
        .try_into()?;
    program.load()?;
    program.attach("tcp_v4_connect", 0)?;
    Ok(Observer { bpf: Some(bpf) })
}
