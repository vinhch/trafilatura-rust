use super::*;

/// The lines joined as the dump prints them, one `\n` after each.
fn dump_lines(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// `md_with` in Markdown mode.
fn md(body: &str) -> String {
    md_with(body, true)
}

#[test]
fn test_handle_lists() {
    // handle_lists output structure on small and malformed lists.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let cases = [
        // malformed list (common error): loose text becomes an item
        (
            "Description:<item>a</item><item>b</item>",
            "<item>Description:</item><item>a</item><item>b</item>",
        ),
        (
            "header<item>text</item>",
            "<item>header</item><item>text</item>",
        ),
        ("   <item>text</item>", "<item>text</item>"),
        ("<item><p>text</p></item>", "<item><p>text</p></item>"),
        (
            "<item>text1<p>text2</p></item>",
            "<item>text1<p>text2</p></item>",
        ),
        (
            "<item>text<lb/>more text</item>",
            "<item>text<lb/>more text</item>",
        ),
        // item tails join the item, after its last child
        ("<item>text</item>tail", "<item>text tail</item>"),
        (
            "<item><p>text</p></item>tail",
            "<item><p>text</p>tail</item>",
        ),
        (
            "<item><p>text</p>tail1</item>tail",
            "<item><p>text</p>tail1 tail</item>",
        ),
        (
            "<item><p>text</p>\n</item>tail",
            "<item><p>text</p>tail</item>",
        ),
        (
            "<item><list><item>text</item></list></item>tail",
            "<item><list><item>text</item></list>tail</item>",
        ),
        // the first item of a nested list does not come back as an item of the parent
        (
            "<item>a<list><item>b<list><item>c</item></list></item></list></item>",
            "<item>a<list><item>b<list><item>c</item></list></item></list></item>",
        ),
    ];
    for (items, expected) in cases {
        let mut tree = xml_tree(&format!("<list>{items}</list>"));
        let root = tree.root();
        let list = handle_lists(&mut tree, root, &ctx).unwrap().unwrap();
        assert_eq!(
            tree.tostring(list, false, true),
            format!("<list>{expected}</list>"),
            "{items}"
        );
    }
}

#[test]
fn test_markdown_escaping() {
    // Markdown-mode output escapes metacharacters that would corrupt GFM structure.
    // pipe in table cell text must be escaped so it doesn't split the column
    assert!(
        md("<body><table><row><cell>a|b</cell><cell>c</cell></row></table></body>")
            .contains("a\\|b")
    );

    // pipe inside formatted text (hi) inside a cell must also be escaped
    assert!(
        md(r##"<body><table><row><cell><hi rend="#b">x|y</hi></cell></row></table></body>"##)
            .contains("x\\|y")
    );

    // URL with a space must be wrapped in angle brackets
    assert!(
        md(r#"<body><p><ref target="http://a b/c">link</ref></p></body>"#)
            .contains("[link](<http://a b/c>)")
    );

    // square brackets in link text must be escaped
    assert!(
        md(r#"<body><p><ref target="http://x">a[b]c</ref></p></body>"#)
            .contains("[a\\[b\\]c](http://x)")
    );

    // square brackets in image alt must be escaped
    assert!(
        md(r#"<body><graphic src="img.png" alt="a[b]c"/></body>"#)
            .contains("![a\\[b\\]c](img.png)")
    );

    // a ref with no/empty target renders as bare [text]; a graphic with no src keeps empty parens
    assert_eq!(md("<body><p><ref>txt</ref></p></body>"), "[txt]");
    assert_eq!(
        md(r#"<body><p><ref target="">txt</ref></p></body>"#),
        "[txt]"
    );
    assert_eq!(md(r#"<body><graphic alt="a"/></body>"#), "![a]()");

    // backtick in inline code needs a longer fence
    assert!(md(r##"<body><p><hi rend="#t">a`b</hi></p></body>"##).contains("``a`b``"));

    // inline code whose content abuts a backtick gets a space pad so the fence does not merge
    assert_eq!(
        md(r##"<body><p><hi rend="#t">`x</hi></p></body>"##),
        "`` `x ``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">x`</hi></p></body>"##),
        "`` x` ``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">`</hi></p></body>"##),
        "`` ` ``"
    );

    // triple backtick in block code needs a 4-backtick fence
    let result = md("<body><code>a```b</code></body>");
    assert!(result.contains("````"));
    assert!(result.contains("a```b"));

    // ~~ inside del content must not close the strikethrough early
    assert!(md("<body><p><del>a~~b</del></p></body>").contains("~~a\\~\\~b~~"));

    // del wrapping an inline child must preserve the strikethrough marker
    assert!(
        md(r##"<body><p><del><hi rend="#b">bold</hi></del></p></body>"##).contains("~~**bold**~~")
    );

    // pipe in graphic tail inside a table cell must be escaped
    assert!(
        md(r#"<body><table><row><cell><graphic src="img.png" alt="img"/>tail|text</cell></row></table></body>"#)
            .contains("tail\\|text")
    );
    // pipe in non-graphic element tail inside a cell must also be escaped (separate code path)
    assert!(
        md(r##"<body><table><row><cell><hi rend="#b">bold</hi>tail|pipe</cell></row></table></body>"##)
            .contains("tail\\|pipe")
    );

    // block math in a cell must not inject newlines that split the table row
    let result = md("<body><table><row><cell>x \\[E=mc^2\\] y</cell></row></table></body>");
    assert!(!result.contains('\n'));
    assert!(result.contains("$$ E=mc^2 $$"));

    // bold wrapping a link must preserve both the bold marker and the link
    assert!(
        md(r##"<body><p><hi rend="#b"><ref target="http://x.com">link</ref></hi></p></body>"##)
            .contains("**[link](http://x.com)**")
    );

    // a graphic wrapped in inline formatting must keep the image, not be dropped
    assert_eq!(
        md(r##"<body><p><hi rend="#b">x <graphic src="i.jpg" alt="A"/> y</hi></p></body>"##),
        "**x ![A](i.jpg) y**"
    );
    assert_eq!(
        md(r#"<body><p><del>x <graphic src="i.jpg" alt="A"/> y</del></p></body>"#),
        "~~x ![A](i.jpg) y~~"
    );

    // del as a direct child of a table cell (no enclosing p)
    let result = extract(
        "<html><body><table><tr><td><del>gone</del></td></tr></table></body>",
        &Kw::zero().markdown().formatting(true),
    )
    .unwrap();
    assert!(!result.is_empty());
    assert!(result.contains("~~gone~~"));
}

#[test]
fn test_markdown_asterisk_escaping() {
    // Literal '*' in source text must be escaped so CommonMark renders it literally, not as emphasis.

    // a bare asterisk in plain paragraph text must not turn into emphasis
    assert_eq!(
        md("<body><p>This *should* not be italic and 3*4=12</p></body>"),
        "This \\*should\\* not be italic and 3\\*4=12"
    );

    // a real <hi> bold marker must survive untouched next to escaped literal asterisks
    assert_eq!(
        md(r##"<body><p><hi rend="#b">bold</hi> and *literal* asterisks</p></body>"##),
        "**bold** and \\*literal\\* asterisks"
    );

    // asterisks inside a heading are escaped, the '#' prefix is untouched
    assert_eq!(
        md(r#"<body><head rend="h2">Title *with* asterisk</head></body>"#),
        "## Title \\*with\\* asterisk"
    );

    // asterisks inside a list item are escaped, the '- ' marker is untouched
    assert_eq!(
        md("<body><list><item>list item *text*</item></list></body>"),
        "- list item \\*text\\*"
    );

    // asterisks inside table cell text are escaped alongside the pre-existing pipe escaping
    assert!(md("<body><table><row><cell>a*b|c</cell></row></table></body>").contains("a\\*b\\|c"));

    // asterisks inside link text are escaped, the [text](url) syntax is untouched
    assert!(
        md(r#"<body><p><ref target="http://x.com">link *text*</ref></p></body>"#)
            .contains("[link \\*text\\*](http://x.com)")
    );

    // asterisks inside image alt text are escaped
    assert!(md(r#"<body><graphic src="i.png" alt="a*b"/></body>"#).contains("![a\\*b](i.png)"));

    // asterisks inside struck-through (del) text are escaped, the ~~ markers are untouched
    assert_eq!(md("<body><p><del>a*b</del></p></body>"), "~~a\\*b~~");

    // a bold link keeps both its ** markers and its brackets: nested markup must not be
    // re-escaped as if it were literal source text
    assert_eq!(
        md(
            r##"<body><p><ref target="http://x.com"><hi rend="#b">bold link</hi></ref></p></body>"##
        ),
        "[**bold link**](http://x.com)"
    );

    // asterisks inside verbatim code (block and inline) must NOT be escaped
    assert_eq!(md("<body><code>a * b</code></body>"), "`a * b`");
    assert_eq!(
        md(r##"<body><p><hi rend="#t">a*b</hi></p></body>"##),
        "`a*b`"
    );

    // a run of consecutive asterisks (e.g. a plain-text divider) must not be read as emphasis
    assert_eq!(
        md("<body><p>*** section break ***</p></body>"),
        "\\*\\*\\* section break \\*\\*\\*"
    );

    // an even-length run must also be escaped character-by-character, not skipped as a pair
    assert_eq!(
        md("<body><p>**not bold**</p></body>"),
        "\\*\\*not bold\\*\\*"
    );

    // a literal '*' touching a real <hi> open/close boundary (immediately inside or outside
    // the tag, singly or doubled) must be escaped without disturbing the real emphasis
    // marker. #i is the higher-risk case since its marker is a single '*', the same
    // character being escaped, while #b's '**' is a different-looking two-char sequence.
    for (rend, marker) in [("#i", "*"), ("#b", "**")] {
        // just outside the opening tag
        assert_eq!(
            md(&format!(r#"<body><p>*<hi rend="{rend}">x</hi></p></body>"#)),
            format!("\\*{marker}x{marker}")
        );
        // just inside the opening tag (first character of the hi's own text)
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">*x</hi></p></body>"#)),
            format!("{marker}\\*x{marker}")
        );
        // just outside the closing tag
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">x</hi>*</p></body>"#)),
            format!("{marker}x{marker}\\*")
        );
        // just inside the closing tag (last character of the hi's own text)
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">x*</hi></p></body>"#)),
            format!("{marker}x\\*{marker}")
        );
        // wrapping the text from inside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p><hi rend="{rend}">*x*</hi></p></body>"#
            )),
            format!("{marker}\\*x\\*{marker}")
        );
        // wrapping the whole <hi> element from outside on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p>*<hi rend="{rend}">x</hi>*</p></body>"#
            )),
            format!("\\*{marker}x{marker}\\*")
        );
        // a literal '**' run just inside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p><hi rend="{rend}">**x**</hi></p></body>"#
            )),
            format!("{marker}\\*\\*x\\*\\*{marker}")
        );
        // a literal '**' run just outside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p>**<hi rend="{rend}">x</hi>**</p></body>"#
            )),
            format!("\\*\\*{marker}x{marker}\\*\\*")
        );
    }

    // a pre-existing literal backslash immediately before an asterisk must round-trip:
    // rendered output should still show exactly one backslash then one asterisk
    assert_eq!(
        md("<body><p>Edge case: a\\*b</p></body>"),
        "Edge case: a\\\\\\*b"
    );

    // include_formatting=False is plain-text mode: asterisks must be left completely alone
    assert_eq!(
        md_with("<body><p>This *stays* as-is</p></body>", false),
        "This *stays* as-is"
    );
}

#[test]
fn test_markdown_special_char_escaping() {
    // Literal CommonMark metacharacters other than '*' must also be escaped so they render as
    // themselves: '_', '`', '[', ']', '<', '~' are ambiguous wherever they appear in prose, while
    // '#', '-', '+', '.'/')' after digits and '>' only matter at the very start of a fresh block
    // line (heading/list/blockquote syntax). Everything else (quotes, parens, punctuation) is inert
    // in body text and must be left alone.

    // underscore, backtick, brackets and '<' are escaped anywhere in a paragraph
    assert_eq!(
        md("<body><p>a_b_c and `code` and [bracket] and &lt;tag&gt; text</p></body>"),
        "a\\_b\\_c and \\`code\\` and \\[bracket\\] and \\<tag> text"
    );
    // a lone '~' is also escaped: GitHub renders a single tilde as strikethrough, not just '~~'
    assert_eq!(
        md("<body><p>~single~ tilde</p></body>"),
        "\\~single\\~ tilde"
    );

    // a leading '#' in a plain paragraph must not become an ATX heading
    assert_eq!(
        md("<body><p># not a heading</p></body>"),
        "\\# not a heading"
    );
    // a leading '>' in a plain paragraph must not become a blockquote
    assert_eq!(
        md("<body><p>&gt; not a quote</p></body>"),
        "\\> not a quote"
    );
    // a leading '-'/'+' in a plain paragraph must not become a bullet list
    assert_eq!(md("<body><p>- not a list</p></body>"), "\\- not a list");
    assert_eq!(md("<body><p>+ not a list</p></body>"), "\\+ not a list");
    // a leading digit + '.'/')' in a plain paragraph must not become an ordered list
    assert_eq!(md("<body><p>1. not a list</p></body>"), "1\\. not a list");
    assert_eq!(md("<body><p>1) not a list</p></body>"), "1\\) not a list");

    // the same markers are safe mid-line and must be left alone (only line-start is risky)
    assert_eq!(
        md("<body><p>mid-line - and 3.14 and 100% and word) stay put</p></body>"),
        "mid-line - and 3.14 and 100% and word) stay put"
    );

    // a list item's own content starting with a marker-like character must not nest as a
    // sub-list; the item's real '- '/'N. ' marker (added separately) is untouched
    assert_eq!(
        md("<body><list><item>- nested looking</item></list></body>"),
        "- \\- nested looking"
    );
    assert_eq!(
        md(r#"<body><list rend="ol"><item>2. nested ordered looking</item></list></body>"#),
        "1. 2\\. nested ordered looking"
    );

    // a blockquote element's own leading '>' must also be escaped
    assert_eq!(
        md("<body><quote>&gt; quoted text with - marker</quote></body>"),
        "\\> quoted text with - marker"
    );

    // a heading's own '#' prefix already claims the line, so an inner leading '#' is inert
    assert_eq!(
        md(r##"<body><head rend="h2"># nested hash in heading</head></body>"##),
        "## # nested hash in heading"
    );

    // table cells are inline-only in GFM: a leading marker-like character there is never risky
    assert!(
        md("<body><table><row><cell>- looks like a list</cell></row></table></body>")
            .contains("| - looks like a list |")
    );

    // inert punctuation (quotes, parens, and other characters with no CommonMark meaning in
    // body text) must be left completely alone
    let inert = "<body><p>quote &quot; apostrophe ' paren ( ) percent % dollar $ at @ caret ^ eq = brace { } colon : semi ; comma , bang !</p></body>";
    assert_eq!(
        md(inert),
        "quote \" apostrophe ' paren ( ) percent % dollar $ at @ caret ^ eq = brace { } colon : semi ; comma , bang !"
    );
    // '!' alone (not immediately escaping a '[') does not need its own escaping
    assert_eq!(
        md("<body><p>! not an image on its own</p></body>"),
        "! not an image on its own"
    );

    // verbatim code (block and inline) must not have any of these characters escaped
    assert_eq!(
        md("<body><code>a_b`c[d]e&lt;f&gt;</code></body>"),
        "``a_b`c[d]e<f>``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">a_b`c</hi></p></body>"##),
        "``a_b`c``"
    );

    // brackets in link/image text are still escaped so a false image ('![...]') can't form,
    // without needing to escape '!' itself: an escaped '[' already blocks the image syntax
    assert_eq!(
        md(r#"<body><p><ref target="http://x.com">![img]-like text</ref></p></body>"#),
        "[!\\[img\\]-like text](http://x.com)"
    );

    // a pre-existing literal backslash immediately before an underscore must round-trip:
    // rendered output should still show exactly one backslash then one underscore
    assert_eq!(
        md("<body><p>Edge case: a\\_b</p></body>"),
        "Edge case: a\\\\\\_b"
    );

    // include_formatting=False is plain-text mode: none of these characters are touched
    assert_eq!(
        md_with(
            "<body><p>This _stays_ as-is with `code` [b] &lt;tag&gt;</p></body>",
            false
        ),
        "This _stays_ as-is with `code` [b] <tag>"
    );
}

#[test]
fn test_markdown_link_code_brackets() {
    // Backslash escapes are literal inside a code span, so brackets in inline code within link text stay as-is.
    let cases = [
        ("<code>a[b]c</code>", "[`a[b]c`](http://x.com)"),
        // code spans bind tighter than link brackets, unbalanced ones included
        ("<code>a]b</code>", "[`a]b`](http://x.com)"),
        (r##"<hi rend="#t">x[0]</hi>"##, "[`x[0]`](http://x.com)"),
        ("<code>a`b[1]</code>", "[``a`b[1]``](http://x.com)"),
        // brackets in the surrounding link text are still escaped
        ("f <code>a]b</code> [q]", "[f `a]b` \\[q\\]](http://x.com)"),
    ];
    for (inner, expected) in cases {
        let body = format!(r#"<body><p><ref target="http://x.com">{inner}</ref></p></body>"#);
        assert_eq!(md(&body), expected);
    }
}

#[test]
fn test_markdown_link_angle_bracket_targets() {
    // A '<' or '>' in a link/image target must stay inside the angle-bracket destination.
    // upstream: tests/unit_tests.py::test_markdown_link_angle_bracket_targets
    let cases = [
        ("http://x.com/a>b", "[t](<http://x.com/a\\>b>)"),
        ("http://x.com/a<b", "[t](<http://x.com/a\\<b>)"),
        ("http://x.com/a<b>c", "[t](<http://x.com/a\\<b\\>c>)"),
        ("http://x.com/a b>c", "[t](<http://x.com/a b\\>c>)"),
        ("http://x.com/end>", "[t](<http://x.com/end\\>>)"),
        ("<start.example/p", "[t](<\\<start.example/p>)"),
        ("http://x.com/a\\>b", "[t](<http://x.com/a\\\\\\>b>)"),
        ("http://x.com/a>>b", "[t](<http://x.com/a\\>\\>b>)"),
        (">x.com/p", "[t](<\\>x.com/p>)"),
        ("http://x.com/<>", "[t](<http://x.com/\\<\\>>)"),
        ("http://x.com/a b/c", "[t](<http://x.com/a b/c>)"),
        ("http://x.com/a(b)", "[t](<http://x.com/a(b)>)"),
        ("http://x.com/a)b(c", "[t](<http://x.com/a)b(c>)"),
        ("http://x.com/a(b", "[t](<http://x.com/a(b>)"),
        ("http://x.com/plain", "[t](http://x.com/plain)"),
    ];
    for (target, expected) in cases {
        assert_eq!(md_link("t", Some(target), false), expected);
        assert_eq!(md_link("t", Some(target), true), format!("!{expected}"));
    }
    assert_eq!(md_link("t", None, false), "[t]");
    assert_eq!(md_link("t", None, true), "![t]");
    assert_eq!(
        md_link("a[b]c", Some("http://x.com/p>q"), false),
        "[a\\[b\\]c](<http://x.com/p\\>q>)"
    );

    // end-to-end through xmltotxt: '>' and '<' arrive as entities in the attribute
    assert_eq!(
        md(r#"<body><p><ref target="http://x.com/a&gt;b">t</ref></p></body>"#),
        "[t](<http://x.com/a\\>b>)"
    );
    assert_eq!(
        md(r#"<body><graphic src="http://x.com/a&lt;b" alt="A"/></body>"#),
        "![A](<http://x.com/a\\<b>)"
    );
}

#[test]
fn test_xmltotxt_no_mutation() {
    // xmltotxt must not mutate its input tree (math/emphasis passes run on a deepcopy).
    let mut tree =
        xml_tree(r##"<body><p>formula \(x\) <hi rend="#b"><hi rend="#i">y</hi></hi></p></body>"##);
    let root = tree.root();
    let before = tree.tostring(root, false, false);
    let out = xml_to_text(&mut tree, Some(root), true, &Call::default()).unwrap();
    assert!(out.contains("$x$"));
    assert!(out.contains("***y***"));
    // source tree untouched
    assert_eq!(tree.tostring(root, false, false), before);
}

#[test]
fn test_math_conversion() {
    // LaTeX math delimiters are converted to $ notation; unmatched delimiters and code are left alone.
    // inline math \(...\) -> $...$
    assert_eq!(md("<body><p>\\(x^2\\)</p></body>"), "$x^2$");
    // unmatched \( must not become a lone $
    assert_eq!(md("<body><p>regex \\( open</p></body>"), "regex \\( open");
    // math delimiters inside a fenced code block must not be converted
    assert!(
        xmltotxt(
            "<body><code>prefix\n\\[E=mc^2\\]\nsuffix</code></body>",
            true
        )
        .contains("\\[E=mc^2\\]")
    );
    // math delimiters inside inline code must not be converted either
    assert!(
        xmltotxt(
            r##"<body><p>use <hi rend="#t">a\(x\)b</hi> here</p></body>"##,
            true
        )
        .contains("`a\\(x\\)b`")
    );
    // but math in a code element's prose tail is still converted
    assert!(
        xmltotxt("<body><p><code>k</code> then \\(t\\) end</p></body>", true)
            .contains("`k` then $t$ end")
    );
}

#[test]
fn test_math_is_not_commonmark_escaped() {
    // Inside a formula '_', '*', '[', ']', '<', '`' and '~' are LaTeX notation, so the
    // CommonMark escaping pass must step over math spans while still escaping the prose
    // around them.

    // subscripts and '<' survive inline math verbatim
    assert_eq!(
        md("<body><p>we write \\(a_1 &lt; a_2\\) here</p></body>"),
        "we write $a_1 < a_2$ here"
    );
    // block math too, including braces, stars and brackets
    assert_eq!(
        md("<body><p>\\[ x_i = \\sum_j A_{ij} y_j^* \\]</p></body>"),
        "$$\nx_i = \\sum_j A_{ij} y_j^*\n$$"
    );
    assert_eq!(
        md("<body><p>\\(\\left[u_0\\right]\\)</p></body>"),
        "$\\left[u_0\\right]$"
    );
    // prose on both sides of a formula is still escaped
    assert_eq!(
        md("<body><p>file_one has \\(n_1\\) rows in dir_two</p></body>"),
        "file\\_one has $n_1$ rows in dir\\_two"
    );
    // an unmatched delimiter is not a math span, so its text is escaped as ordinary prose
    assert_eq!(
        md("<body><p>regex \\( a_b open</p></body>"),
        "regex \\( a\\_b open"
    );
}

#[test]
fn test_math_recovery() {
    // D-2: upstream parses with libxml2, where `math` has no namespace and is recovered. On the
    // html5 tree `math` carries the MathML namespace, so neither `recover_math` nor the
    // `MANUALLY_CLEANED` loop sees it: nothing is recovered and nothing is dropped. The expected
    // dumps are the html5 results of upstream's own functions; `recover_math` itself is tested in
    // `html_processing::tests::cleaning::recover_math_on_html_namespace_math`.
    // oracle: dump_tree(tree_cleaning(parse_document(html), options)) with options.tables = True
    let call = Call::default();
    let mut options = default_options();
    options.tables = true;
    let clean = |html: &str| {
        let (mut tree, root) = lxml_fromstring(html);
        tree_cleaning(&mut tree, root, &ctx_for(&options, &call)).unwrap();
        dump(&tree, DumpMode::Exact).0
    };

    // the annotation holds the original LaTeX and takes precedence over alttext
    assert_eq!(
        clean(
            r#"<html><body><p>a <math alttext="rendered"><semantics><annotation encoding="application/x-tex">x^2</annotation></semantics></math> b</p></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            r#"      math@math alttext="rendered""#,
            "        semantics@math",
            r#"          annotation@math encoding="application/x-tex""#,
            r#"            #text "x^2""#,
            r#"      #text " b""#,
        ])
    );
    // alttext is used when there is no LaTeX annotation
    assert_eq!(
        clean(r#"<html><body><p>a <math alttext="y_1"><mi>y</mi></math> b</p></body></html>"#),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            r#"      math@math alttext="y_1""#,
            "        mi@math",
            r#"          #text "y""#,
            r#"      #text " b""#,
        ])
    );
    // display="block" yields block delimiters
    assert_eq!(
        clean(
            r#"<html><body><p><math display="block" alttext="z"><mi>z</mi></math></p></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      math@math alttext="z" display="block""#,
            "        mi@math",
            r#"          #text "z""#,
        ])
    );
    // nothing to recover: the element stays too, as nothing sees it
    assert_eq!(
        clean("<html><body><p>a <math><mi>q</mi></math> b</p></body></html>"),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            "      math@math",
            "        mi@math",
            r#"          #text "q""#,
            r#"      #text " b""#,
        ])
    );
    // the tail of the formula
    assert_eq!(
        clean(r#"<html><body><p><math alttext="u"><mi>u</mi></math> tail</p></body></html>"#),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      math@math alttext="u""#,
            "        mi@math",
            r#"          #text "u""#,
            r#"      #text " tail""#,
        ])
    );
    // a formula in a table cell
    assert_eq!(
        clean(
            r#"<html><body><table><tr><td>name</td><td><math alttext="1.33\times 10^{-4}"><mi>v</mi></math></td></tr></table></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    table",
            "      tr",
            "        td",
            r#"          #text "name""#,
            "        td",
            r#"          math@math alttext="1.33\\times 10^{-4}""#,
            "            mi@math",
            r#"              #text "v""#,
        ])
    );

    // end to end: Markdown output renders the recovered formula as $...$
    // deviation: D-2, upstream asserts "$1.33\times 10^{-4}$" in the result; on the html5 tree the
    // `math` element carries the MathML namespace, so nothing is recovered and the annotation text
    // stays as it is. oracle: extract(doc, output_format="markdown", config=ZERO_CONFIG) inside
    // html5_parsers(), with fast=True
    let doc = r#"<html><body><article><p>Some introductory prose that is long enough to be retained here.</p><p>The error was <math alttext="1.33\times 10^{-4}"><semantics><annotation encoding="application/x-tex">1.33\times 10^{-4}</annotation></semantics></math> overall.</p></article></body></html>"#;
    assert_eq!(
        extract(doc, &Kw::zero().markdown()).as_deref(),
        Some(
            "Some introductory prose that is long enough to be retained here.\n\nThe error was 1.33\\times 10^{-4} overall."
        )
    );
}

#[test]
fn test_inline_edge_cases() {
    // Cover lb in inline context, structural-tag fallback, and redundant emphasis collapse.
    // <lb/> inside an inline element (hi) renders as a newline
    assert!(
        xmltotxt(r##"<body><p><hi rend="#b">A<lb/>B</hi></p></body>"##, true).contains("**A\nB**")
    );
    // structural tag (e.g. <span>) nested inside inline: its text is carried through
    assert!(
        xmltotxt(
            r##"<body><p><hi rend="#b"><span>carried</span></hi></p></body>"##,
            true
        )
        .contains("**carried**")
    );
    // redundant nested emphasis is collapsed: **x** not ****x****
    assert_eq!(
        md(r##"<body><p><hi rend="#b"><hi rend="#b">x</hi></hi></p></body>"##),
        "**x**"
    );
}

#[test]
fn test_heading_inline_formatting() {
    // A heading keeps its # prefix even when it starts with an inline child (e.g. <h2><strong>...).

    // child-first headings must NOT lose the # prefix
    assert_eq!(
        md(r##"<body><head rend="h2"><hi rend="#b">B</hi> text</head></body>"##),
        "## **B** text"
    );
    assert_eq!(
        md(r##"<body><head rend="h2"><hi rend="#b">B</hi></head></body>"##),
        "## **B**"
    );
    assert_eq!(
        md(r##"<body><head rend="h3"><hi rend="#i">I</hi> rest</head></body>"##),
        "### *I* rest"
    );
    // no rend -> level 2
    assert_eq!(
        md(r##"<body><head><hi rend="#b">B</hi> t</head></body>"##),
        "## **B** t"
    );
    // text-first and plain headings are unchanged
    assert_eq!(
        md(r##"<body><head rend="h2">x <hi rend="#b">B</hi></head></body>"##),
        "## x **B**"
    );
    assert_eq!(
        md(r#"<body><head rend="h2">plain</head></body>"#),
        "## plain"
    );
    // a heading inside a table cell stays prefix-less
    assert_eq!(
        md(
            r##"<body><table><row><cell><head rend="h3"><hi rend="#b">H</hi></head></cell></row></table></body>"##
        ),
        "| **H** |"
    );
}

#[test]
fn test_mixed_content_extraction() {
    // Test extraction from HTML with mixed content.
    let html =
        r#"<html><body><p>Text here</p><img src="img.jpg"/><video src="video.mp4"/></body></html>"#;
    // deviation: D-1, upstream runs this with `fast=False`; the value is the same
    assert_eq!(extract(html, &Kw::zero()).as_deref(), Some("Text here"));
}

#[test]
fn test_nonstd_html_entities() {
    // Test handling non-standard HTML entities.
    let html = "<html><body><p>Text &customentity; more text</p></body></html>";
    // deviation: D-1, upstream runs this with `fast=False`; the value is the same
    assert_eq!(
        extract(html, &Kw::zero()).as_deref(),
        Some("Text &customentity; more text")
    );
}

#[test]
fn test_large_doc_performance() {
    // Performance test on large HTML documents.
    let large_html = format!(
        "<html><body>{}</body></html>",
        "<p>Sample text</p>".repeat(10000)
    );
    let start = std::time::Instant::now();
    // deviation: D-1, upstream runs this with `fast=False`
    extract(&large_html, &Kw::zero());
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "Large document performance issue"
    );
}
