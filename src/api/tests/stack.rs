//! The stack contract: `extract`, `bare_extraction` and `html2txt` fit a 512 KiB thread in a debug
//! build on input nested past the depth bound, one shape at a time, so every recursive walk the
//! pipeline reaches runs at its bound. The recall pages put a frame around the shape that triggers
//! the script-text rerun.

use super::*;
use crate::cancel::Call;
use crate::dom::{MAX_DEPTH, ParseLimits, parse_document};
use crate::settings::Focus;

/// The smallest stack a host may give a thread; the contract's bound.
const STACK_BYTES: usize = 512 * 1024;

/// Past the depth the parser keeps, so the walks run at their bound.
const LEVELS: usize = MAX_DEPTH + 16;

/// The formats that walk the tree on the way out.
const TREE_FORMATS: [Format; 7] = [
    Format::Txt,
    Format::Markdown,
    Format::Xml,
    Format::XmlTei,
    Format::Json,
    Format::Csv,
    Format::Html,
];

/// One recursive shape of the input. Each level carries text.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Div,
    /// `ul > li`: nested lists.
    List,
    Blockquote,
    /// `table > tr > td`: nested tables in cells.
    Table,
    /// `b` and `i` alternate: inline emphasis, which becomes `hi`.
    Emphasis,
    /// `b` and `a` alternate: a link inside emphasis inside a link.
    Reference,
}

const SHAPES: [Shape; 6] = [
    Shape::Div,
    Shape::List,
    Shape::Blockquote,
    Shape::Table,
    Shape::Emphasis,
    Shape::Reference,
];

impl Shape {
    /// The open tag of `level` and the close tag that matches it.
    fn tags(self, level: usize) -> (String, &'static str) {
        match self {
            Shape::Div => ("<div>".to_owned(), "</div>"),
            Shape::List => ("<ul><li>".to_owned(), "</li></ul>"),
            Shape::Blockquote => ("<blockquote>".to_owned(), "</blockquote>"),
            Shape::Table => ("<table><tr><td>".to_owned(), "</td></tr></table>"),
            Shape::Emphasis if level.is_multiple_of(2) => ("<b>".to_owned(), "</b>"),
            Shape::Emphasis => ("<i>".to_owned(), "</i>"),
            Shape::Reference if level.is_multiple_of(2) => ("<b>".to_owned(), "</b>"),
            Shape::Reference => (format!("<a href=\"https://example.org/{level}\">"), "</a>"),
        }
    }
}

/// `levels` nested copies of `shape`, each with its own sentence of text, closed innermost first.
fn nest(shape: Shape, levels: usize) -> String {
    let mut html = String::new();
    let mut closing = Vec::with_capacity(levels);
    for level in 0..levels {
        let (open, close) = shape.tags(level);
        html.push_str(&open);
        html.push_str(&format!(" Level {level} keeps this sentence of prose. "));
        closing.push(close);
    }
    for close in closing.iter().rev() {
        html.push_str(close);
    }
    html
}

/// One input page and the options its extraction runs with.
struct Page {
    name: String,
    html: String,
    /// The recall frame: `Focus::Recall` and the default `drop_script_text`.
    recall: bool,
}

impl Page {
    /// The options of this page for `format`. With metadata off, `drop_script_text` is on, which
    /// the recall frame needs for its script-text rerun (`api.rs`, `Options::build`).
    fn extractor(&self, format: Format) -> Extractor {
        Options::new()
            .format(format)
            .focus(if self.recall {
                Focus::Recall
            } else {
                Focus::Balanced
            })
            .with_metadata(!self.recall)
            .formatting(true)
            .links(true)
            .tables(true)
            .images(true)
            .build()
    }
}

/// Each shape as a plain page, and the same shape inside the `recall_page` frame of
/// `tests/measure/mod.rs`: a form, an aside, a `div` before the shape, and a bare `script` after it.
fn pages() -> Vec<Page> {
    let mut pages = Vec::new();
    for shape in SHAPES {
        pages.push(Page {
            name: format!("{shape:?}"),
            html: format!(
                "<html><head><title>Deep page</title></head><body>{}</body></html>",
                nest(shape, LEVELS)
            ),
            recall: false,
        });
        let html = format!(
            "<html><head><title>Deep page</title></head><body>\
             <form><aside><p>Short aside paragraph here.</p></aside>\
             <div>{}</div></form><div>{}</div><script>{}</script><div>{}</div></body></html>",
            "Form body sentence with several words in it. ".repeat(8),
            "Outside text that is part of the page as well. ".repeat(3),
            "var a = 1;".repeat(4),
            nest(shape, LEVELS),
        );
        // the frame's script is not a type upstream reads, so the parse drops its text; it comes
        // before the shape, because the parser stops at the depth bound and reads nothing past it
        let limits = ParseLimits {
            drop_script_text: true,
            ..ParseLimits::default()
        };
        let tree = parse_document(&html, &limits, &Call::default()).unwrap();
        assert!(
            tree.dropped_script_text,
            "{shape:?} in the recall frame: the parse dropped no script text"
        );
        pages.push(Page {
            name: format!("{shape:?} in the recall frame"),
            html,
            recall: true,
        });
    }
    pages
}

/// Runs `check` on a thread with the contract's stack, and rethrows what it panicked with.
fn on_small_stack(check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(check)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

#[test]
fn extract_deep_html_fits_512k_stack() {
    on_small_stack(|| {
        for page in pages() {
            for format in TREE_FORMATS {
                let text = page
                    .extractor(format)
                    .extract(&Input::text(&page.html), &Call::default())
                    .unwrap_or_else(|error| panic!("{}, {format:?}: {error:?}", page.name));
                assert!(!text.is_empty(), "{}, {format:?}: empty output", page.name);
            }
        }
    });
}

#[test]
fn bare_extraction_deep_html_fits_512k_stack() {
    on_small_stack(|| {
        for page in pages() {
            let document = page
                .extractor(Format::Python)
                .bare_extraction(&Input::text(&page.html), &Call::default())
                .unwrap_or_else(|error| panic!("{}: {error:?}", page.name));
            assert!(
                document
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.is_empty()),
                "{}: empty text",
                page.name
            );
        }
    });
}

#[test]
fn html2txt_deep_html_fits_512k_stack() {
    on_small_stack(|| {
        for page in pages() {
            for clean in [true, false] {
                let text = html2txt(&Input::text(&page.html), clean, &Call::default())
                    .unwrap_or_else(|error| panic!("{}, clean {clean}: {error:?}", page.name));
                assert!(!text.is_empty(), "{}, clean {clean}: empty text", page.name);
            }
        }
    });
}
