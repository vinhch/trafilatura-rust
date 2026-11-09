use super::*;
use encoding_rs::{WINDOWS_1251, WINDOWS_1252};

use crate::settings::{DEFAULT_MAX_INPUT_BYTES, MAX_FILE_SIZE};
use crate::utils::text::{
    IMAGE_EXTENSION, RE_FILTER, RE_HTML_LANG, is_image_file, return_printables_and_spaces,
};

const SIZE: usize = MAX_FILE_SIZE;

fn load_bytes(bytes: &[u8]) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn capped(max: usize) -> ParseLimits {
    ParseLimits {
        max_input_bytes: Some(max),
        ..ParseLimits::default()
    }
}

#[test]
fn as_list_matches_python() {
    let text = r#"{"o":{},"s":"x","l":[1,"a"],"e":[],"z":null,"n":5,"b":true}"#;
    let json = crate::pyjson::loads(text, true, &Call::default())
        .unwrap()
        .unwrap();
    let root = json.root();
    let get = |key: &str| json.get(root, key);
    let items = |key: &str| json.items(get(key).unwrap()).collect::<Vec<_>>();
    // oracle: as_list(None) gives []
    assert_eq!(as_list(&json, None), []);
    // oracle: as_list([1, 'a']) gives the list, as_list([]) gives []
    assert_eq!(as_list(&json, get("l")), items("l"));
    assert_eq!(as_list(&json, get("e")), []);
    // oracle: as_list('x') gives ['x'], as_list({}) gives [{}], as_list(5) gives [5], as_list(True) gives [True]
    for key in ["s", "o", "n", "b"] {
        assert_eq!(as_list(&json, get(key)), [get(key).unwrap()], "{key}");
    }
    // oracle: as_list(json.loads('null')) gives []
    assert_eq!(as_list(&json, get("z")), []);
}

#[test]
fn normalize_unicode_is_nfc() {
    // oracle: normalize_unicode("e" + chr(0x301) + " " + chr(0x212b)), normalize_unicode("\xe9")
    assert_eq!(normalize_unicode("e\u{301} \u{212b}"), "\u{e9} \u{c5}");
    assert!(matches!(normalize_unicode("\u{e9}"), Cow::Borrowed(_)));
}

mod binary;
#[cfg(feature = "decompress")]
mod decompress;
mod input;

// The text helpers. Expected values come from upstream Python (`scripts/oracle.sh`); the oracle
// comment names the expression.
mod text;
