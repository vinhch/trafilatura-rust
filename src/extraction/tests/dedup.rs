//! Duplicate counts of whole extractions: the script-text rerun, the entry points that commit, and
//! caches shared across calls and threads.

use std::thread;

use super::*;
use crate::deduplication::DedupCache;

/// The paragraph the duplicate counts are read under: trimmed, as `duplicate_test` keys it.
fn long_paragraph() -> String {
    "Form body sentence with several words in it, long enough for the duplicate test. ".repeat(2)
}

/// A page whose text readers reach dropped script text, holding `paragraph` twice. Its paragraphs
/// all sit in a `nav`, which recall mode cleans away, so `tree_cleaning` restores its backup and the
/// script survives: the extraction takes the script-text rerun of `bare_extraction`, and the
/// restored paragraphs reach the output.
fn rerun_page(paragraph: &str) -> String {
    format!(
        "<html><head><title>t</title></head><body><form><nav><p>{paragraph}</p><p>{paragraph}</p></nav>\
         <div>{}</div></form><script>{}</script></body></html>",
        "Form body sentence with several words in it. ".repeat(8),
        "var a = 1;".repeat(200),
    )
}

#[test]
fn rerun_leaves_counts_of_one_pass() {
    let paragraph = long_paragraph();
    let key = paragraph.trim();
    let page = rerun_page(&paragraph);
    let options = cascade_options(false, true, false);

    let cache = DedupCache::new(64).expect("maxsize 64 is valid");
    let with_rerun = bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &options,
        &Call::default().dedup(&cache),
        &ParseLimits {
            drop_script_text: true,
            ..ParseLimits::default()
        },
    )
    .expect("the rerun page extracts");

    // the same page, one pass with the script text kept, into a fresh cache
    let once_cache = DedupCache::new(64).expect("maxsize 64 is valid");
    let once = bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &options,
        &Call::default().dedup(&once_cache),
        &ParseLimits {
            drop_script_text: false,
            ..ParseLimits::default()
        },
    )
    .expect("the page extracts");

    assert_eq!(with_rerun.raw_text, once.raw_text);
    assert!(
        with_rerun
            .raw_text
            .as_deref()
            .is_some_and(|raw| raw.contains(key))
    );
    assert!(once_cache.get(key).is_some(), "the paragraph was counted");
    assert_eq!(cache.get(key), once_cache.get(key));
}

#[test]
fn tree_source_commits() {
    let page = main_page("");
    let options = bare_options(Format::Txt, false);

    let plain = DedupCache::new(64).expect("maxsize 64 is valid");
    bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &options,
        &Call::default().dedup(&plain),
        &ParseLimits::default(),
    )
    .expect("the page extracts");

    let tree = load(&page);
    let from_tree = DedupCache::new(64).expect("maxsize 64 is valid");
    bare_with(
        Source::Tree(&tree, tree.root()),
        &options,
        &Call::default().dedup(&from_tree),
        &ParseLimits::default(),
    )
    .expect("the tree extracts");

    assert!(
        plain.get(&main_text()).is_some(),
        "the paragraph was counted"
    );
    assert_eq!(from_tree.get(&main_text()), plain.get(&main_text()));
    assert_eq!(from_tree.len(), plain.len());
}

#[test]
fn metadata_call_commits() {
    let page = main_page("");
    let plain_options = bare_options(Format::Txt, false);
    let mut metadata_options = bare_options(Format::Txt, false);
    metadata_options.with_metadata = true;

    let plain = DedupCache::new(64).expect("maxsize 64 is valid");
    bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &plain_options,
        &Call::default().dedup(&plain),
        &ParseLimits::default(),
    )
    .expect("the page extracts");

    let with_metadata = DedupCache::new(64).expect("maxsize 64 is valid");
    bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &metadata_options,
        &Call::default().dedup(&with_metadata),
        &ParseLimits::default(),
    )
    .expect("the page extracts with metadata");

    assert!(
        plain.get(&main_text()).is_some(),
        "the paragraph was counted"
    );
    assert_eq!(with_metadata.get(&main_text()), plain.get(&main_text()));
    assert_eq!(with_metadata.len(), plain.len());
}

#[test]
fn small_cache_many_texts() {
    let cache = DedupCache::new(1).expect("maxsize 1 is valid");
    let body: String = (0..30)
        .map(|i| format!("<p>{}</p>", prose(&format!("Topic{i}"), 2)))
        .collect();
    let page = full_page(&body, "");

    let doc = bare_with(
        Source::Input(HtmlInput::Str(&page)),
        &bare_options(Format::Txt, false),
        &Call::default().dedup(&cache),
        &ParseLimits::default(),
    );

    assert!(doc.is_ok(), "{doc:?}");
    assert_eq!(cache.len(), 1);
}

#[test]
fn shared_cache_two_threads() {
    let cache = DedupCache::new(4).expect("maxsize 4 is valid");
    let page = main_page("");
    let options = bare_options(Format::Txt, false);

    thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                for _ in 0..20 {
                    let result = bare_with(
                        Source::Input(HtmlInput::Str(&page)),
                        &options,
                        &Call::default().dedup(&cache),
                        &ParseLimits::default(),
                    );
                    assert!(
                        matches!(
                            result,
                            Ok(_) | Err(ExtractError::Rejected(Rejection::Duplicate))
                        ),
                        "{result:?}"
                    );
                }
            });
        }
    });

    assert!(cache.len() <= cache.maxsize());
}

#[test]
fn call_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Call<'static>>();
}
