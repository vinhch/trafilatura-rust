use std::time::{Duration, Instant};

use super::author::{
    AUTHOR_EMAIL, AUTHOR_EMOJI_REMOVE, AUTHOR_PREFIX, AUTHOR_REMOVE_NICKNAME,
    AUTHOR_REMOVE_NUMBERS, AUTHOR_REMOVE_PREPOSITION, AUTHOR_REMOVE_SPECIAL, AUTHOR_REPLACE_JOIN,
    AUTHOR_SPLIT, AUTHOR_TWITTER, author_name, joined_names,
};
use super::cases::{JSON_CASES, NORMALIZE_AUTHORS_CASES, NORMALIZE_JSON_CASES, PARSE_ERROR_CASES};
use super::*;
use crate::cancel::{CancelToken, CheckTrip};
use crate::pyjson::dumps_str;

fn fields(document: &Document) -> [Option<&str>; 4] {
    [
        document.title.as_deref(),
        document.author.as_deref(),
        document.sitename.as_deref(),
        document.pagetype.as_deref(),
    ]
}

fn category_list(document: &Document) -> Option<Vec<&str>> {
    document
        .categories
        .as_ref()
        .map(|list| list.iter().map(String::as_str).collect())
}

/// `json.loads(text, strict=False)` then `extract_json` on a new document.
fn run(text: &str, call: &Call<'_>) -> (Document, Result<(), JsonFailure>) {
    let mut document = Document::new();
    let result = match try_loads(text, false, &Call::default()).unwrap() {
        Ok(json) => extract_json(&json, json.root(), &mut document, call),
        Err(Invalid::Raised) => Err(JsonFailure::Raised),
        Err(Invalid::Decode) => panic!("not JSON: {text}"),
    };
    (document, result)
}

fn article_text(author: &str) -> String {
    format!(
        r#"{{"@context":"https://schema.org","@type":"Article","author":{}}}"#,
        dumps_str(author)
    )
}

#[test]
fn patterns_compile() {
    for pattern in [
        &JSON_AUTHOR_1,
        &JSON_AUTHOR_2,
        &JSON_AUTHOR_REMOVE,
        &JSON_PUBLISHER,
        &JSON_TYPE,
        &JSON_CATEGORY,
        &JSON_SCHEMA_ORG,
        &JSON_UNICODE_REPLACE,
        &JSON_NAME,
        &JSON_HEADLINE,
        &AUTHOR_PREFIX,
        &AUTHOR_REMOVE_NUMBERS,
        &AUTHOR_TWITTER,
        &AUTHOR_REPLACE_JOIN,
        &AUTHOR_REMOVE_NICKNAME,
        &AUTHOR_REMOVE_SPECIAL,
        &AUTHOR_REMOVE_PREPOSITION,
        &AUTHOR_EMAIL,
        &AUTHOR_SPLIT,
        &AUTHOR_EMOJI_REMOVE,
        &HTML_STRIP_TAGS,
    ] {
        assert!(pattern.is_some());
    }
}

#[test]
fn json_cases_match_python() {
    let call = Call::default();
    for case in JSON_CASES {
        let (document, result) = run(case.text, &call);
        let raised = matches!(result, Err(JsonFailure::Raised));
        assert!(result.is_ok() || raised, "stopped: {}", case.text);
        assert_eq!(raised, case.raised, "raised: {}", case.text);
        let expected = [case.title, case.author, case.sitename, case.pagetype];
        assert_eq!(
            fields(&document),
            expected,
            "title, author, sitename, pagetype (d19 {}): {}",
            case.d19,
            case.text
        );
        assert_eq!(
            category_list(&document),
            case.categories.map(<[&str]>::to_vec),
            "categories (d19 {}): {}",
            case.d19,
            case.text
        );
    }
}

#[test]
fn parse_error_cases_match_python() {
    let call = Call::default();
    for (text, expected) in PARSE_ERROR_CASES {
        let mut document = Document::new();
        extract_json_parse_error(text, &mut document, &call).unwrap();
        let [title, author, sitename, pagetype] = fields(&document);
        let actual = (author, pagetype, sitename, title);
        let wanted = (
            expected.author,
            expected.pagetype,
            expected.sitename,
            expected.title,
        );
        assert_eq!(actual, wanted, "{text}");
        assert_eq!(
            category_list(&document),
            expected.categories.map(<[&str]>::to_vec),
            "categories: {text}"
        );
    }
}

#[test]
fn normalize_authors_matches_python() {
    let call = Call::default();
    for (current, text, expected) in NORMALIZE_AUTHORS_CASES {
        let actual = normalize_authors(*current, text, &call).unwrap();
        assert_eq!(actual.as_deref(), *expected, "{current:?} + {text:?}");
    }
}

#[test]
fn normalize_authors_stops_on_a_cancelled_call() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    // The name is dropped as empty, so the fullest-form filter has nothing to check; only the
    // check per name can stop the call.
    assert_eq!(
        normalize_authors(None, "123", &call),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn normalize_json_matches_python() {
    for (text, expected) in NORMALIZE_JSON_CASES {
        assert_eq!(normalize_json(text).unwrap(), *expected, "{text:?}");
    }
}

#[test]
fn deep_author_string_raises() {
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    let (document, result) = run(&article_text(&deep), &Call::default());
    assert_eq!(result, Err(JsonFailure::Raised));
    assert_eq!(document.author, None);
}

#[test]
fn long_integer_author_string_raises() {
    let author = format!("[{}]", "1".repeat(5000));
    let (document, result) = run(&article_text(&author), &Call::default());
    assert_eq!(result, Err(JsonFailure::Raised));
    assert_eq!(document.author, None);
}

#[test]
fn overlong_charref_in_json_is_raised() {
    let author = format!("&#{};", "9".repeat(5000));
    let (_, result) = run(&article_text(&author), &Call::default());
    assert_eq!(result, Err(JsonFailure::Raised));
}

#[test]
fn unescape_error_is_raised_and_other_errors_stop() {
    assert_eq!(
        JsonFailure::from(ExtractError::Rejected(Rejection::InvalidValue)),
        JsonFailure::Raised
    );
    for error in [
        ExtractError::Cancelled,
        ExtractError::DeadlineExceeded,
        ExtractError::Rejected(Rejection::InputTooLarge),
    ] {
        assert_eq!(JsonFailure::from(error.clone()), JsonFailure::Stop(error));
    }
}

#[test]
fn cancelled_call_stops() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let (document, result) = run(&article_text("Jane Doe"), &call);
    assert_eq!(result, Err(JsonFailure::Stop(ExtractError::Cancelled)));
    assert_eq!(document.author, None);
}

#[test]
fn many_authors_stop_on_cancel() {
    let authors: Vec<String> = (0..20_000)
        .map(|n| format!(r#"{{"name":"Author Number{n}x"}}"#))
        .collect();
    let text = format!(
        r#"{{"@context":"https://schema.org","@type":"Article","author":[{}]}}"#,
        authors.join(",")
    );
    let token = CancelToken::new();
    let json = try_loads(&text, false, &Call::default()).unwrap().unwrap();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut document = Document::new();
    let result = extract_json(&json, json.root(), &mut document, &call);
    assert_eq!(result, Err(JsonFailure::Stop(ExtractError::Cancelled)));
    assert_eq!(document.author, None);
}

#[test]
fn fullest_form_filter_checks_the_call() {
    let current = (0..20_000)
        .map(|n| format!("Author Number{n}x"))
        .collect::<Vec<_>>()
        .join("; ");
    let started = Instant::now();
    let call = Call::default().deadline(started + Duration::from_millis(20));
    let result = normalize_authors(Some(&current), "123", &call);
    assert_eq!(result, Err(ExtractError::DeadlineExceeded));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn author_regexes_stop_on_cancel() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let text = r#"{"author":{"name":"Jane Doe"}}"#;
    let mut document = Document::new();
    assert_eq!(
        extract_json_parse_error(text, &mut document, &call),
        Err(ExtractError::Cancelled)
    );
    let regex = JSON_AUTHOR_1.as_ref().unwrap();
    assert_eq!(
        extract_json_author(text, regex, &call),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn flat_list_is_processed_once() {
    // extract_json and process_parent each tick at visited 0, once per call, so a list of two
    // objects checks two fewer times than the two objects alone. Each object's author checks add
    // the same on both sides: with names, the second object sees the first one's author and still
    // runs the fullest-form filter once. With empty names, no name is kept, metadata.author stays
    // None, and each object checks as it does alone. Measured: 8 against 5 + 5 with names, 6
    // against 4 + 4 with empty names.
    let article = |headline: &str| {
        format!(
            r#"{{"@context": "https://schema.org", "@type": "NewsArticle", "headline": "{headline}", "author": {{"name": "Jane Doe"}}}}"#
        )
    };
    let checks = |text: &str| {
        let trip = CheckTrip::counting();
        let json = try_loads(text, false, &Call::default()).unwrap().unwrap();
        let mut document = Document::new();
        extract_json(
            &json,
            json.root(),
            &mut document,
            &Call::default().trip(&trip),
        )
        .unwrap();
        trip.checks()
    };
    let (a, b) = (article("A"), article("B"));
    assert_eq!(
        checks(&format!("[{a}, {b}]")),
        checks(&format!("[{a}]")) + checks(&format!("[{b}]")) - 2
    );
}

#[test]
fn list_loops_stop_on_cancel() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let json = try_loads(
        r#"{"section":["A","B"],"names":["A","B"],"author":["x"]}"#,
        false,
        &Call::default(),
    )
    .unwrap()
    .unwrap();
    let get = |key: &str| json.get(json.root(), key).unwrap();
    let stop = JsonFailure::Stop(ExtractError::Cancelled);
    assert_eq!(
        categories_of(&json, get("section"), &call),
        Err(stop.clone())
    );
    assert_eq!(joined_names(&json, get("names"), &call), Err(stop.clone()));
    assert_eq!(author_name(&json, get("author"), &call), Err(stop));
}
