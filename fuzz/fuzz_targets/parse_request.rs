#![no_main]

use interfire_proto::Request;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(frame) = std::str::from_utf8(data) else {
        return;
    };
    let _ = Request::parse(frame);
});
