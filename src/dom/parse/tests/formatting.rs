use std::collections::HashSet;

use html5ever::tendril::TendrilSink;

use super::*;
use crate::dom::{DumpMode, dump};

// The 14 names html5ever 0.40.1 keeps in its list of active formatting elements (`rules.rs`).
const FORMATTING_NAMES: [&str; 14] = [
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
];

// The element names of the HTML Standard, obsolete ones included, plus the legacy `image` and
// `isindex` html5ever's tree builder special-cases. The test covers the names listed here only:
// a name missing from this table is not checked.
#[rustfmt::skip]
const HTML_ELEMENT_NAMES: &[&str] = &[
    "a", "abbr", "acronym", "address", "applet", "area", "article", "aside", "audio", "b",
    "base", "basefont", "bdi", "bdo", "bgsound", "big", "blink", "blockquote", "body", "br",
    "button", "canvas", "caption", "center", "cite", "code", "col", "colgroup", "command",
    "data", "datalist", "dd", "del", "details", "dfn", "dialog", "dir", "div", "dl", "dt", "em",
    "embed", "fencedframe", "fieldset", "figcaption", "figure", "font", "footer", "form",
    "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr",
    "html", "i", "iframe", "image", "img", "input", "ins", "isindex", "kbd", "keygen", "label",
    "legend", "li", "link", "listing", "main", "map", "mark", "marquee", "math", "menu",
    "menuitem", "meta", "meter", "multicol", "nav", "nextid", "nobr", "noembed", "noframes",
    "noscript", "object", "ol", "optgroup", "option", "output", "p", "param", "picture",
    "plaintext", "pre", "progress", "q", "rb", "rp", "rt", "rtc", "ruby", "s", "samp", "script",
    "search", "section", "select", "selectedcontent", "slot", "small", "source", "spacer",
    "span", "strike", "strong", "style", "sub", "summary", "sup", "svg", "table", "tbody", "td",
    "template", "textarea", "tfoot", "th", "thead", "time", "title", "tr", "track", "tt", "u",
    "ul", "var", "video", "wbr", "xmp",
];

#[test]
fn is_formatting_is_exactly_html5evers_list() {
    let html = |name: &str| QualName::new(None, ns!(html), LocalName::from(name));
    for name in FORMATTING_NAMES {
        assert!(is_formatting(&html(name)), "{name}");
    }
    for name in HTML_ELEMENT_NAMES {
        assert_eq!(
            is_formatting(&html(name)),
            FORMATTING_NAMES.contains(name),
            "{name}"
        );
    }
    for name in ["strike2", "bb", ""] {
        assert!(!is_formatting(&html(name)), "{name}");
    }
    let svg_a = QualName::new(None, ns!(svg), local_name!("a"));
    assert!(!is_formatting(&svg_a));
}

// The compare is charged per formatting start tag, by name: a name missing from `is_formatting`
// would go uncharged. `a` and `nobr` start tags make html5ever close an open element of the same
// name first, so an `applet` marker keeps the heavy one open for them.
#[test]
fn every_formatting_start_tag_is_charged_a_compare() {
    for name in FORMATTING_NAMES {
        let open = format!("<html><body>{}", heavy_tag(name, 0));
        let (_, base) = compare_outcome(&open, 500_000);
        let (_, plain) = compare_outcome(&format!("{open}<{name}>"), 500_000);
        let (_, marked) = compare_outcome(&format!("{open}<applet><{name}>"), 500_000);
        let (_, applet) = compare_outcome(&format!("{open}<applet>"), 500_000);
        assert!(
            marked > applet,
            "{name}: {marked} with the tag, {applet} without"
        );
        if name == "a" || name == "nobr" {
            assert_eq!(plain, base, "{name} closes the open one first");
        } else {
            assert!(plain > base, "{name}: {plain} with the tag, {base} without");
        }
    }
}

// A formatting start tag whose own element the depth cut leaves unplaced is charged nothing. The
// page: a heavy `b`, a `b` that `</p>` leaves only in the active formatting list, `MAX_DEPTH - 4`
// nested `div`s, then `<b>`. Reconstruction clones the listed `b` under the last `div`, at
// `MAX_DEPTH`, and the tag's own `b` has no room. The clone sits under the heavy `b`, so charging
// it in the tag's place would cost as much again and could reject a page that is only too deep.
#[test]
fn unplaced_formatting_element_is_charged_nothing() {
    let prefix = format!(
        "<html><body>{}<p><b></p>{}",
        heavy_tag("b", 0),
        "<div>".repeat(MAX_DEPTH - 4)
    );
    let html = format!("{prefix}<b>");
    let (_, before) = compare_outcome(&prefix, 500_000);
    // The budget covers the prefix and is below the prefix plus a second charge of the same size.
    let max_nodes = before.div_ceil(COMPARE_WORK_PER_NODE);
    assert!(before > 2 * PAIR_UNITS && max_nodes * COMPARE_WORK_PER_NODE < 2 * before);

    let call = Call::default();
    let driver = Driver::new(new_sink(&limited(max_nodes), &call, false), false);
    driver.process(&html);
    {
        let inner = driver.sink().inner.borrow();
        let b = local_name!("b");
        let unplaced = |node: &&SinkNode| {
            node.kind == Kind::Element && node.qual.local == b && node.parent.is_none()
        };
        assert!(inner.stopped, "the depth cut stops the parse");
        assert_eq!(inner.nodes.iter().filter(unplaced).count(), 1);
        assert_eq!(inner.abort, None);
        assert_eq!(inner.compare_work, before);
        let deepest = (0..inner.nodes.len())
            .filter_map(|index| u32::try_from(index).ok())
            .filter(|&id| inner.node(id).kind == Kind::Element && inner.node(id).parent.is_some())
            .map(|id| inner.depth(id))
            .max();
        assert_eq!(deepest, Some(MAX_DEPTH));
    }

    assert!(parse_document(&html, &limited(max_nodes), &call).is_ok());
}

// A real page: 20,000 paragraphs, each with an unclosed `font`. Noah's Ark keeps three entries,
// so a paragraph costs 210 units, 21 percent of the budget in all.
#[test]
fn unclosed_font_page_of_20000_paragraphs_parses() {
    let paragraph = "<p><font color=\"#000000\" size=2 face=Arial>text of the paragraph\n</p>";
    let html = format!("<html><body>{}", paragraph.repeat(20_000));
    let tree = parse_document(&html, &ParseLimits::default(), &Call::default()).unwrap();
    assert!(tree.len() > 80_000, "{} nodes", tree.len());
    let (abort, work) = compare_outcome(&html, DEFAULT_MAX_NODES);
    assert_eq!(abort, None);
    assert!(
        work * 2 < DEFAULT_MAX_NODES * COMPARE_WORK_PER_NODE,
        "{work} units"
    );
}

/// `count` paragraphs, each with an unclosed `<font color=cN size=2>`, the colors cycling.
fn font_paragraphs(count: usize, colors: usize) -> String {
    let body: String = (0..count)
        .map(|i| format!("<p><font color=c{} size=2>t</p>", i % colors))
        .collect();
    format!("<html><body>{body}")
}

// Reconstruction and adoption-agency clones run no Noah's Ark compare, so they are not charged
// as one: pages of unclosed formatting elements that differ in an attribute stay parseable.
#[test]
fn unclosed_font_pages_with_varying_attributes_parse() {
    for (count, colors) in [(200, 200), (2_000, 10), (500, 20)] {
        let html = font_paragraphs(count, colors);
        let tree = parse(&html, false)
            .unwrap_or_else(|error| panic!("{count} paragraphs, {colors} colors: {error:?}"));
        assert_eq!(
            dump(&tree, DumpMode::Exact),
            dump(&whole_tree(&html, false), DumpMode::Exact),
            "{count} paragraphs, {colors} colors"
        );
    }
}

// html5ever cannot stop inside a piece, so the gate drops the tokens after the abort: fed whole,
// 1,000 paragraphs create the 20 elements the limit allows, one more, and no more.
#[test]
fn tokens_after_the_abort_do_not_reach_the_tree_builder() {
    let call = Call::default();
    let html = format!("<html><body>{}", "<p>x".repeat(1_000));
    for fragment in [false, true] {
        let driver = Driver::new(new_sink(&limited(20), &call, fragment), fragment);
        driver.process(&html);
        let inner = driver.sink().inner.borrow();
        assert_eq!(inner.abort, Some(TOO_LARGE), "fragment {fragment}");
        assert!(inner.stopped);
        assert_eq!(inner.elements, 21, "fragment {fragment}");
    }
}

// The charge counts same-name open formatting elements as the ancestors of the new element.
// html5ever's list holds them as open elements (reconstruction runs first), and they are
// ancestors even across foster parenting and the adoption agency. The page is fed one tag at a
// time and, after every formatting start tag, each same-name formatting element html5ever still
// references (open stack and list, `trace_handles`) must be an ancestor of the new element. The
// pieces hold no marker (`td`, `th`, `caption`, `template`, `applet`, `object`, `marquee`), whose
// entries html5ever stops comparing at.
#[test]
fn same_name_formatting_elements_are_ancestors_of_the_new_one() {
    struct Handles(RefCell<Vec<u32>>);
    impl html5ever::interface::Tracer for Handles {
        type Handle = u32;
        fn trace_handle(&self, node: &u32) {
            self.0.borrow_mut().push(*node);
        }
    }
    let formatting = [
        ("<b>", "b"),
        ("<b x=1>", "b"),
        ("<b x=2>", "b"),
        ("<i>", "i"),
        ("<a href=x>", "a"),
        ("<a>", "a"),
        ("<font color=r>", "font"),
        ("<u>", "u"),
        ("<nobr>", "nobr"),
        ("<em>", "em"),
    ];
    let others = [
        "</b>",
        "</i>",
        "</a>",
        "</font>",
        "</u>",
        "</nobr>",
        "</em>",
        "<p>",
        "</p>",
        "<div>",
        "</div>",
        "<table>",
        "</table>",
        "<tbody>",
        "<tr>",
        "<span>",
        "</span>",
        "x",
        "<br>",
        "<li>",
        "<ul>",
        "</ul>",
        "<h1>",
        "</h1>",
        "<form>",
        "<button>",
        "</button>",
        "<svg>",
        "</svg>",
        "<select>",
        "<option>",
        "</select>",
        "<pre>",
        "<center>",
    ];
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % bound as u64).unwrap()
    };
    let mut checked = 0usize;
    for round in 0..3_000 {
        let call = Call::default();
        let limits = ParseLimits {
            max_nodes: None,
            ..ParseLimits::default()
        };
        let mut parser = html5ever::parse_document(new_sink(&limits, &call, false), parse_opts());
        let mut fed = String::new();
        for _ in 0..40 {
            let (piece, name) = if next(2) == 0 {
                let (piece, name) = formatting[next(formatting.len())];
                (piece, Some(name))
            } else {
                (others[next(others.len())], None)
            };
            fed.push_str(piece);
            parser.process(StrTendril::from_slice(piece));
            let Some(name) = name else { continue };
            let handles = Handles(RefCell::new(Vec::new()));
            parser.tokenizer.sink.trace_handles(&handles);
            let inner = parser.tokenizer.sink.sink.inner.borrow();
            let Some(new) = inner.nodes.len().checked_sub(1) else {
                continue;
            };
            let new = u32::try_from(new).unwrap();
            let node = inner.node(new);
            if node.kind != Kind::Element || &*node.qual.local != name {
                continue;
            }
            let mut ancestors = HashSet::new();
            let mut up = node.parent;
            while let Some(id) = up {
                ancestors.insert(id);
                up = inner.node(id).parent;
            }
            if ancestors.is_empty() {
                continue;
            }
            checked += 1;
            for &handle in handles.0.borrow().iter() {
                let other = inner.node(handle);
                let same_name = other.kind == Kind::Element
                    && other.qual.ns == ns!(html)
                    && &*other.qual.local == name;
                assert!(
                    handle == new || !same_name || ancestors.contains(&handle),
                    "round {round}: {name} {handle} is not an ancestor of {new} after {fed:?}"
                );
            }
        }
    }
    assert!(checked > 20_000, "{checked} formatting tags checked");
}

#[test]
fn long_script_text_dropped_across_chunks() {
    let html = format!("<script>{}</script><p>x</p>", "a".repeat(CHUNK_BYTES * 3));
    let limits = ParseLimits {
        drop_script_text: true,
        ..ParseLimits::default()
    };
    let tree = parse_document(&html, &limits, &Call::default()).unwrap();
    let text_of = |name: &str| {
        let mut stack = vec![tree.root()];
        while let Some(id) = stack.pop() {
            if &*tree.node(id).name == name {
                return tree.node(id).text.clone();
            }
            stack.extend(tree.children(id));
        }
        panic!("no element named {name}");
    };
    assert_eq!(text_of("script"), None);
    assert_eq!(text_of("p"), Some("x".to_string()));
    assert!(tree.dropped_script_text);
}
