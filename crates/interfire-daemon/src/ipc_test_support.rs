//! Test-only helpers for IPC coverage (fake streams without peer credentials).
#![allow(unsafe_code)]

use std::fs;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;

/// Unix stream handle where `SO_PEERCRED` is unavailable (mutate must fail closed).
pub fn stream_without_peer_creds() -> UnixStream {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .expect("open /dev/null");
    let fd = file.into_raw_fd();
    unsafe { UnixStream::from_raw_fd(fd) }
}
