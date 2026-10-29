// Checks that parsing arbitrary text never panics, times out or exhausts memory; an `Err` is a valid outcome.
#![no_main]

use libfuzzer_sys::fuzz_target;
use trafilatura_rust::__internals::{ParseLimits, parse_document};
use trafilatura_rust::Call;

fuzz_target!(|html: &str| {
    let _ = parse_document(html, &ParseLimits::default(), &Call::default());
});
