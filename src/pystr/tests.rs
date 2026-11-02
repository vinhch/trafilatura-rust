use regex::Regex;

use super::*;

// oracle: [hex(c) for c in range(0x110000) if chr(c).isspace()]
const PYTHON_SPACE: &[u32] = &[
    0x9, 0xa, 0xb, 0xc, 0xd, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x2001,
    0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f,
    0x205f, 0x3000,
];

fn every_char() -> impl Iterator<Item = char> {
    (0..=0x10FFFF).filter_map(char::from_u32)
}

#[test]
fn is_space_matches_python() {
    let rust_misses: Vec<char> = PYTHON_SPACE
        .iter()
        .filter_map(|&cp| char::from_u32(cp))
        .filter(|c| !c.is_whitespace())
        .collect();
    // Rust's `char::is_whitespace` disagrees on exactly these.
    assert_eq!(rust_misses, ['\x1c', '\x1d', '\x1e', '\x1f']);
    let ours: Vec<u32> = every_char()
        .filter(|&c| is_space(c))
        .map(u32::from)
        .collect();
    assert_eq!(ours, PYTHON_SPACE);
    // oracle: chr(0x200b).isspace(), chr(0xfeff).isspace()
    assert!(!is_space('\u{200b}') && !is_space('\u{feff}'));
}

#[test]
fn is_all_space_is_false_for_empty() {
    assert!(!is_all_space("")); // oracle: "".isspace()
    assert!(is_all_space(" \x1c\u{a0}\n")); // oracle: " \x1c\xa0\n".isspace()
    assert!(!is_all_space(" a ")); // oracle: " a ".isspace()
}

#[test]
fn split_whitespace_and_strip_match_python() {
    // oracle: "  a\xa0b\x1cc \x1f ".split()
    let parts: Vec<&str> = split_whitespace("  a\u{a0}b\x1cc \x1f ").collect();
    assert_eq!(parts, ["a", "b", "c"]);
    // oracle: " \t".split()
    assert_eq!(split_whitespace(" \t").count(), 0);
    // oracle: " \x1c\xa0a b\x85 ".strip()
    assert_eq!(strip(" \x1c\u{a0}a b\u{85} "), "a b");
    // oracle: chr(0x200b).strip() == chr(0x200b)
    assert_eq!(strip("\u{200b}"), "\u{200b}");
}

#[test]
fn lstrip_rstrip_match_python() {
    // oracle: "\x1c a \x1f".lstrip(), "\x1c a \x1f".rstrip(), "\x85　".lstrip()
    assert_eq!(lstrip("\x1c a \x1f"), "a \x1f");
    assert_eq!(rstrip("\x1c a \x1f"), "\x1c a");
    assert_eq!(lstrip("\u{85}\u{3000}"), "");
    assert_eq!(rstrip("\u{85}\u{3000}"), "");
}

#[test]
fn int_str_matches_python() {
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    // oracle: str(int(s)), or the exception name
    let long_underscored = "1_".repeat(2500) + "1";
    let long_negative = format!("-{}7", "0".repeat(4299));
    let too_long = format!("{}1", "0".repeat(4300));
    let (expected_long, zeros) = ("1".repeat(2501), "0".repeat(4300));
    for (s, expected) in [
        ("2", "2"),
        (" 1_0 ", "10"),
        ("-0", "0"),
        ("+5", "5"),
        ("-007", "-7"),
        ("\u{661}\u{662}", "12"),
        ("\u{FF11}", "1"),
        ("\u{85}1\u{3000}", "1"),
        (long_underscored.as_str(), expected_long.as_str()),
        (long_negative.as_str(), "-7"),
        ("1_2_3", "123"),
        (" \t\n\x0b\x0c\r7\r", "7"),
        ("\u{663}_\u{664}", "34"),
        (zeros.as_str(), "0"),
    ] {
        assert_eq!(
            int_str(s).as_deref(),
            Ok(expected),
            "{:?}",
            s.chars().take(20).collect::<String>()
        );
    }
    for s in [
        "",
        "x",
        "1__0",
        "_1",
        "1_",
        "1 2",
        "\x1c1\x1f",
        "\x1f7",
        "\u{B2}",
        "+",
        "-",
        "- 1",
        "+-1",
        too_long.as_str(),
    ] {
        assert_eq!(
            int_str(s),
            invalid,
            "{:?}",
            s.chars().take(20).collect::<String>()
        );
    }
}

#[test]
fn splitlines_matches_python() {
    // oracle: "a\nb\rc\r\nd\x0be\x0cf\x1cg\x85h i j".splitlines()
    let parts: Vec<&str> = splitlines("a\nb\rc\r\nd\x0be\x0cf\x1cg\u{85}h i j").collect();
    assert_eq!(parts, ["a", "b", "c", "d", "e", "f", "g", "h i j"]);
    // oracle: ("a\x1db\x1ec" + chr(0x2028) + "d" + chr(0x2029) + "e\n").splitlines()
    let parts: Vec<&str> = splitlines("a\x1db\x1ec\u{2028}d\u{2029}e\n").collect();
    assert_eq!(parts, ["a", "b", "c", "d", "e"]);
    // oracle: ("a\x1fb" + chr(0x200b) + "c").splitlines()
    assert_eq!(
        splitlines("a\x1fb\u{200b}c").collect::<Vec<_>>(),
        ["a\x1fb\u{200b}c"]
    );
}

#[test]
fn splitlines_edges_match_python() {
    let lines = |s| splitlines(s).collect::<Vec<_>>();
    assert!(lines("").is_empty()); // oracle: "".splitlines()
    assert_eq!(lines("\n"), [""]); // oracle: "\n".splitlines()
    assert_eq!(lines("a\n\nb"), ["a", "", "b"]); // oracle: "a\n\nb".splitlines()
    assert_eq!(lines("a\r\n\r\n"), ["a", ""]); // oracle: "a\r\n\r\n".splitlines()
    assert_eq!(lines("\r\n"), [""]); // oracle: "\r\n".splitlines()
}

#[test]
fn is_printable_matches_python() {
    assert!(is_printable(' ')); // oracle: " ".isprintable()
    assert!(!is_printable('\u{a0}')); // oracle: "\xa0".isprintable()
    assert!(!is_printable('\u{200b}')); // oracle: chr(0x200b).isprintable()
    assert!(!is_printable('\u{378}')); // oracle: chr(0x378).isprintable()
    assert!(is_printable('\u{1F600}')); // oracle: chr(0x1F600).isprintable()
    assert!(!is_printable('\x7f')); // oracle: "\x7f".isprintable()
    assert!(!is_printable('\u{ad}')); // oracle: "\xad".isprintable()
    assert!(!is_printable('\u{2028}')); // oracle: chr(0x2028).isprintable()
    assert!(!is_printable('\u{e000}')); // oracle: chr(0xe000).isprintable()
    assert!(!is_printable('\u{e0001}')); // oracle: chr(0xe0001).isprintable()
}

#[test]
fn title_matches_python() {
    for &(input, expected) in title::TITLE_CASES {
        assert_eq!(title(input), expected, "{input:?}");
    }
}

#[test]
fn is_upper_matches_python() {
    for &(start, end) in UPPER {
        for edge in [start, end] {
            assert!(char::from_u32(edge).is_some_and(is_upper), "U+{edge:04X}");
        }
        for outside in [start.checked_sub(1), end.checked_add(1)]
            .into_iter()
            .flatten()
        {
            assert!(
                !char::from_u32(outside).is_some_and(is_upper),
                "U+{outside:04X}"
            );
        }
    }
    // oracle: [c.isupper() for c in ("A", "a", "ǅ", "Ⅻ")]
    assert!(is_upper('A') && !is_upper('a') && !is_upper('\u{1C5}') && is_upper('\u{216B}'));
}

#[test]
fn unicode_escape_matches_python() {
    // oracle: s.encode().decode("unicode_escape"), or None for the UnicodeDecodeError
    for (input, expected) in [
        ("Fran\u{E7}ois", Some("Fran\u{C3}\u{A7}ois")),
        (r"a\x41\101\n", Some("aAA\n")),
        (r"\q", Some(r"\q")),
        (r"\8", Some(r"\8")),
        ("\u{E9}\\u0041", Some("\u{C3}\u{A9}A")),
        ("\\\u{E9}", Some("\\\u{C3}\u{A9}")),
        (r"\400\777", Some("\u{100}\u{1FF}")),
        (r"\1234", Some("S4")),
        (r"\0", Some("\0")),
        (r"\u12", None),
        (r"\x4", None),
        (r"\xZ1", None),
        (r"ab\", None),
        (r"\U00110000", None),
    ] {
        assert_eq!(unicode_escape(input).as_deref(), expected, "{input:?}");
    }
    // CPython's rules for these, which the oracle's argument passing cannot carry
    assert_eq!(unicode_escape("a\\\\b\\\nc").as_deref(), Some("a\\bc"));
}

#[test]
fn unicode_escape_unported_escapes() {
    // D-20: upstream decodes `\N{BULLET}` to U+2022 and `\ud83d` to a lone surrogate
    assert_eq!(unicode_escape(r"\N{BULLET}"), None);
    // Built with `concat!` so the source never holds a literal surrogate escape
    let surrogate_pair = concat!("a", "\\", "ud83d", "\\", "ude00");
    assert_eq!(
        unicode_escape(surrogate_pair).as_deref(),
        Some("a\u{FFFF}\u{FFFF}")
    );
}

#[test]
fn lower_matches_python() {
    // oracle: [s.lower() for s in ("ABİc", "ΑΣ", "Σ", "Α.ΣΣ", "\U00010d50", "\U00010570")]
    assert_eq!(lower("AB\u{130}c"), "ab\u{69}\u{307}c");
    assert_eq!(lower("\u{391}\u{3A3}"), "\u{3B1}\u{3C2}");
    assert_eq!(lower("\u{3A3}"), "\u{3C3}");
    assert_eq!(lower("\u{391}.\u{3A3}\u{3A3}"), "\u{3B1}.\u{3C3}\u{3C2}");
    assert_eq!(lower("\u{10D50}"), "\u{10D50}");
    assert_eq!(lower("\u{10570}"), "\u{10597}");
    // oracle: chr(0x10d50).lower() is unchanged, while Rust's `to_lowercase` maps it
    assert_ne!("\u{10D50}".to_lowercase(), "\u{10D50}");
}

#[test]
fn is_alnum_and_is_punctuation_match_python() {
    // oracle: c.isalnum() and unicodedata.category(c)[0] == "P"
    for c in ['a', 'Z', '1', '²', '٣', 'Ⅻ', '一', 'é'] {
        assert!(is_alnum(c), "{c:?}");
    }
    for c in ['_', '-', ' ', '\u{301}', '$', '。'] {
        assert!(!is_alnum(c), "{c:?}");
    }
    for c in ['_', '-', '!', '。', '«', '、'] {
        assert!(is_punctuation(c), "{c:?}");
    }
    for c in ['$', '+', 'a', ' ', '\u{301}'] {
        assert!(!is_punctuation(c), "{c:?}");
    }
}

#[test]
fn is_alpha_matches_python() {
    // oracle: [c.isalpha() for c in (chr(0x2160), chr(0x345), chr(0x1c5), chr(0xe9))]
    assert!(!is_alpha('\u{2160}'));
    assert!(!is_alpha('\u{345}'));
    assert!(is_alpha('\u{1c5}'));
    assert!(is_alpha('\u{e9}'));
    // oracle: chr(0x2160).isalpha() is False, while Rust's `is_alphabetic` says true
    assert!('\u{2160}'.is_alphabetic());
    // oracle: [(c, chr(c).isalpha()) for c in range(0x110000)], by the edges of its true runs
    for &(start, end) in ALPHA {
        for (code, expected) in [
            (start.checked_sub(1), false),
            (Some(start), true),
            (Some(end), true),
            (end.checked_add(1), false),
        ] {
            let Some(c) = code.and_then(char::from_u32) else {
                continue;
            };
            assert_eq!(is_alpha(c), expected, "U+{:04X}", u32::from(c));
        }
    }
}

/// `repr` expected values name a backslash as `@`, so that no escape sequence stands in the source.
fn at(s: &str) -> String {
    s.replace('@', "\\")
}

#[test]
fn repr_matches_python() {
    // oracle: repr(s)
    for (s, expected) in [
        ("it's", at("\"it's\"")),
        ("say \"hi\"", at("'say \"hi\"'")),
        ("a\nb", at("'a@nb'")),
        ("\u{85}", at("'@x85'")),
        ("\u{e9}", at("'\u{e9}'")),
        ("it's \"x\"", at("'it@'s \"x\"'")),
        ("\u{2028}", at("'@u2028'")),
        ("\u{e0001}", at("'@U000e0001'")),
        ("a\\b", at("'a@@b'")),
        ("t\tc\r", at("'t@tc@r'")),
        ("\x7f\x1f\u{a0}", at("'@x7f@x1f@xa0'")),
        ("\u{ff}\u{100}", at("'\u{ff}\u{100}'")),
        ("'", at("\"'\"")),
        ("\"", at("'\"'")),
        ("", at("''")),
    ] {
        assert_eq!(repr(s), expected, "{s:?}");
    }
}

#[test]
fn is_all_printable_is_true_for_empty() {
    assert!(is_all_printable("")); // oracle: "".isprintable()
    assert!(is_all_printable("a b\u{1F600}")); // oracle: ("a b" + chr(0x1F600)).isprintable()
    assert!(!is_all_printable("a\nb")); // oracle: "a\nb".isprintable()
}

#[test]
fn is_word_matches_python() {
    // oracle: re.match(r"\w", c) is not None
    for (c, expected) in [
        ('\u{b2}', true),
        ('\u{301}', false),
        ('_', true),
        ('\u{203f}', false),
        ('a', true),
        ('\u{aa}', true),
        ('\u{2160}', true),
        ('\u{bd}', true),
        ('\u{660}', true),
        ('\u{200c}', false),
        ('\u{200d}', false),
        ('\u{b7}', false),
        ('-', false),
    ] {
        assert_eq!(is_word(c), expected, "U+{:04X}", u32::from(c));
    }
}

#[test]
fn decimal_matches_python() {
    // oracle: unicodedata.decimal(c, None)
    for (c, expected) in [
        ('0', Some(0)),
        ('9', Some(9)),
        ('\u{663}', Some(3)),
        ('\u{FF11}', Some(1)),
        ('\u{B2}', None),
        ('a', None),
    ] {
        assert_eq!(decimal_value(c), expected, "U+{:04X}", u32::from(c));
    }
    // oracle: [s.isdecimal() for s in ("007", chr(0x661) + chr(0x662), "", "-2", "2x", "\xb2", "1\xb2")]
    for (s, expected) in [
        ("007", true),
        ("\u{661}\u{662}", true),
        ("", false),
        ("-2", false),
        ("2x", false),
        ("\u{B2}", false),
        ("1\u{B2}", false),
    ] {
        assert_eq!(is_decimal(s), expected, "{s:?}");
    }
}

#[test]
fn nd_zeros_sorted_ten_apart() {
    assert!(ND_ZEROS.windows(2).all(|pair| {
        match pair {
            [a, b] => u32::from(*b)
                .checked_sub(u32::from(*a))
                .is_some_and(|gap| gap >= 10),
            _ => false,
        }
    }));
}

#[test]
fn ascii_fast_paths_match_tables() {
    for c in '\0'..='\x7f' {
        assert_eq!(is_space(c), in_ranges(SPACE, c), "{c:?}");
        assert_eq!(is_printable(c), in_ranges(PRINTABLE, c), "{c:?}");
    }
}

fn class_regex(class: &str) -> Regex {
    Regex::new(&format!(r"\A[{class}]\z")).expect("class compiles")
}

#[test]
fn space_class_matches_is_space() {
    let re = class_regex(PY_SPACE_CLASS);
    let mut buf = [0u8; 4];
    for c in every_char() {
        assert_eq!(
            re.is_match(c.encode_utf8(&mut buf)),
            is_space(c),
            "U+{:04X}",
            u32::from(c)
        );
    }
}

#[test]
fn word_class_matches_is_word() {
    let re = class_regex(PY_WORD_CLASS);
    let mut buf = [0u8; 4];
    for c in every_char() {
        assert_eq!(
            re.is_match(c.encode_utf8(&mut buf)),
            is_word(c),
            "U+{:04X}",
            u32::from(c)
        );
    }
    // Rust's own `\w` counts these differently from Python's
    let rust_word = Regex::new(r"\A\w\z").expect("compiles");
    assert!(re.is_match("\u{b2}") && !rust_word.is_match("\u{b2}"));
    assert!(!re.is_match("\u{301}") && rust_word.is_match("\u{301}"));
}

#[test]
fn digit_class_matches_decimal_value() {
    let re = class_regex(&digit_class());
    let mut buf = [0u8; 4];
    for c in every_char() {
        assert_eq!(
            re.is_match(c.encode_utf8(&mut buf)),
            decimal_value(c).is_some(),
            "U+{:04X}",
            u32::from(c)
        );
    }
}

#[test]
fn classes_compose_inside_larger_classes() {
    let re = Regex::new(&format!(r"\A[{PY_SPACE_CLASS}x-]+\z")).expect("compiles");
    assert!(re.is_match("x- \x1c\u{a0}"));
    assert!(!re.is_match("y"));
}

#[test]
fn is_digit_matches_python() {
    // Every run of `str.isdigit` starts and ends where the table says, and the code point past it is out.
    for &(first, last) in decimal::DIGIT {
        for code_point in [first, last] {
            let c = char::from_u32(code_point).expect("a scalar value");
            assert!(is_digit(c), "U+{code_point:04X}");
        }
        if let Some(after) = char::from_u32(last + 1) {
            assert!(!is_digit(after), "U+{:04X}", last + 1);
        }
    }
    // oracle: chr(0xb2).isdigit(), chr(0x663).isdigit(), chr(0xbd).isdigit(), chr(0x61).isdigit()
    assert!(is_digit('\u{b2}') && is_digit('\u{663}'));
    assert!(!is_digit('\u{bd}') && !is_digit('a'));
}

#[test]
fn ignore_case_i_dotless() {
    // oracle: re.fullmatch("yay\u{131}", s, re.I) is true for "yayi", "yayI", "yay\u{131}" and "yay\u{130}"
    let re = Regex::new(&format!(r"\A(?i:yay{})\z", ignore_case_i("\u{131}"))).expect("compiles");
    for text in ["yayi", "yayI", "yay\u{131}", "yay\u{130}"] {
        assert!(re.is_match(text), "{text:?}");
    }
    assert!(!re.is_match("yayy"), "a letter that is no i");
}
