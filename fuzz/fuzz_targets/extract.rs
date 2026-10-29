// Checks that extracting from arbitrary bytes (any encoding, possibly compressed) never panics, times
// out or exhausts memory; an `Err` is a valid outcome.
#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use trafilatura_rust::{Call, Extractor, Format, Input, Options};

const OPTION_SETS: usize = 6;

/// Index `i`: format `[Txt, Markdown, Html][i / 2]`; an odd `i` turns links, images and formatting on,
/// an even one leaves them at the defaults of `Options::new()`.
static EXTRACTORS: LazyLock<[Extractor; OPTION_SETS]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let options = Options::new().format([Format::Txt, Format::Markdown, Format::Html][i / 2]);
        if i % 2 == 1 {
            options.links(true).images(true).formatting(true).build()
        } else {
            options.build()
        }
    })
});

fuzz_target!(|data: &[u8]| {
    // The length picks the options, so no input byte is consumed and seeds keep their magic bytes.
    let extractor = &EXTRACTORS[data.len() % OPTION_SETS];
    let _ = extractor.extract(&Input::bytes(data), &Call::default());
});
