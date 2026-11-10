use std::io::Write;

use super::*;

const HTML: &str = "<html><head/><body><div>ABC</div></body></html>";

// brotli.compress(b"<html>a</html>")
const BROTLI_HTML: &[u8] = &[
    0x8b, 0x06, 0x80, 0x3c, 0x68, 0x74, 0x6d, 0x6c, 0x3e, 0x61, 0x3c, 0x2f, 0x68, 0x74, 0x6d, 0x6c,
    0x3e, 0x03,
];
// brotli.compress(b"0" * 100_000)
const BROTLI_ZEROS: &[u8] = &[
    0x5b, 0x9f, 0x86, 0x81, 0x5f, 0x02, 0x26, 0x1e, 0x0b, 0x04, 0xb2, 0xfc, 0x02, 0x00,
];

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn input_cap_reads_the_raw_length_not_the_decompressed_one() {
    // gzip of a page larger than the cap, itself smaller than the cap
    let page = format!("<html><body>{}</body></html>", "<p>word</p>".repeat(2_000));
    let compressed = gzip(page.as_bytes());
    assert!(compressed.len() < page.len());
    let limits = capped(compressed.len());
    assert!(
        load_html(
            HtmlInput::Bytes(&compressed),
            SIZE,
            &limits,
            &Call::default()
        )
        .is_ok()
    );
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

#[test]
fn test_decode() {
    assert!(!decode_file(b" ", SIZE).is_empty());
    for compressed in [
        gzip(HTML.as_bytes()),
        zlib(HTML.as_bytes()),
        zstd(HTML.as_bytes()),
    ] {
        assert_eq!(
            handle_compressed_file(&compressed, SIZE).as_ref(),
            HTML.as_bytes()
        );
        assert_eq!(decode_file(&compressed, SIZE), HTML);
    }
    assert_eq!(
        handle_compressed_file(BROTLI_HTML, SIZE).as_ref(),
        b"<html>a</html>"
    );
}

#[test]
fn compressed_html_is_not_binary() {
    // deviation: D-12 (the gzip magic is checked after decompression)
    let page = "<html><body><p>one</p><p>two</p></body></html>";
    let tree = load_bytes(&gzip(page.as_bytes())).unwrap();
    assert_eq!(tree.iter(tree.root(), &["p"]).count(), 2);
}

#[test]
fn decompress_gzip_and_cap() {
    let bomb = gzip(&vec![b'0'; 25_000_000]);
    assert_eq!(
        handle_compressed_file(&bomb, SIZE).as_ref(),
        bomb.as_slice()
    );
    assert_eq!(handle_compressed_file(&bomb, 30_000_000).len(), 25_000_000);
    // Exactly at the cap is accepted, one over is not.
    let data = gzip(&[b'a'; 100]);
    assert_eq!(handle_compressed_file(&data, 100).len(), 100);
    assert_eq!(handle_compressed_file(&data, 99).as_ref(), data.as_slice());
}

#[test]
fn decompress_keeps_corrupt_input() {
    let whole = zstd(&HTML.as_bytes().repeat(500));
    let half = whole.get(..whole.len() / 2).unwrap();
    let small = zstd(HTML.as_bytes());
    let cut = small.get(..small.len() - 4).unwrap();
    for bad in [
        "äöüß".as_bytes(),
        b"\x1f\x8b\x08abc",
        b"\x28\xb5\x2f\xfdabc",
        half,
        cut,
    ] {
        assert_eq!(handle_compressed_file(bad, SIZE).as_ref(), bad);
    }
}

#[test]
fn decompress_multi_member_and_padding() {
    let multi = [gzip(b"<html>part1 "), gzip(b"part2</html>")].concat();
    assert_eq!(
        handle_compressed_file(&multi, SIZE).as_ref(),
        b"<html>part1 part2</html>"
    );
    let padded = [gzip(b"<html>a "), vec![0; 8], gzip(b"b</html>"), vec![0; 8]].concat();
    assert_eq!(
        handle_compressed_file(&padded, SIZE).as_ref(),
        b"<html>a b</html>"
    );
    let frames = [zstd(b"<html>a "), zstd(b"b</html>")].concat();
    assert_eq!(
        handle_compressed_file(&frames, SIZE).as_ref(),
        b"<html>a b</html>"
    );
}

#[test]
fn decompress_member_flood() {
    let member = gzip(b"a");
    let flood = member.repeat(MAX_MEMBERS + 1);
    assert_eq!(
        handle_compressed_file(&flood, SIZE).as_ref(),
        flood.as_slice()
    );
    let limit = member.repeat(MAX_MEMBERS);
    assert_eq!(
        handle_compressed_file(&limit, SIZE).as_ref(),
        vec![b'a'; MAX_MEMBERS]
    );
}

#[test]
fn decompress_zstd_and_brotli_bombs() {
    let bomb = zstd(&vec![b'0'; 25_000_000]);
    assert_eq!(
        handle_compressed_file(&bomb, SIZE).as_ref(),
        bomb.as_slice()
    );
    assert_eq!(
        handle_compressed_file(BROTLI_ZEROS, 1000).as_ref(),
        BROTLI_ZEROS
    );
    assert_eq!(handle_compressed_file(BROTLI_ZEROS, SIZE).len(), 100_000);
}

fn load_at(bytes: &[u8], max_size: usize) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        max_size,
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn too_large() -> ExtractError {
    ExtractError::Rejected(Rejection::InputTooLarge(InputLimit::MaxFileSize))
}

#[test]
fn oversized_gzip_is_too_large() {
    let compressed = gzip(&[b'0'; 2000]);
    assert_eq!(load_at(&compressed, 1000).map(drop), Err(too_large()));
}

#[test]
fn oversized_zstd_is_too_large() {
    let compressed = zstd(&[b'0'; 2000]);
    assert_eq!(load_at(&compressed, 1000).map(drop), Err(too_large()));
}

#[test]
fn gzip_at_cap_is_accepted() {
    let at_cap = format!("<html><body><p>{}</p></body></html>", "a".repeat(967));
    assert_eq!(at_cap.len(), 1000);
    assert!(load_at(&gzip(at_cap.as_bytes()), 1000).is_ok());
    let over = format!("<html><body><p>{}</p></body></html>", "a".repeat(968));
    assert_eq!(over.len(), 1001);
    assert_eq!(
        load_at(&gzip(over.as_bytes()), 1000).map(drop),
        Err(too_large())
    );
}

#[test]
fn oversized_gzip_members_add_up() {
    // Each member is under the cap; the output of both is over it.
    let member = gzip(&[b'0'; 600]);
    let two = [member.clone(), member].concat();
    assert_eq!(load_at(&two, 1000).map(drop), Err(too_large()));
}

#[test]
fn corrupt_gzip_still_empty_tree() {
    let whole = gzip(HTML.as_bytes());
    let cut = whole.get(..whole.len() - 8).unwrap();
    assert_eq!(
        load_at(cut, SIZE).map(drop),
        Err(ExtractError::Rejected(Rejection::EmptyTree))
    );
}

#[test]
fn oversized_brotli_and_zlib_keep_upstream_path() {
    // Values of 0.1.0 on the parent commit: both give EmptyTree at this cap.
    assert_eq!(
        load_at(BROTLI_ZEROS, 1000).map(drop),
        Err(ExtractError::Rejected(Rejection::EmptyTree))
    );
    assert_eq!(
        load_at(&zlib(&[b'0'; 2000]), 1000).map(drop),
        Err(ExtractError::Rejected(Rejection::EmptyTree))
    );
}

#[test]
fn brotli_like_text_under_small_cap_is_unchanged() {
    // A valid brotli stream whose uncompressed meta-block decodes to a 1,244-byte page: past the
    // cap of 1000, so brotli fails there and upstream keeps the raw bytes. Cap 0 decodes nothing,
    // so it gives the raw-bytes result that 0.1.0 gives at cap 1000.
    let mut page = b"Quick read <html><body><p>".to_vec();
    page.extend(std::iter::repeat_n(b'x', 1200));
    page.extend_from_slice(b"</p></body></html>");
    let stream = brotli_uncompressed(&page);
    assert_eq!(bounded_unbrotli(&stream, 1000), None);
    assert_eq!(bounded_unbrotli(&stream, SIZE), Some(page));
    let small = load_at(&stream, 1000).map(|tree| tree.tostring(tree.root(), false, false));
    let raw = load_at(&stream, 0).map(|tree| tree.tostring(tree.root(), false, false));
    assert!(small.is_ok());
    assert_eq!(small, raw);
}

/// A brotli stream of one uncompressed meta-block holding `literal`, then the empty last
/// meta-block (RFC 7932, section 9.2). WBITS is 16, so `literal` is at most 65,536 bytes.
fn brotli_uncompressed(literal: &[u8]) -> Vec<u8> {
    let mut bits = vec![false]; // WBITS: 16
    bits.push(false); // ISLAST
    bits.extend([false, false]); // MNIBBLES: 4 nibbles
    push_bits(&mut bits, literal.len() - 1, 16); // MLEN - 1
    bits.push(true); // ISUNCOMPRESSED
    bits.resize(bits.len().next_multiple_of(8), false);
    let mut out = pack_bits(&bits);
    out.extend_from_slice(literal);
    out.push(0b11); // ISLAST, ISLASTEMPTY, then padding
    out
}

/// Appends the low `width` bits of `value`, least significant first.
fn push_bits(bits: &mut Vec<bool>, value: usize, width: usize) {
    bits.extend((0..width).map(|at| (value >> at) & 1 == 1));
}

/// Packs bits into bytes, least significant bit first.
fn pack_bits(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0_u8, |byte, (at, &bit)| byte | (u8::from(bit) << at))
        })
        .collect()
}

#[test]
fn decode_file_keeps_raw_bytes_when_oversized() {
    let compressed = gzip(&[b'0'; 2000]);
    let decoded = decode_file(&compressed, 1000);
    assert_eq!(decoded, decode_bytes(Cow::Borrowed(&compressed)));
    assert!(!decoded.is_empty());
}

#[test]
fn html2txt_oversized_gzip_is_too_large() {
    let compressed = gzip(&vec![0u8; MAX_FILE_SIZE + 1]);
    let input = crate::api::Input::bytes(&compressed);
    assert_eq!(
        crate::api::html2txt(&input, false, &Call::default()),
        Err(too_large())
    );
}
