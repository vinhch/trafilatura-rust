//! Tests of the htmldate port: settings, utils, validators, extractors, the page-text search, the tree examiners
//! and `find_date`. Expected values in the generated `*_cases.rs` tables come from `scripts/oracle.sh`; a
//! `// oracle:` comment names the probe that gave a hand-written value.

use std::sync::LazyLock;

use regex::Regex;

use super::core::{
    date_candidates, examine_abbr_elements, examine_elements, examine_header, examine_text,
    examine_time_elements, find_date, search_page, title_h1_elements,
};
use super::examine_cases::{EXAMINE_CASES, Expected};
use super::extractor_cases::{
    EXTRACT_URL_DATE_CASES, IDIOSYNCRASIES_CASES, PATTERN_SEARCH_CASES, REGEX_PARSE_CASES,
    TRY_DATE_EXPR_CASES,
};
use super::extractors::{
    JSON_MODIFIED, JSON_PUBLISHED, discard_unwanted, external_date, extract_url_date,
    free_text_expressions, idiosyncrasies_search, img_search, json_search, pattern_search,
    regex_parse, try_date_expr,
};
use super::find_date_cases::FIND_DATE_CASES;
use super::search_cases::{
    EXAMINE_TEXT_CASES, SEARCH_PAGE_CASES, SEARCH_PAGE_MAX_2045_CASES, W3_RUN_CASE,
};
use super::settings::{CLEANING_LIST, MIN_DATE};
use super::utils::{Extractor, NotAfter, Pattern, clean_html, remove_if_attached, trim_text};
use super::validator_cases::{
    CHECK_DATE_INPUT_CASES, COMPARE_VALUES_CASES, FILTER_YMD_CASES, IS_VALID_DATE_STR_CASES,
};
use super::validators::{
    Format, check_date_input, compare_values, correct_year, filter_ymd_candidate, get_max_date,
    is_valid_date_str, plausible_year_filter, validate_and_convert,
};
use crate::cancel::{Call, CancelToken};
use crate::dom::{ParseLimits, Tree, parse_document};
use crate::error::ExtractError;
use crate::pydatetime::DateTime;
use crate::settings::DateParams;

/// `max_date` "2024-01-31", midnight, naive.
const LATEST: DateTime = DateTime {
    year: 2024,
    month: 1,
    day: 31,
    hour: 0,
    minute: 0,
    second: 0,
    microsecond: 0,
    offset: None,
};

/// The default of the date-input check in the generated table.
const DEFAULT: DateTime = DateTime {
    year: 2000,
    month: 1,
    day: 1,
    hour: 0,
    minute: 0,
    second: 0,
    microsecond: 0,
    offset: None,
};

fn parse(html: &str) -> Tree {
    parse_document(html, &ParseLimits::default(), &Call::default()).expect("the page parses")
}

#[test]
fn is_valid_date_str_matches_python() {
    for &(text, expected) in IS_VALID_DATE_STR_CASES {
        assert_eq!(
            is_valid_date_str(text, &MIN_DATE, &LATEST),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn compare_values_matches_python() {
    for &(reference, attempt, original, expected) in COMPARE_VALUES_CASES {
        let options = Extractor {
            extensive: false,
            max: LATEST,
            min: MIN_DATE,
            original,
        };
        assert_eq!(
            compare_values(reference, attempt, &options),
            expected,
            "{attempt:?} original {original}"
        );
    }
}

#[test]
fn filter_ymd_candidate_matches_python() {
    for &(bestmatch, copyear, expected) in FILTER_YMD_CASES {
        let groups = bestmatch.map(|groups| groups.map(Some));
        assert_eq!(
            filter_ymd_candidate(groups, copyear, &MIN_DATE, &LATEST),
            Ok(expected.map(str::to_owned)),
            "{bestmatch:?} copyear {copyear}"
        );
    }
}

#[test]
fn check_date_input_matches_python() {
    for &(text, expected) in CHECK_DATE_INPUT_CASES {
        assert_eq!(check_date_input(text, DEFAULT), expected, "{text:?}");
    }
}

#[test]
fn trim_text_matches_python() {
    // oracle: trim_text("\x1c a  b\n") is "a b"
    assert_eq!(trim_text("\x1c a  b\n"), "a b");
}

#[test]
fn correct_year_matches_python() {
    // oracle: correct_year(89), (90), (99), (100)
    for (year, expected) in [(89, 2089), (90, 1990), (99, 1999), (100, 100)] {
        assert_eq!(correct_year(year), expected, "{year}");
    }
}

#[test]
fn get_max_date_default_is_end_of_utc_day() {
    for input in [None, Some("garbage"), Some("")] {
        let max = get_max_date(input);
        assert_eq!(
            (
                max.hour,
                max.minute,
                max.second,
                max.microsecond,
                max.offset
            ),
            (23, 59, 59, 999_999, None),
            "{input:?}"
        );
        assert!(max.year >= 2026, "{max:?}");
    }
}

#[test]
fn get_max_date_keeps_aware_bound() {
    assert_eq!(
        get_max_date(Some("2024-01-31T10:00+02:00")),
        DateTime {
            year: 2024,
            month: 1,
            day: 31,
            hour: 10,
            minute: 0,
            second: 0,
            microsecond: 0,
            offset: Some(7_200_000_000),
        }
    );
    assert_eq!(get_max_date(Some("2024-01-31")), LATEST);
}

#[test]
fn clean_html_stops_like_lxml() {
    // oracle: clean_html(tree, CLEANING_LIST), then lxml.html.tostring of the root
    let cases = [
        (
            "<object><audio></audio></object><video>v</video>",
            "<html><head></head><body><video>v</video></body></html>",
        ),
        (
            "<object><svg></svg></object><video>v</video>",
            "<html><head></head><body></body></html>",
        ),
    ];
    for (page, expected) in cases {
        let mut tree = parse(page);
        let root = tree.root();
        clean_html(&mut tree, root, CLEANING_LIST, &Call::default()).expect("not cancelled");
        assert_eq!(tree.tostring_html(root, true), expected, "page {page:?}");
    }
}

#[test]
fn plausible_year_filter_keeps_first_occurrence_order() {
    // oracle: plausible_year_filter with these patterns and bounds 1995-01-01 and 2024-01-31 gives
    // [("2021-01-01", 2), ("2020-05-05", 3)]: the count order would put 2020-05-05 first
    static DATE: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(\d{4}-\d{2}-\d{2})").ok());
    static YEAR: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(\d{4})").ok());
    let pattern = Pattern {
        regex: &DATE,
        not_after: None,
    };
    let yearpat = Pattern {
        regex: &YEAR,
        not_after: None,
    };
    let text = "2021-01-01 2020-05-05 2021-01-01 1990-01-01 2050-01-01 2020-05-05 2020-05-05";
    let counts = plausible_year_filter(
        text,
        &pattern,
        &yearpat,
        &MIN_DATE,
        &LATEST,
        &Call::default(),
    )
    .expect("not cancelled");
    assert_eq!(
        counts.iter().collect::<Vec<_>>(),
        [("2021-01-01", 2), ("2020-05-05", 3)]
    );
}

#[test]
fn pattern_not_after_retries_one_code_point_later() {
    // oracle: re.findall(r"(?<!w3.org)(\d+)", "w3.org2020 x2020 w3.org2021") is these five values
    static DIGITS: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(\d+)").ok());
    static W3_ORG: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\A(?:w3.org)\z").ok());
    let pattern = Pattern {
        regex: &DIGITS,
        not_after: Some(NotAfter {
            anchored: &W3_ORG,
            width: 6,
        }),
    };
    let found = pattern
        .findall("w3.org2020 x2020 w3.org2021", &Call::default())
        .expect("not cancelled");
    assert_eq!(found, ["3", "020", "2020", "3", "021"]);
}

#[test]
fn remove_if_attached_drops_the_tail() {
    // oracle: lxml's parent.remove(p) on <div><p>a</p>TAIL<span>s</span></div>, the html root serialized
    let mut tree = parse("<div><p>a</p>TAIL<span>s</span></div>");
    let root = tree.root();
    let p = tree.iter(root, &["p"]).next().expect("a p");
    remove_if_attached(&mut tree, p);
    assert_eq!(
        tree.tostring_html(root, true),
        "<html><head></head><body><div><span>s</span></div></body></html>"
    );
}

#[test]
fn validate_and_convert_matches_python() {
    // oracle: validate_and_convert with the bounds 1995-01-01 and 2024-01-31
    let inside = DateTime {
        year: 2020,
        month: 1,
        day: 15,
        hour: 10,
        minute: 20,
        second: 30,
        microsecond: 0,
        offset: None,
    };
    assert_eq!(
        validate_and_convert(Some(&inside), Format::Ymd, &MIN_DATE, &LATEST),
        Some("2020-01-15".to_owned())
    );
    let outside = DateTime {
        year: 2025,
        ..inside
    };
    assert_eq!(
        validate_and_convert(Some(&outside), Format::Ymd, &MIN_DATE, &LATEST),
        None
    );
    let aware = DateTime {
        microsecond: 7,
        offset: Some(0),
        ..inside
    };
    assert_eq!(
        validate_and_convert(Some(&aware), Format::Reference, &MIN_DATE, &LATEST),
        Some("2020-01-15T10:20:30.000007+0000".to_owned())
    );
}

/// The options of the string extractors: the bounds of the generated tables, no output format.
fn search_options() -> Extractor {
    Extractor {
        extensive: false,
        max: LATEST,
        min: MIN_DATE,
        original: false,
    }
}

#[test]
fn try_date_expr_matches_python() {
    for &(text, format, extensive, expected) in TRY_DATE_EXPR_CASES {
        assert_eq!(
            try_date_expr(
                Some(text),
                format,
                extensive,
                &MIN_DATE,
                &LATEST,
                &Call::default()
            ),
            Ok(expected.map(str::to_owned)),
            "{text:?} {format:?} extensive {extensive}"
        );
    }
}

#[test]
fn regex_parse_matches_python() {
    for &(text, expected) in REGEX_PARSE_CASES {
        assert_eq!(
            regex_parse(text, &Call::default()),
            Ok(expected),
            "{text:?}"
        );
    }
}

#[test]
fn extract_url_date_matches_python() {
    for &(testurl, expected) in EXTRACT_URL_DATE_CASES {
        assert_eq!(
            extract_url_date(testurl, &search_options()).as_deref(),
            expected,
            "{testurl:?}"
        );
    }
}

#[test]
fn idiosyncrasies_search_matches_python() {
    for (index, &(text, expected)) in IDIOSYNCRASIES_CASES.iter().enumerate() {
        assert_eq!(
            idiosyncrasies_search(text, &search_options(), &Call::default()),
            Ok(expected.map(str::to_owned)),
            "case {index}"
        );
    }
}

#[test]
fn pattern_search_matches_python() {
    for &(text, published, expected) in PATTERN_SEARCH_CASES {
        let pattern = Pattern {
            regex: if published {
                &JSON_PUBLISHED
            } else {
                &JSON_MODIFIED
            },
            not_after: None,
        };
        assert_eq!(
            pattern_search(text, &pattern, &search_options(), &Call::default()),
            Ok(expected.map(str::to_owned)),
            "{text:?} published {published}"
        );
    }
}

#[test]
fn regex_parse_long_letter_run_is_linear() {
    // oracle: regex_parse("a" * 200000 + " 5, 2020") is None, and regex_parse("a" * 200000 + "March 5, 2020")
    // is datetime(2020, 3, 5): the month word is the longest known suffix of the run
    let run = "a".repeat(200_000);
    assert_eq!(
        regex_parse(&format!("{run} 5, 2020"), &Call::default()),
        Ok(None)
    );
    assert_eq!(
        regex_parse(&format!("{run}March 5, 2020"), &Call::default()),
        Ok(DateTime::new(2020, 3, 5))
    );
}

#[test]
fn idiosyncrasies_core_cap() {
    // oracle: idiosyncrasies_search("01.09.2017 " * 1500 + "date: 02.09.2017") is "2017-09-02": past the
    // 1,000-core cap the search runs unbounded; "01.09.2017 " * 1500 with no cue is None
    let cores = "01.09.2017 ".repeat(1500);
    assert_eq!(
        idiosyncrasies_search(
            &format!("{cores}date: 02.09.2017"),
            &search_options(),
            &Call::default()
        ),
        Ok(Some("2017-09-02".to_owned()))
    );
    assert_eq!(
        idiosyncrasies_search(&cores, &search_options(), &Call::default()),
        Ok(None)
    );
}

#[test]
fn external_date_gives_none() {
    // D-13: dateparser is not ported, so no string gives a date from the external parser
    assert_eq!(external_date("15 de marzo de 2017"), None);
    assert_eq!(external_date("March 5, 2020"), None);
}

#[test]
fn cancelled_call_stops_regex_parse() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let text = "Foo 5, 2020 ".repeat(1000);
    assert_eq!(regex_parse(&text, &call), Err(ExtractError::Cancelled));
}

#[test]
fn examine_text_matches_python() {
    for &(text, expected) in EXAMINE_TEXT_CASES {
        assert_eq!(
            examine_text(text, &search_options(), &Call::default()),
            Ok(expected.map(str::to_owned)),
            "{text:?}"
        );
    }
}

#[test]
fn search_page_matches_python() {
    for &(htmlstring, original, expected) in SEARCH_PAGE_CASES {
        let options = Extractor {
            original,
            ..search_options()
        };
        assert_eq!(
            search_page(htmlstring, &options, &Call::default()),
            Ok(expected.map(str::to_owned)),
            "{htmlstring:?} original {original}"
        );
    }
}

#[test]
fn search_page_max_2045_matches_python() {
    let max = DateTime::new(2045, 12, 31).expect("a valid date");
    for &(htmlstring, original, expected) in SEARCH_PAGE_MAX_2045_CASES {
        let options = Extractor {
            max,
            original,
            ..search_options()
        };
        assert_eq!(
            search_page(htmlstring, &options, &Call::default()),
            Ok(expected.map(str::to_owned)),
            "{htmlstring:?} original {original}"
        );
    }
}

#[test]
fn simple_pattern_w3_run_is_linear() {
    // Each "w3.org/2020/" is rejected by the lookbehind and retried one code point later; the search then
    // returns the year after the run.
    let page = "w3.org/2020/ ".repeat(20_000) + " 2021 ";
    assert_eq!(
        search_page(&page, &search_options(), &Call::default()),
        Ok(W3_RUN_CASE.map(str::to_owned))
    );
}

#[test]
fn cancelled_call_stops_search_page() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert_eq!(
        search_page("<p>May 12, 2019</p>", &search_options(), &call),
        Err(ExtractError::Cancelled)
    );
}

/// The date the examiner `function` returns on `tree`, as `examine_cases` names it.
fn examine_date(
    function: &str,
    tree: &Tree,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let root = tree.root();
    match function {
        "header" => examine_header(tree, root, options, call),
        "json" => json_search(tree, root, options, call),
        "abbr" => examine_abbr_elements(tree, root, options, call),
        "time" => examine_time_elements(tree, root, options, call),
        "elements" => {
            let candidates = date_candidates(tree, root, options.extensive, call)?;
            examine_elements(tree, &candidates, options, call)
        }
        "title_h1" => {
            let elements = title_h1_elements(tree, root, call)?;
            examine_elements(tree, &elements, options, call)
        }
        "img" => img_search(tree, root, options, call),
        other => panic!("no examiner named {other}"),
    }
}

#[test]
fn examine_cases_match_python() {
    for (index, case) in EXAMINE_CASES.iter().enumerate() {
        let tree = parse(case.html);
        let options = Extractor {
            extensive: case.extensive,
            original: case.original,
            ..search_options()
        };
        let context = format!(
            "case {index} {} original {} extensive {}",
            case.function, case.original, case.extensive
        );
        match &case.result {
            Expected::Texts(expected) => {
                let got = free_text_expressions(&tree, tree.root(), &Call::default())
                    .expect("not cancelled");
                assert_eq!(got.as_slice(), *expected, "{context}");
            }
            Expected::Date(expected) => {
                let got = examine_date(case.function, &tree, &options, &Call::default())
                    .expect("not cancelled");
                assert_eq!(got.as_deref(), *expected, "{context}");
            }
        }
    }
}

#[test]
fn discard_unwanted_drops_tail() {
    // oracle: scripts/oracle.sh 'from parity import html5tree; from htmldate.extractors import discard_unwanted;
    // from lxml.html import tostring; t = html5tree.parse_document(<page>); discard_unwanted(t);
    // print(tostring(t, encoding="unicode"))', with <page> the div below: the wm-ipp div and its tail "TAIL" go
    let mut tree = parse(r#"<div><div id="wm-ipp">x</div>TAIL<p>a</p></div>"#);
    let root = tree.root();
    discard_unwanted(&mut tree, root, &Call::default()).expect("not cancelled");
    assert_eq!(
        tree.tostring_html(root, true),
        "<html><head></head><body><div><p>a</p></div></body></html>"
    );
}

#[test]
fn cancelled_call_stops_examine_header() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let tree = parse(r#"<html><head><meta name="date" content="2020-01-15"></head></html>"#);
    assert_eq!(
        examine_header(&tree, tree.root(), &search_options(), &call),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn find_date_matches_python() {
    for &(html, url, original, extensive, expected) in FIND_DATE_CASES {
        let tree = parse(html);
        let params = DateParams {
            original_date: original,
            extensive_search: extensive,
            max_date: Some("2024-01-31".to_owned()),
        };
        assert_eq!(
            find_date(&tree, tree.root(), &params, url, &Call::default()),
            Ok(expected.map(str::to_owned)),
            "{html:?} url {url:?} original {original} extensive {extensive}"
        );
    }
}

#[test]
fn cancelled_call_stops_find_date() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let tree = parse("<html><body><p>May 12, 2019</p></body></html>");
    let params = DateParams {
        original_date: false,
        extensive_search: true,
        max_date: None,
    };
    assert_eq!(
        find_date(&tree, tree.root(), &params, None, &call),
        Err(ExtractError::Cancelled)
    );
}
