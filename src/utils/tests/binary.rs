use super::*;
use crate::error::TreeLimit;

fn binary_sample(magic: &[u8]) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend((0..16_384u32).map(|i| ((i * 7919 + 13) % 251) as u8));
    out
}

// gzip header with the FNAME flag and the file name `name`
fn gzip_named_sample(name: &[u8]) -> Vec<u8> {
    let mut magic = vec![0x1f, 0x8b, 0x08, 0x08, 1, 2, 3, 4, 0x00, 0x03];
    magic.extend(name);
    magic.push(0);
    binary_sample(&magic)
}

// zip local file header whose name field is `name`
fn zip_named_sample(name: &[u8]) -> Vec<u8> {
    let mut magic = b"PK\x03\x04\x14\x00".to_vec();
    magic.extend([0; 20]);
    magic.extend([10, 0, 0, 0]);
    magic.extend(name);
    binary_sample(&magic)
}

const BINARY_MAGICS: [&[u8]; 13] = [
    b"%PDF-",
    b"PK\x03\x04",
    b"%PDF-1.4\n",
    b"%PDF-1.7\n",
    b"\x89PNG\r\n\x1a\n",
    b"\xff\xd8\xff\xdb",
    b"\xff\xd8\xff\xe0",
    b"\xff\xd8\xff\xe1",
    b"GIF87a",
    b"GIF89a",
    b"RIFF\x00\x00\x00\x00WEBPVP8 ",
    b"\x1f\x8b\x08",
    b"\x28\xb5\x2f\xfd",
];

fn no_parse() -> ParseLimits {
    // max_nodes 0 makes any parse fail with TreeTooLarge, so EmptyTree proves no parse ran
    ParseLimits {
        max_nodes: Some(0),
        ..ParseLimits::default()
    }
}

#[test]
fn binary_input_is_rejected_before_parsing() {
    // deviation: D-12
    for magic in BINARY_MAGICS {
        assert!(
            matches!(
                load_html(
                    HtmlInput::Bytes(&binary_sample(magic)),
                    SIZE,
                    &no_parse(),
                    &Call::default()
                ),
                Err(ExtractError::Rejected(Rejection::EmptyTree))
            ),
            "{magic:?}"
        );
    }
}

#[test]
fn binary_samples_give_upstream_none() {
    // oracle: upstream load_html returns None for each of these samples (scripts/oracle.sh)
    for magic in BINARY_MAGICS {
        assert!(
            matches!(
                load_bytes(&binary_sample(magic)),
                Err(ExtractError::Rejected(Rejection::EmptyTree))
            ),
            "{magic:?}"
        );
    }
}

#[test]
fn binary_with_an_html_name_is_parsed() {
    // oracle: a header naming `index.html` puts `html` in upstream's first 50 characters (it
    // lowercases them), so load_html returns a tree for each sample, `INDEX.HTML` included
    for name in [&b"index.html"[..], b"INDEX.HTML"] {
        for sample in [gzip_named_sample(name), zip_named_sample(name)] {
            assert!(matches!(
                load_html(
                    HtmlInput::Bytes(&sample),
                    SIZE,
                    &no_parse(),
                    &Call::default()
                ),
                Err(ExtractError::Rejected(Rejection::TreeTooLarge(
                    TreeLimit::MaxNodes
                )))
            ));
        }
    }
}

#[test]
fn text_with_a_binary_magic_prefix_is_rejected() {
    // oracle: upstream load_html returns a tree of 2 children for this input, which has no `html`
    // in its first 50 characters; the shortcut rejects it
    assert!(matches!(
        load_bytes(b"GIF89a<p>a</p><p>b</p>"),
        Err(ExtractError::Rejected(Rejection::EmptyTree))
    ));
}

#[test]
fn text_that_looks_binary_is_parsed() {
    let call = Call::default();
    // Str input never takes the shortcut
    assert!(matches!(
        load_html(
            HtmlInput::Str("%PDF-1.4\n<html><body><p>a</p></body></html>"),
            SIZE,
            &no_parse(),
            &call
        ),
        Err(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::MaxNodes
        )))
    ));
    // a BOM page and leading whitespace are HTML
    for page in [
        &b"\xef\xbb\xbf<html><body><p>a</p></body></html>"[..],
        b"  \n<html><body><p>a</p></body></html>",
    ] {
        assert!(matches!(
            load_html(HtmlInput::Bytes(page), SIZE, &no_parse(), &call),
            Err(ExtractError::Rejected(Rejection::TreeTooLarge(
                TreeLimit::MaxNodes
            )))
        ));
    }
}
