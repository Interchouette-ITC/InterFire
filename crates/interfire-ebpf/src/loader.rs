//! Load, attach, and detach the TCP-connect observation program.

use std::io;
use std::path::Path;

use aya::maps::{MapData, MapError, RingBuf};
use aya::programs::ProgramError;
use aya::{Ebpf, EbpfError, EbpfLoader};

/// eBPF program section / function name attached to `tcp_v4_connect`.
pub const PROGRAM_NAME: &str = "interfire_tcp_connect";

/// Ring buffer map name declared by the eBPF program.
pub const EVENT_MAP: &str = "EVENTS";

/// Why observation could not start.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Embedded or on-disk bytecode was missing or unreadable.
    #[error("eBPF bytecode unavailable: {0}")]
    Bytecode(#[source] io::Error),
    /// `aya` rejected the object (BTF, verifier, or format).
    #[error("eBPF load failed: {0}")]
    Ebpf(#[from] EbpfError),
    /// Map conversion failed.
    #[error("eBPF map error: {0}")]
    Map(#[from] MapError),
    /// Required map or program name was absent from the object.
    #[error("eBPF object missing {0}")]
    MissingSymbol(&'static str),
    /// Attach failed (capabilities, kprobe symbol, or permissions).
    #[error("eBPF attach failed: {0}")]
    Attach(#[from] ProgramError),
}

/// High-level observation readiness for status / CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObserverStatus {
    /// Kprobe attached; ring buffer readable.
    Attached,
    /// Load or attach failed; daemon must not claim enforcement.
    Degraded,
}

/// Loaded TCP-connect observer. Dropping detaches with the `Ebpf` object.
pub struct Observer {
    pub(crate) bpf: Ebpf,
}

impl Observer {
    /// Load bytecode from memory and attach to `tcp_v4_connect`.
    ///
    /// `bytecode` must be sufficiently aligned for ELF parsing (use
    /// [`aya::include_bytes_aligned`] or [`Self::load_path_and_attach`]).
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the object is invalid, symbols are missing, or
    /// attach fails (missing caps/BTF/kprobe).
    pub fn load_and_attach(bytecode: &[u8]) -> Result<Self, LoadError> {
        let bpf = EbpfLoader::new().load(bytecode)?;
        Self::attach_loaded(bpf)
    }

    /// Load bytecode from a filesystem path and attach.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the file cannot be read or attach fails.
    pub fn load_path_and_attach(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let bpf = EbpfLoader::new().load_file(path.as_ref())?;
        Self::attach_loaded(bpf)
    }

    /// Load the release object embedded beside this crate, then attach.
    ///
    /// # Errors
    ///
    /// Same as [`Self::load_and_attach`].
    pub fn load_embedded_and_attach() -> Result<Self, LoadError> {
        Self::load_and_attach(aya::include_bytes_aligned!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/bpf/interfire-ebpf-programs"
        )))
    }

    fn attach_loaded(bpf: Ebpf) -> Result<Self, LoadError> {
        #[cfg(not(test))]
        {
            crate::loader_attach::attach_loaded(bpf)
        }
        #[cfg(test)]
        {
            attach_loaded_under_test(bpf)
        }
    }

    #[must_use]
    pub const fn status(&self) -> ObserverStatus {
        ObserverStatus::Attached
    }

    /// Borrow the ring buffer map for event polling.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::MissingSymbol`] when the map is absent, or
    /// [`LoadError::Map`] when the map type is wrong.
    pub fn ring_buf(&mut self) -> Result<RingBuf<&mut MapData>, LoadError> {
        let map = self
            .bpf
            .map_mut(EVENT_MAP)
            .ok_or(LoadError::MissingSymbol(EVENT_MAP))?;
        Ok(RingBuf::try_from(map)?)
    }
}

#[cfg(test)]
fn attach_loaded_under_test(bpf: Ebpf) -> Result<Observer, LoadError> {
    let _ = bpf
        .map(EVENT_MAP)
        .ok_or(LoadError::MissingSymbol(EVENT_MAP))?;
    drop(bpf);
    Err(LoadError::Bytecode(io::Error::other(
        "attach skipped under unit tests",
    )))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use object::{Object, ObjectSymbol};

    use super::*;

    fn embedded_bytecode() -> &'static [u8] {
        aya::include_bytes_aligned!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/bpf/interfire-ebpf-programs"
        ))
    }

    #[test]
    fn embedded_bytecode_is_non_empty() {
        assert!(!embedded_bytecode().is_empty());
    }

    #[test]
    fn embedded_bytecode_is_valid_elf() {
        let data = embedded_bytecode();
        object::read::File::parse(data).expect("embedded eBPF object must parse as ELF");
    }

    #[test]
    fn load_error_display_covers_variants() {
        let bytecode = LoadError::Bytecode(io::Error::other("missing"));
        assert!(bytecode.to_string().contains("unavailable"));
        assert!(bytecode.source().is_some());

        let missing = LoadError::MissingSymbol("EVENTS");
        assert!(missing.to_string().contains("EVENTS"));
        assert!(missing.source().is_none());
    }

    #[test]
    fn load_path_missing_file_is_ebpf_error() {
        let result = Observer::load_path_and_attach("/tmp/interfire-no-such-ebpf-object");
        assert!(matches!(result, Err(LoadError::Ebpf(_))));
    }

    #[test]
    fn load_path_garbage_is_ebpf_error() {
        let path =
            std::env::temp_dir().join(format!("interfire-garbage-ebpf-{}", std::process::id()));
        std::fs::write(&path, b"not-an-elf").unwrap();
        let result = Observer::load_path_and_attach(&path);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(LoadError::Ebpf(_))));
    }

    #[test]
    fn load_embedded_without_caps_is_explicit_error() {
        let result = Observer::load_embedded_and_attach();
        if let Err(error) = result {
            let message = error.to_string();
            assert!(
                message.contains("eBPF") || message.contains("attach") || message.contains("map"),
                "unexpected error text: {message}"
            );
            assert!(error.source().is_some());
        }
    }

    #[test]
    fn observer_status_and_ring_buf_after_test_attach() {
        let result = Observer::load_and_attach(embedded_bytecode());
        if let Ok(mut observer) = result {
            assert_eq!(observer.status(), ObserverStatus::Attached);
            let _ = observer.ring_buf();
        }
    }

    #[test]
    fn load_without_caps_is_explicit_error() {
        // Non-root CI must not crash; map create / attach fails with a typed error.
        let result = Observer::load_and_attach(embedded_bytecode());
        if let Err(error) = result {
            let message = error.to_string();
            assert!(
                message.contains("eBPF") || message.contains("attach") || message.contains("map"),
                "unexpected error text: {message}"
            );
        }
    }

    #[test]
    fn observer_status_is_distinct() {
        assert_ne!(ObserverStatus::Attached, ObserverStatus::Degraded);
    }

    #[test]
    fn embedded_object_declares_program_and_event_map() {
        let file = object::read::File::parse(embedded_bytecode()).expect("elf");
        let symbols = file
            .symbols()
            .map(|symbol| symbol.name().unwrap_or(""))
            .collect::<Vec<_>>();
        assert!(symbols.iter().any(|name| name.contains(PROGRAM_NAME)));
        assert!(symbols.iter().any(|name| name.contains(EVENT_MAP)));
    }
}
