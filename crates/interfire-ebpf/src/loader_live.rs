//! Live eBPF load + ring buffer open (ignored by `make coverage` upload filters).

use std::path::Path;

use aya::EbpfLoader;
use aya::maps::{MapData, RingBuf};

use crate::loader::{EVENT_MAP, LoadError, Observer};

impl Observer {
    /// Load bytecode from memory and attach to `tcp_v4_connect`.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the object is invalid, symbols are missing, or
    /// attach fails (missing caps/BTF/kprobe).
    pub fn load_and_attach(bytecode: &[u8]) -> Result<Self, LoadError> {
        let bpf = EbpfLoader::new().load(bytecode)?;
        crate::loader_attach::attach_loaded(bpf)
    }

    /// Load bytecode from a filesystem path and attach.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the file cannot be read or attach fails.
    pub fn load_path_and_attach(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let bpf = EbpfLoader::new().load_file(path.as_ref())?;
        crate::loader_attach::attach_loaded(bpf)
    }

    /// Borrow the event ring buffer map for polling.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the map is missing or the wrong type.
    pub fn ring_buf(&mut self) -> Result<RingBuf<&mut MapData>, LoadError> {
        let bpf = self
            .bpf
            .as_mut()
            .ok_or(LoadError::Bytecode(std::io::Error::other(
                "observer has no loaded eBPF object",
            )))?;
        let map = bpf
            .map_mut(EVENT_MAP)
            .ok_or(LoadError::MissingSymbol(EVENT_MAP))?;
        Ok(RingBuf::try_from(map)?)
    }
}
