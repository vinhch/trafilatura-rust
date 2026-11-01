use super::cases::{
    FUZZ_ISOFORMAT_CASES, ISOFORMAT_CASES, NEW_CASES, STRFTIME_CASES, STRPTIME_YMD_CASES,
    TIMESTAMP_CASES,
};
use super::*;

#[test]
fn from_isoformat_matches_python() {
    for &(text, expected) in ISOFORMAT_CASES {
        assert_eq!(DateTime::from_isoformat(text), expected, "{text:?}");
    }
}

#[test]
fn from_isoformat_matches_python_on_fuzzed_strings() {
    for &(text, expected) in FUZZ_ISOFORMAT_CASES {
        assert_eq!(DateTime::from_isoformat(text), expected, "{text:?}");
    }
}

#[test]
fn strptime_ymd_matches_python() {
    for &(text, expected) in STRPTIME_YMD_CASES {
        assert_eq!(DateTime::strptime_ymd(text), expected, "{text:?}");
    }
}

#[test]
fn from_timestamp_utc_matches_python() {
    for &(seconds, expected) in TIMESTAMP_CASES {
        assert_eq!(DateTime::from_timestamp_utc(seconds), expected, "{seconds}");
    }
}

#[test]
fn strftime_matches_python() {
    for &(value, ymd, reference) in STRFTIME_CASES {
        assert_eq!(value.strftime_ymd(), ymd);
        assert_eq!(value.strftime_reference(), reference);
    }
}

#[test]
fn new_rejects_what_python_rejects() {
    for &(year, month, day, expected) in NEW_CASES {
        assert_eq!(
            DateTime::new(year, month, day),
            expected,
            "({year}, {month}, {day})"
        );
    }
}

#[test]
fn timestamp_reads_naive_as_utc() {
    let naive = DateTime::new(2020, 1, 1).expect("2020-01-01 is a date");
    // oracle: datetime(2020, 1, 1).timestamp() == 1577836800.0 under TZ=UTC (oracle_setup asserts it)
    assert_eq!(naive.timestamp_micros(), 1_577_836_800_000_000);
}

#[test]
fn timestamp_subtracts_the_offset() {
    let aware = DateTime::from_isoformat("2020-01-01T10:20:30+05:30").expect("a valid datetime");
    // oracle: datetime.fromisoformat("2020-01-01T10:20:30+05:30").timestamp() == 1577854230.0
    assert_eq!(aware.timestamp_micros(), 1_577_854_230_000_000);
}

#[test]
fn wall_clock_keeps_fields_and_drops_the_offset() {
    let value = DateTime::from_isoformat("2020-01-01T10:20:30.5+05:30").expect("a valid datetime");
    assert_eq!(value.wall_clock(), (2020, 1, 1, 10, 20, 30, 500_000));
}
