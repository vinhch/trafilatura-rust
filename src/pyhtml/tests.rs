use std::borrow::Cow;

use web_atoms::NAMED_ENTITIES;

use super::*;
use crate::error::{ExtractError, Rejection};

fn unescaped(s: &str) -> String {
    unescape(s).expect("valid references").into_owned()
}

#[test]
fn unescape_matches_python() {
    // oracle: html.unescape(s)
    for (s, expected) in [
        (
            "&ampx &notit; &notin &amp &AMP; &#128; &#0; &#xD800; &#1; &#x110000;",
            "&x \u{AC}it; \u{AC}in & & \u{20AC} \u{FFFD} \u{FFFD}  \u{FFFD}",
        ),
        ("&#99999999999999999999;|&#4294967361;", "\u{FFFD}|\u{FFFD}"),
        ("&#13;", "\r"),
        ("&#0;", "\u{FFFD}"),
        ("&#x81;", "\u{81}"),
        ("&#x9D;", "\u{9D}"),
        ("&#1;", ""),
        ("&#xB;", ""),
        ("&#xFFFF;", ""),
        ("&#x10FFFF;", ""),
        ("&# ", "&# "),
        ("&;", "&;"),
        ("a&", "a&"),
        ("&frac12;x", "\u{BD}x"),
        ("&frac12x", "\u{BD}x"),
        ("&ltx", "<x"),
        ("&gtx;", ">x;"),
        ("&l", "&l"),
        ("&lt", "<"),
        ("&acE;", "\u{223E}\u{333}"),
        ("&NotEqualTilde;", "\u{2242}\u{338}"),
        ("&acEx", "&acEx"),
        ("&#x1F;", ""),
        ("&#x7F;", ""),
        ("&#x7f", ""),
        ("&#xfdd0;", ""),
        ("&#xFDEF;", ""),
        ("&#x1FFFE;", ""),
        ("&#xD7FF;", "\u{D7FF}"),
        ("&#xE000;", "\u{E000}"),
        ("&#65", "A"),
        ("&#65x", "Ax"),
        ("&#x41g", "Ag"),
        ("&#X41;", "A"),
        ("&#xZ", "&#xZ"),
        ("&#", "&#"),
        ("&#x", "&#x"),
        ("&&amp;", "&&"),
        ("&amp;&amp", "&&"),
        ("&#x;", "&#x;"),
        ("&#;", "&#;"),
        ("&a#b;", "&a#b;"),
        ("&amp;amp;", "&amp;"),
        ("&\rfoo;", "&\rfoo;"),
        ("&\x0bamp;", "&\x0bamp;"),
        ("&ampamp", "&amp"),
        ("&notinx", "\u{AC}inx"),
        ("&notin;", "\u{2209}"),
        ("&lt;&amp&amp&amp", "<&&&"),
        ("x&y", "x&y"),
        ("&\u{E9};", "&\u{E9};"),
        ("&nbsp;&nbsp", "\u{A0}\u{A0}"),
        ("&Aacute;&Aacutex", "\u{C1}\u{C1}x"),
        ("&#x0;", "\u{FFFD}"),
        ("&#x0D;", "\r"),
        ("&#xD;", "\r"),
        ("&Aacutex", "\u{C1}x"),
        ("&notit;", "\u{AC}it;"),
        ("&ampxyzxyzxyz", "&xyzxyzxyz"),
        ("&am\u{E9}", "&am\u{E9}"),
    ] {
        assert_eq!(unescaped(s), expected, "{s:?}");
    }
}

#[test]
fn unescape_name_run_is_capped_at_32_code_points() {
    // oracle: html.unescape("&" + "x" * 40) == "&" + "x" * 40
    let long = format!("&{}", "x".repeat(40));
    assert_eq!(unescaped(&long), long);
    // oracle: html.unescape("&" + "a" * 32 + ";") == "&" + "a" * 32 + ";"
    let thirty_two = format!("&{};", "a".repeat(32));
    assert_eq!(unescaped(&thirty_two), thirty_two);
    let thirty_one = format!("&{};", "a".repeat(31));
    assert_eq!(unescaped(&thirty_one), thirty_one);
}

#[test]
fn unescape_digit_limits() {
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    // oracle: html.unescape("&#x" + "0" * 5000 + "41;")
    assert_eq!(unescaped(&format!("&#x{}41;", "0".repeat(5000))), "A");
    // oracle: html.unescape("&#" + "0" * 4298 + "65;"), and with 4299 zeros
    assert_eq!(unescaped(&format!("&#{}65;", "0".repeat(4298))), "A");
    assert_eq!(
        unescape(&format!("&#{}65;", "0".repeat(4299))).map(Cow::into_owned),
        invalid
    );
}

#[test]
fn unescape_borrows_without_ampersand() {
    assert!(matches!(
        unescape("plain text"),
        Ok(Cow::Borrowed("plain text"))
    ));
    assert!(matches!(unescape("a &amp; b"), Ok(Cow::Owned(_))));
}

#[test]
fn invalid_numeric_references_match_python() {
    // oracle: [html.unescape("&#%d;" % n) for n in range(0x110000)]: 94 empty results and
    // 2050 U+FFFD results (0, the 2048 surrogates and U+FFFD itself)
    let mut empty = 0;
    let mut replacement = 0;
    for code in 0..=0x10FFFF_u32 {
        match unescaped(&format!("&#{code};")).as_str() {
            "" => empty += 1,
            "\u{FFFD}" => replacement += 1,
            _ => {}
        }
    }
    assert_eq!((empty, replacement), (94, 2050));
    // oracle: [ord(html.unescape("&#%d;" % n)) for n in range(0x80, 0xa0)]
    let c1: Vec<u32> = (0x80..0xA0)
        .map(|code| {
            unescaped(&format!("&#{code};"))
                .chars()
                .map(u32::from)
                .sum()
        })
        .collect();
    assert_eq!(
        c1,
        [
            8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143,
            144, 8216, 8217, 8220, 8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382,
            376
        ]
    );
}

#[test]
fn legacy_name_bound_matches_the_table() {
    let longest = NAMED_ENTITIES
        .entries()
        .filter(|&(name, codes)| !name.ends_with(';') && *codes != (0, 0))
        .map(|(name, _)| name.chars().count())
        .max();
    assert_eq!(longest, Some(LEGACY_NAME_MAX_CHARS));
}

#[test]
fn entity_table_matches_python() {
    // oracle:
    //   from html.entities import html5 as t; h=0xcbf29ce484222325
    //   for k in sorted(t):
    //       for b in (k+"\t"+" ".join("%X"%ord(c) for c in t[k])+"\n").encode(): h=((h^b)*0x100000001b3)%2**64
    //   print(len(t), hex(h))
    let mut rows: Vec<(&str, (u32, u32))> = NAMED_ENTITIES
        .entries()
        .map(|(name, codes)| (*name, *codes))
        .filter(|&(_, codes)| codes != (0, 0))
        .collect();
    rows.sort_unstable();
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for (name, (first, second)) in &rows {
        let codes = if *second == 0 {
            format!("{first:X}")
        } else {
            format!("{first:X} {second:X}")
        };
        for byte in format!("{name}\t{codes}\n").bytes() {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    assert_eq!((rows.len(), digest), (2231, 0x816a_a585_04ba_cc0b));
}
