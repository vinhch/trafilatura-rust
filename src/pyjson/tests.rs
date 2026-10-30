use std::time::{Duration, Instant};

use super::*;
use crate::cancel::CancelToken;
use crate::utils::as_list;

/// `@` stands for a backslash in test text, so the source holds no backslash-u escape.
fn bs(text: &str) -> String {
    text.replace('@', "\\")
}

fn parse(text: &str, strict: bool) -> Option<Json> {
    loads(&bs(text), strict, &Call::default()).unwrap()
}

/// Text outside printable ASCII, and `"` and `\`, is written as `<hex code point>`.
fn esc(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if (c.is_ascii_graphic() || c == ' ') && c != '"' && c != '\\' {
            out.push(c);
        } else {
            out.push_str(&format!("<{:x}>", u32::from(c)));
        }
    }
    out
}

/// Recursive on purpose: only for small documents.
fn show(json: &Json, r: JsonRef) -> String {
    match json.kind(r) {
        Kind::Null => "null".to_string(),
        Kind::Bool => "bool".to_string(),
        Kind::Number => "N".to_string(),
        Kind::Str => format!("\"{}\"", esc(json.as_str(r).unwrap())),
        Kind::List => {
            let items: Vec<String> = json.items(r).map(|item| show(json, item)).collect();
            format!("[{}]", items.join(","))
        }
        Kind::Object => {
            let entries: Vec<String> = json
                .entries(r)
                .map(|(key, value)| format!("\"{}\":{}", esc(key), show(json, value)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
    }
}

fn shown(text: &str, strict: bool) -> Option<String> {
    parse(text, strict).map(|json| show(&json, json.root()))
}

fn nested(open: &str, innermost: &str, close: &str, depth: usize) -> String {
    format!("{}{innermost}{}", open.repeat(depth), close.repeat(depth))
}

/// Counts nodes and the deepest level with an explicit stack.
fn walk(json: &Json) -> (usize, usize) {
    let (mut count, mut deepest) = (0, 0);
    let mut stack = vec![(json.root(), 1)];
    while let Some((r, depth)) = stack.pop() {
        count += 1;
        deepest = deepest.max(depth);
        stack.extend(json.items(r).map(|item| (item, depth + 1)));
        stack.extend(json.entries(r).map(|(_, value)| (value, depth + 1)));
    }
    (count, deepest)
}

type Builder = (&'static str, fn(usize) -> String);

type Row = (&'static str, Option<&'static str>, Option<&'static str>);

// Rows are (text, expected with strict=False, expected with strict=True), `None` for a Python
// ValueError or RecursionError. `@` is a backslash.
const ROWS: &[Row] = &[
    // oracle: json.loads('{"a":"x\ty\x01"}', strict=False), strict=True
    ("{\"a\":\"x\ty\x01\"}", Some("{\"a\":\"x<9>y<1>\"}"), None),
    // oracle: json.loads('"\\ud800x"')
    ("\"@ud800x\"", Some("\"<ffff>x\""), Some("\"<ffff>x\"")),
    // oracle: json.loads('"\\ud83d\\ude00"')
    ("\"@ud83d@ude00\"", Some("\"<1f600>\""), Some("\"<1f600>\"")),
    // oracle: json.loads('[NaN, Infinity, -Infinity, 1e999, -0, 1.0]')
    (
        "[NaN, Infinity, -Infinity, 1e999, -0, 1.0]",
        Some("[N,N,N,N,N,N]"),
        Some("[N,N,N,N,N,N]"),
    ),
    // oracle: json.loads('{"a":1,"b":2,"a":3}') gives {'a': 3, 'b': 2}, keys in that order
    (
        "{\"a\":1,\"b\":2,\"a\":3}",
        Some("{\"a\":N,\"b\":N}"),
        Some("{\"a\":N,\"b\":N}"),
    ),
    // oracle: json.loads('{"a":"first","b":2,"a":"last"}') gives {'a': 'last', 'b': 2}
    (
        "{\"a\":\"first\",\"b\":2,\"a\":\"last\"}",
        Some("{\"a\":\"last\",\"b\":N}"),
        Some("{\"a\":\"last\",\"b\":N}"),
    ),
    // oracle: json.loads(chr(0xFEFF) + '{}') raises ValueError
    ("\u{FEFF}{}", None, None),
    // oracle: json.loads(' \n{}\r\t ') gives {}
    (" \n{}\r\t ", Some("{}"), Some("{}")),
    // oracle: json.loads('\xa0{}') raises ValueError
    ("\u{A0}{}", None, None),
    // oracle: json.loads('{} {}') raises ValueError
    ("{} {}", None, None),
    // oracle: json.loads('[1,]') raises ValueError
    ("[1,]", None, None),
    // oracle: json.loads('[01]') raises ValueError
    ("[01]", None, None),
    // oracle: json.loads('["\\x41"]') raises ValueError
    ("[\"@x41\"]", None, None),
    // oracle: json.loads('') raises ValueError
    ("", None, None),
    // oracle: json.loads('""') gives ''
    ("\"\"", Some("\"\""), Some("\"\"")),
    // oracle: json.loads('{"k\\ud800":1,"k\\ud800":2,"k\\ud801":3}') gives 2 keys, 'k\ud800' and 'k\ud801'
    (
        "{\"k@ud800\":1,\"k@ud800\":2,\"k@ud801\":3}",
        Some("{\"k<ffff>\":N,\"k<ffff>\":N}"),
        Some("{\"k<ffff>\":N,\"k<ffff>\":N}"),
    ),
    // oracle: json.loads('"\\ud800\\uZZZZ"') raises ValueError
    ("\"@ud800@uZZZZ\"", None, None),
    // oracle: json.loads('"\\ud800\\u0041"') gives '\ud800A'
    ("\"@ud800@u0041\"", Some("\"<ffff>A\""), Some("\"<ffff>A\"")),
    // oracle: json.loads('"\\udc00\\ud800"') gives '\udc00\ud800'
    (
        "\"@udc00@ud800\"",
        Some("\"<ffff><ffff>\""),
        Some("\"<ffff><ffff>\""),
    ),
    // oracle: json.loads('"\\ud800\\ud83d\\ude00"') gives '\ud800\U0001f600'
    (
        "\"@ud800@ud83d@ude00\"",
        Some("\"<ffff><1f600>\""),
        Some("\"<ffff><1f600>\""),
    ),
    // oracle: json.loads('"\\ud83d\\ude00\\ud83d"') gives '\U0001f600\ud83d'
    (
        "\"@ud83d@ude00@ud83d\"",
        Some("\"<1f600><ffff>\""),
        Some("\"<1f600><ffff>\""),
    ),
    // oracle: json.loads('"\\ud800\\"') raises ValueError
    ("\"@ud800@\"", None, None),
    // oracle: json.loads('"\\u00e9\\/\\b\\f\\n\\r\\t\\"\\\\"') gives '\xe9/\x08\x0c\n\r\t"\\'
    (
        "\"@u00e9@/@b@f@n@r@t@\"@@\"",
        Some("\"<e9>/<8><c><a><d><9><22><5c>\""),
        Some("\"<e9>/<8><c><a><d><9><22><5c>\""),
    ),
    // oracle: json.loads('"\\u12"') raises ValueError
    ("\"@u12\"", None, None),
    // oracle: json.loads('"\\u12G4"') raises ValueError
    ("\"@u12G4\"", None, None),
    // oracle: json.loads('"\\ud800\\u12"') raises ValueError
    ("\"@ud800@u12\"", None, None),
    // oracle: json.loads('"\\uD83D\\uDE00"') gives '\U0001f600'
    ("\"@uD83D@uDE00\"", Some("\"<1f600>\""), Some("\"<1f600>\"")),
    // oracle: json.loads('{"\\u0061":1}') gives {'a': 1}
    ("{\"@u0061\":1}", Some("{\"a\":N}"), Some("{\"a\":N}")),
    // oracle: json.loads('1.') raises ValueError
    ("1.", None, None),
    // oracle: json.loads('1.e5') raises ValueError
    ("1.e5", None, None),
    // oracle: json.loads('1e') raises ValueError
    ("1e", None, None),
    // oracle: json.loads('1e+') raises ValueError
    ("1e+", None, None),
    // oracle: json.loads('1E+5') gives 100000.0
    ("1E+5", Some("N"), Some("N")),
    // oracle: json.loads('-') raises ValueError
    ("-", None, None),
    // oracle: json.loads('-Infinity') gives -inf
    ("-Infinity", Some("N"), Some("N")),
    // oracle: json.loads('-Inf') raises ValueError
    ("-Inf", None, None),
    // oracle: json.loads('+1') raises ValueError
    ("+1", None, None),
    // oracle: json.loads('.5') raises ValueError
    (".5", None, None),
    // oracle: json.loads('0e0') gives 0.0
    ("0e0", Some("N"), Some("N")),
    // oracle: json.loads('-0.0e-0') gives -0.0
    ("-0.0e-0", Some("N"), Some("N")),
    // oracle: json.loads('-01') raises ValueError
    ("-01", None, None),
    // oracle: json.loads('1.5e3x') raises ValueError
    ("1.5e3x", None, None),
    // oracle: json.loads('00000') raises ValueError
    ("00000", None, None),
    // oracle: json.loads('-NaN') raises ValueError
    ("-NaN", None, None),
    // oracle: json.loads('nan') raises ValueError
    ("nan", None, None),
    // oracle: json.loads('Infinit') raises ValueError
    ("Infinit", None, None),
    // oracle: json.loads('tru') raises ValueError
    ("tru", None, None),
    // oracle: json.loads('nul') raises ValueError
    ("nul", None, None),
    // oracle: json.loads('nullx') raises ValueError
    ("nullx", None, None),
    // oracle: json.loads('[truefalse]') raises ValueError
    ("[truefalse]", None, None),
    // oracle: json.loads('[1 2]') raises ValueError
    ("[1 2]", None, None),
    // oracle: json.loads('["a" "b"]') raises ValueError
    ("[\"a\" \"b\"]", None, None),
    // oracle: json.loads('{"a" 1}') raises ValueError
    ("{\"a\" 1}", None, None),
    // oracle: json.loads('{1:2}') raises ValueError
    ("{1:2}", None, None),
    // oracle: json.loads('{"a":1,}') raises ValueError
    ("{\"a\":1,}", None, None),
    // oracle: json.loads('[,1]') raises ValueError
    ("[,1]", None, None),
    // oracle: json.loads('[1,2') raises ValueError
    ("[1,2", None, None),
    // oracle: json.loads('{"a":1') raises ValueError
    ("{\"a\":1", None, None),
    // oracle: json.loads('{"a"') raises ValueError
    ("{\"a\"", None, None),
    // oracle: json.loads('"abc') raises ValueError
    ("\"abc", None, None),
    // oracle: json.loads('[1]]') raises ValueError
    ("[1]]", None, None),
    // oracle: json.loads('{"a":1}{') raises ValueError
    ("{\"a\":1}{", None, None),
    // oracle: json.loads('[] x') raises ValueError
    ("[] x", None, None),
    // oracle: json.loads('1 2') raises ValueError
    ("1 2", None, None),
    // oracle: json.loads('[ ]') gives []
    ("[ ]", Some("[]"), Some("[]")),
    // oracle: json.loads('{ }') gives {}
    ("{ }", Some("{}"), Some("{}")),
    // oracle: json.loads('{ "a" : [ 1 , { "b" : null } ] }') gives {'a': [1, {'b': None}]}
    (
        "{ \"a\" : [ 1 , { \"b\" : null } ] }",
        Some("{\"a\":[N,{\"b\":null}]}"),
        Some("{\"a\":[N,{\"b\":null}]}"),
    ),
    // oracle: json.loads('[[],{},[{}],{"a":[]}]') gives [[], {}, [{}], {'a': []}]
    (
        "[[],{},[{}],{\"a\":[]}]",
        Some("[[],{},[{}],{\"a\":[]}]"),
        Some("[[],{},[{}],{\"a\":[]}]"),
    ),
    // oracle: json.loads('{"a":{"b":{"c":[1,2,[3]]}}}')
    (
        "{\"a\":{\"b\":{\"c\":[1,2,[3]]}}}",
        Some("{\"a\":{\"b\":{\"c\":[N,N,[N]]}}}"),
        Some("{\"a\":{\"b\":{\"c\":[N,N,[N]]}}}"),
    ),
    // oracle: json.loads('true') and json.loads(' false ') are bools
    ("true", Some("bool"), Some("bool")),
    (" false ", Some("bool"), Some("bool")),
    // oracle: json.loads('\x0b[]') and json.loads('\x0c[]') raise ValueError
    ("\u{b}[]", None, None),
    ("\u{c}[]", None, None),
    // oracle: json.loads('"a\x1fb"'), strict=True raises ValueError
    ("\"a\u{1f}b\"", Some("\"a<1f>b\""), None),
    // oracle: json.loads('"a\x7fb"') accepts a raw DEL in both modes
    ("\"a\u{7f}b\"", Some("\"a<7f>b\""), Some("\"a<7f>b\"")),
    // oracle: json.loads('"' + chr(0x2028) + '"') accepts U+2028 in both modes
    ("\"\u{2028}\"", Some("\"<2028>\""), Some("\"<2028>\"")),
    // oracle: json.loads('"\x00"'), strict=True raises ValueError
    ("\"\u{0}\"", Some("\"<0>\""), None),
    // oracle: json.loads('"a\nb"'), strict=True raises ValueError
    ("\"a\nb\"", Some("\"a<a>b\""), None),
    // oracle: json.loads('[' + chr(0x661) + ']') and json.loads(chr(0x661)) raise ValueError (Arabic-Indic digit)
    ("[\u{661}]", None, None),
    ("\u{661}", None, None),
    // oracle: json.loads('"\xe9\U0001f600"') keeps non-ASCII text
    (
        "\"\u{e9}\u{1f600}\"",
        Some("\"<e9><1f600>\""),
        Some("\"<e9><1f600>\""),
    ),
];

#[test]
fn loads_matches_python_rows() {
    for (text, loose, strict) in ROWS {
        assert_eq!(
            shown(text, false).as_deref(),
            *loose,
            "strict=False {text:?}"
        );
        assert_eq!(
            shown(text, true).as_deref(),
            *strict,
            "strict=True {text:?}"
        );
    }
}

#[test]
fn loads_matches_python_integer_digit_limit() {
    let digits = |n: usize, tail: &str| format!("[{}{tail}]", "1".repeat(n));
    // oracle: json.loads('[' + '1' * 4301 + ']') raises ValueError, 4300 digits parse
    assert!(parse(&digits(4301, ""), false).is_none());
    assert!(parse(&digits(4300, ""), false).is_some());
    // oracle: json.loads('[' + '1' * 5000 + '.5]') parses: a float has no digit limit
    assert!(parse(&digits(5000, ".5"), false).is_some());
    // oracle: json.loads('1' * 4300) parses, '1' * 4301 and '-' + '1' * 4301 raise ValueError
    assert!(parse(&"1".repeat(4300), false).is_some());
    assert!(parse(&"1".repeat(4301), false).is_none());
    assert!(parse(&format!("-{}", "1".repeat(4301)), false).is_none());
    // oracle: json.loads('[' + '1' * 5000 + 'e5]') parses
    assert!(parse(&digits(5000, "e5"), false).is_some());
}

#[test]
fn loads_keeps_the_last_value_at_the_first_position() {
    let json = parse(r#"{"a":"first","b":"x","a":"last"}"#, false).unwrap();
    let root = json.root();
    // oracle: json.loads('{"a":"first","b":"x","a":"last"}') gives {'a': 'last', 'b': 'x'}
    let keys: Vec<&str> = json.entries(root).map(|(key, _)| key).collect();
    assert_eq!(keys, ["a", "b"]);
    assert_eq!(json.as_str(json.get(root, "a").unwrap()), Some("last"));
    assert_eq!(json.get(root, "missing"), None);
}

#[test]
fn loads_reads_kinds() {
    let json = parse(r#"{"s":"x","n":1,"t":true,"z":null,"l":[],"o":{}}"#, true).unwrap();
    let root = json.root();
    let kind = |key: &str| json.kind(json.get(root, key).unwrap());
    assert_eq!(json.kind(root), Kind::Object);
    assert_eq!(kind("s"), Kind::Str);
    assert_eq!(kind("n"), Kind::Number);
    assert_eq!(kind("t"), Kind::Bool);
    assert_eq!(kind("z"), Kind::Null);
    assert_eq!(kind("l"), Kind::List);
    assert_eq!(kind("o"), Kind::Object);
    assert_eq!(json.as_str(root), None);
    assert_eq!(json.items(root).len(), 0);
    assert_eq!(json.entries(json.get(root, "l").unwrap()).len(), 0);
}

#[test]
fn loads_depth_cap() {
    // Each builder gives a document of exactly `depth` nested containers.
    let builders: [Builder; 4] = [
        // oracle: json.loads('[' * 9998 + '1' + ']' * 9998) parses, 9999 raises RecursionError
        ("list over a number", |depth| nested("[", "1", "]", depth)),
        // oracle: json.loads('[' * 9998 + ']' * 9998) parses, 9999 raises RecursionError
        ("list over an empty list", |depth| {
            nested("[", "", "]", depth)
        }),
        // oracle: json.loads('{"a":' * 9998 + '1' + '}' * 9998) parses, 9999 raises RecursionError
        ("object over a number", |depth| {
            nested("{\"a\":", "1", "}", depth)
        }),
        // oracle: json.loads('{"a":' * 9997 + '{}' + '}' * 9997) parses, one level more raises
        // RecursionError
        ("object over an empty object", |depth| {
            nested("{\"a\":", "{}", "}", depth - 1)
        }),
    ];
    for (name, build) in builders {
        assert!(parse(&build(9998), false).is_some(), "{name} at 9998");
        assert!(parse(&build(9999), false).is_none(), "{name} at 9999");
    }
}

#[test]
fn loads_surrogate_keys_stay_distinct() {
    // oracle: json.loads('{"topic_\\ud800":1,"topic_\\ud801":2,"topic_\\uffff":3}') has 3 keys
    for strict in [false, true] {
        let json = parse(
            "{\"topic_@ud800\":1,\"topic_@ud801\":2,\"topic_@uffff\":3}",
            strict,
        )
        .unwrap();
        let keys: Vec<&str> = json.entries(json.root()).map(|(key, _)| key).collect();
        assert_eq!(keys, ["topic_\u{FFFF}"; 3]);
    }
}

#[test]
fn loads_surrogate_keys_stay_distinct_in_a_large_object() {
    // The three keys after 20 entries, where the hash index is in use. The lone surrogate keys
    // t+D800 and t+D801 and the plain key t+FFFF read alike; the repeats of t+D800 and t+FFFF
    // replace the values of their first entries.
    let filler: String = (0..20).map(|i| format!("\"f{i}\":1,")).collect();
    let text = format!(
        "{{{filler}\"t@ud800\":\"a\",\"t@ud801\":\"b\",\"t@uffff\":\"c\",\"t@ud800\":\"d\",\"t\u{FFFF}\":\"e\"}}"
    );
    let json = parse(&text, false).unwrap();
    // oracle: list(json.loads(text).items())[20:] gives three keys with values 'd', 'b', 'e'
    let tail: Vec<(&str, &str)> = json
        .entries(json.root())
        .skip(20)
        .map(|(key, value)| (key, json.as_str(value).unwrap()))
        .collect();
    assert_eq!(json.entries(json.root()).count(), 23);
    assert_eq!(
        tail,
        [("t\u{FFFF}", "d"), ("t\u{FFFF}", "b"), ("t\u{FFFF}", "e")]
    );
}

#[test]
fn loads_many_duplicate_keys_is_linear() {
    let distinct: String = (0..100_000).map(|i| format!("\"k{i}\":1,")).collect();
    let distinct = format!("{{{}}}", distinct.trim_end_matches(','));
    let cycling: String = (0..100_000)
        .map(|i| format!("\"k{}\":1,", i % 1_000))
        .collect();
    let cycling = format!("{{{}}}", cycling.trim_end_matches(','));
    let started = Instant::now();
    let json = loads(&distinct, false, &Call::default()).unwrap().unwrap();
    assert_eq!(json.entries(json.root()).count(), 100_000);
    assert!(started.elapsed() < Duration::from_secs(1));
    let started = Instant::now();
    let json = loads(&cycling, false, &Call::default()).unwrap().unwrap();
    // oracle: len(json.loads('{' + ','.join('"k%d":1' % (i % 1000) for i in range(100000)) + '}')) == 1000
    assert_eq!(json.entries(json.root()).count(), 1_000);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn loads_checks_call() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert!(matches!(loads("", false, &call), Ok(None)));

    // `CHECK_INTERVAL` is 256. The counter starts at 1, so the first tick (at 1) cannot see the
    // token: only the tick of the 256th value, inside the parse loop, can. The list and each of
    // its values tick once.
    let parse_list = |items: usize| {
        let text = format!("[{}]", vec!["0"; items].join(","));
        let mut parser = Parser::new(&text, false);
        parser.visited = 1;
        parser.run(&call)
    };
    assert!(matches!(
        parse_list(300),
        Err(Stop::Call(ExtractError::Cancelled))
    ));
    assert!(matches!(parse_list(200), Ok(())));
}

#[test]
fn deep_document_drops_on_small_stack() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            for (open, innermost, close) in [("[", "1", "]"), ("{\"a\":", "1", "}"), ("[", "", "]")]
            {
                let text = nested(open, innermost, close, 9998);
                let json = loads(&text, false, &Call::default()).unwrap().unwrap();
                let (count, deepest) = walk(&json);
                assert_eq!(deepest, 9998 + usize::from(!innermost.is_empty()));
                assert_eq!(count, deepest);
                assert_eq!(as_list(&json, Some(json.root())).len(), 1);
                drop(json);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn dumps_str_matches_python() {
    let rows = [
        // oracle: json.dumps('x\x1c', ensure_ascii=False) gives '"x\\u001c"'
        ("x\u{1c}", "\"x@u001c\""),
        // oracle: json.dumps('a\x7fb', ensure_ascii=False) keeps a raw DEL
        ("a\u{7f}b", "\"a\u{7f}b\""),
        // oracle: json.dumps('"\\\n\r\t\x08\x0c', ensure_ascii=False) gives '"\\"\\\\\\n\\r\\t\\b\\f"'
        ("\"\\\n\r\t\u{8}\u{c}", "\"@\"@@@n@r@t@b@f\""),
        // oracle: json.dumps(chr(0xe9) + chr(0x2028), ensure_ascii=False) keeps both raw
        ("\u{e9}\u{2028}", "\"\u{e9}\u{2028}\""),
        // oracle: json.dumps('\x00\x1f', ensure_ascii=False) gives '"\\u0000\\u001f"'
        ("\u{0}\u{1f}", "\"@u0000@u001f\""),
        // oracle: json.dumps('\x7f\x80', ensure_ascii=False) keeps both raw
        ("\u{7f}\u{80}", "\"\u{7f}\u{80}\""),
        // oracle: json.dumps('a/b', ensure_ascii=False) does not escape the slash
        ("a/b", "\"a/b\""),
        // oracle: json.dumps('\U0001f600', ensure_ascii=False) keeps the code point
        ("\u{1f600}", "\"\u{1f600}\""),
        // oracle: json.dumps('', ensure_ascii=False) gives '""'
        ("", "\"\""),
    ];
    for (text, expected) in rows {
        assert_eq!(dumps_str(text), bs(expected), "{text:?}");
    }
}
