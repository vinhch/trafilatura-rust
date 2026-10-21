# Deviations from upstream

Every place this crate departs from upstream Python trafilatura at the pinned release, deliberate or
forced. The code carries `// deviation: D-n` at the spot. A row leaves when the difference is gone,
for example after upstream changed; ids are never reused.

| Id | Upstream behavior | This crate | Reason | Since |
| --- | --- | --- | --- | --- |
| D-1 | `fast=False` by default: readability and jusText run after the main extractor (`external.py`) and can replace its result | Fast mode only: no `fast` option, no readability or jusText, every `options.fast` branch takes the fast side | Keeps the first releases thin; a caller that needs a fallback runs one on a rejection. Porting `fast=False` is followup F-6 | first release |
| D-2 | `lxml.html` parses with libxml2: HTML5 tokenizer, libxml2's own tree construction (no implied `tbody`, no adoption agency) | html5ever: HTML5 tokenizer and tree construction, into the crate's arena tree | No maintained pure-Rust libxml2 tree builder; FFI to libxml2 brings C, `unsafe` and system-dependent versions. A libxml2-style builder is followup F-7 | first release |
| D-3 | Bytes that are not valid UTF-8 are decoded with `charset_normalizer`'s guesses (`utils.py::detect_encoding`) | `chardetng` guess, decoded with `encoding_rs`; the strict UTF-8 first pass and the replacement fallback are as upstream | No maintained Rust port of charset_normalizer; porting it is a project of its own. A parity detector is followup F-8 | first release |
| D-4 | Compressed input (gzip, zstd, brotli, zlib) is decompressed (`utils.py::handle_compressed_file`) | Only with the `decompress` feature; the default build parses compressed bytes as they are | HTTP clients decompress before the crate sees the body; keeps three crates out of the default build | first release |
