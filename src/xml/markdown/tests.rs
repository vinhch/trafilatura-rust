use regex::Regex;

use super::*;

#[test]
fn escape_block_start_matches_python() {
    let cases = [
        // oracle: xml._escape_block_start("#\n")
        ("#\n", "\\#\n"),
        // oracle: xml._escape_block_start("#\nx")
        ("#\nx", "#\nx"),
        // oracle: xml._escape_block_start("1.\n")
        ("1.\n", "1\\.\n"),
        // oracle: xml._escape_block_start("\u0661. x")
        ("\u{661}. x", "\u{661}\\. x"),
        // oracle: xml._escape_block_start("\u0661\n")
        ("\u{661}\n", "\u{661}\n"),
        // oracle: xml._escape_block_start("-\n")
        ("-\n", "\\-\n"),
        // oracle: xml._escape_block_start("   #  a")
        ("   #  a", "   \\#  a"),
        // oracle: xml._escape_block_start("    # a")
        ("    # a", "    # a"),
        // oracle: xml._escape_block_start("\uff11.")
        ("\u{ff11}.", "\u{ff11}\\."),
        // oracle: xml._escape_block_start("1234567890. x")
        ("1234567890. x", "1234567890. x"),
        // oracle: xml._escape_block_start("-\n\n")
        ("-\n\n", "-\n\n"),
        // oracle: xml._escape_block_start("## ")
        ("## ", "\\## "),
        // oracle: xml._escape_block_start("#######\n")
        ("#######\n", "#######\n"),
        // oracle: xml._escape_block_start("1.\n\n")
        ("1.\n\n", "1.\n\n"),
        // oracle: xml._escape_block_start("123456789) y")
        ("123456789) y", "123456789\\) y"),
        // oracle: xml._escape_block_start("a. x")
        ("a. x", "a. x"),
        // oracle: xml._escape_block_start(" > q")
        (" > q", " \\> q"),
        // oracle: xml._escape_block_start("+\n")
        ("+\n", "\\+\n"),
        // oracle: xml._escape_block_start("1) x")
        ("1) x", "1\\) x"),
        // oracle: xml._escape_block_start("\u0661\u0662. x")
        ("\u{661}\u{662}. x", "\u{661}\u{662}\\. x"),
        // oracle: xml._escape_block_start("#\t")
        ("#\t", "\\#\t"),
        // oracle: xml._escape_block_start("- a")
        ("- a", "\\- a"),
        // oracle: xml._escape_block_start("12345678. ")
        ("12345678. ", "12345678\\. "),
        // oracle: xml._escape_block_start("\u0966\u0967) \n")
        ("\u{966}\u{967}) \n", "\u{966}\u{967}\\) \n"),
    ];
    for (text, expected) in cases {
        assert_eq!(escape_block_start(text), expected, "{text:?}");
    }
}

#[test]
fn math_spans_match_python() {
    // (input, escape_and_convert_math, convert_math)
    let cases = [
        // oracle: xml._escape_and_convert_math("x\\[a \\[b\\]"), xml._convert_math("x\\[a \\[b\\]")
        (
            "x\\[a \\[b\\]",
            "x\\\\\\[a \n$$\nb\n$$\n",
            "x\\[a \n$$\nb\n$$\n",
        ),
        // oracle: xml._escape_and_convert_math("\x1c\\[a\\]"), xml._convert_math("\x1c\\[a\\]")
        (
            "\u{1c}\\[a\\]",
            "\u{1c}\n$$\na\n$$\n",
            "\u{1c}\n$$\na\n$$\n",
        ),
        // oracle: xml._escape_and_convert_math("a\\(x\\)b\\(y"), xml._convert_math("a\\(x\\)b\\(y")
        ("a\\(x\\)b\\(y", "a$x$b\\(y", "a$x$b\\(y"),
        // oracle: xml._escape_and_convert_math("$a_1$ \\(a_1\\)"), xml._convert_math("$a_1$ \\(a_1\\)")
        ("$a_1$ \\(a_1\\)", "$a\\_1$ $a_1$", "$a_1$ $a_1$"),
        // oracle: xml._escape_and_convert_math("\\(a \\[b\\] c\\)"), xml._convert_math("\\(a \\[b\\] c\\)")
        (
            "\\(a \\[b\\] c\\)",
            "\\(a \n$$\nb\n$$\n c\\)",
            "\\(a \n$$\nb\n$$\n c\\)",
        ),
        // oracle: xml._escape_and_convert_math("\\[a\\]\\[b\\]"), xml._convert_math("\\[a\\]\\[b\\]")
        (
            "\\[a\\]\\[b\\]",
            "\n$$\na\n$$\n\\\\\\[b\\\\\\]",
            "\n$$\na\n$$\n\\[b\\]",
        ),
        // oracle: xml._escape_and_convert_math("  \\[ x \\]"), xml._convert_math("  \\[ x \\]")
        ("  \\[ x \\]", "  \n$$\nx\n$$\n", "  \n$$\nx\n$$\n"),
        // oracle: xml._escape_and_convert_math("\\[\\]]"), xml._convert_math("\\[\\]]")
        ("\\[\\]]", "\\\\\\[\\\\\\]\\]", "\\[\\]]"),
        // oracle: xml._escape_and_convert_math("\\(\nx\\) \\(y\\)"), xml._convert_math("\\(\nx\\) \\(y\\)")
        ("\\(\nx\\) \\(y\\)", "\\(\nx\\) $y$", "\\(\nx\\) $y$"),
        // oracle: xml._escape_and_convert_math("\\(a\nb\\) \\(c\\)"), xml._convert_math("\\(a\nb\\) \\(c\\)")
        ("\\(a\nb\\) \\(c\\)", "\\(a\nb\\) $c$", "\\(a\nb\\) $c$"),
        // oracle: xml._escape_and_convert_math("a_b \\[x_1\\] c*d"), xml._convert_math("a_b \\[x_1\\] c*d")
        (
            "a_b \\[x_1\\] c*d",
            "a\\_b \n$$\nx_1\n$$\n c\\*d",
            "a_b \n$$\nx_1\n$$\n c*d",
        ),
        // oracle: xml._escape_and_convert_math("\\[a\\"), xml._convert_math("\\[a\\")
        ("\\[a\\", "\\\\\\[a\\", "\\[a\\"),
        // oracle: xml._escape_and_convert_math("\\[\n a_b \n\\] z\\(q\\)"), xml._convert_math("\\[\n a_b \n\\] z\\(q\\)")
        (
            "\\[\n a_b \n\\] z\\(q\\)",
            "\n$$\na_b\n$$\n z$q$",
            "\n$$\na_b\n$$\n z$q$",
        ),
        // oracle: xml._escape_and_convert_math("\xa0\\[a\\]"), xml._convert_math("\xa0\\[a\\]")
        (
            "\u{a0}\\[a\\]",
            "\u{a0}\n$$\na\n$$\n",
            "\u{a0}\n$$\na\n$$\n",
        ),
        // oracle: xml._escape_and_convert_math("\\(\\)"), xml._convert_math("\\(\\)")
        ("\\(\\)", "\\(\\)", "\\(\\)"),
        // oracle: xml._escape_and_convert_math("\\(x\\) \\("), xml._convert_math("\\(x\\) \\(")
        ("\\(x\\) \\(", "$x$ \\(", "$x$ \\("),
        // oracle: xml._escape_and_convert_math("\\[x\\] \\(y\\) *z*"), xml._convert_math("\\[x\\] \\(y\\) *z*")
        (
            "\\[x\\] \\(y\\) *z*",
            "\n$$\nx\n$$\n $y$ \\*z\\*",
            "\n$$\nx\n$$\n $y$ *z*",
        ),
    ];
    for (text, escaped, converted) in cases {
        assert_eq!(escape_and_convert_math(text), escaped, "escape {text:?}");
        assert_eq!(convert_math(text), converted, "convert {text:?}");
    }
}

#[test]
fn escape_and_convert_math_borrows_math_free_text() {
    assert!(matches!(
        escape_and_convert_math("plain words"),
        Cow::Borrowed("plain words")
    ));
    assert_eq!(escape_and_convert_math(r"a\)b *c*"), r"a\)b \*c\*");
    assert_eq!(escape_and_convert_math(r"x \(a\) y"), "x $a$ y");
}

#[test]
fn math_scanners_stay_linear_on_unclosed_openers() {
    let text = "\\(".repeat(50_000) + &"\\[".repeat(50_000);
    assert_eq!(convert_math(&text), text);
    assert!(escape_and_convert_math(&text).len() > text.len());
}

#[test]
fn md_link_matches_python() {
    // (text, url, image, expected)
    let cases = [
        // oracle: xml._md_link("`a[b]`", None)
        ("`a[b]`", None, false, "[`a[b]`]"),
        // oracle: xml._md_link("`a[b]`", "http://x/a b")
        (
            "`a[b]`",
            Some("http://x/a b"),
            false,
            "[`a[b]`](<http://x/a b>)",
        ),
        // oracle: xml._md_link("`a[b]`", "u", True)
        ("`a[b]`", Some("u"), true, "![`a[b]`](u)"),
        // oracle: xml._md_link("\\[x]", None)
        ("\\[x]", None, false, "[\\[x\\]]"),
        // oracle: xml._md_link("\\[x]", "http://x/a b")
        (
            "\\[x]",
            Some("http://x/a b"),
            false,
            "[\\[x\\]](<http://x/a b>)",
        ),
        // oracle: xml._md_link("\\[x]", "u", True)
        ("\\[x]", Some("u"), true, "![\\[x\\]](u)"),
        // oracle: xml._md_link("a[b]", None)
        ("a[b]", None, false, "[a\\[b\\]]"),
        // oracle: xml._md_link("a[b]", "http://x/a b")
        (
            "a[b]",
            Some("http://x/a b"),
            false,
            "[a\\[b\\]](<http://x/a b>)",
        ),
        // oracle: xml._md_link("a[b]", "u", True)
        ("a[b]", Some("u"), true, "![a\\[b\\]](u)"),
        // oracle: xml._md_link("``x`[y]", None)
        ("``x`[y]", None, false, "[``x`\\[y\\]]"),
        // oracle: xml._md_link("``x`[y]", "http://x/a b")
        (
            "``x`[y]",
            Some("http://x/a b"),
            false,
            "[``x`\\[y\\]](<http://x/a b>)",
        ),
        // oracle: xml._md_link("``x`[y]", "u", True)
        ("``x`[y]", Some("u"), true, "![``x`\\[y\\]](u)"),
        // oracle: xml._md_link("\\\\[z]", None)
        ("\\\\[z]", None, false, "[\\\\[z\\]]"),
        // oracle: xml._md_link("\\\\[z]", "http://x/a b")
        (
            "\\\\[z]",
            Some("http://x/a b"),
            false,
            "[\\\\[z\\]](<http://x/a b>)",
        ),
        // oracle: xml._md_link("\\\\[z]", "u", True)
        ("\\\\[z]", Some("u"), true, "![\\\\[z\\]](u)"),
        // oracle: xml._md_link("``a`b`` [c] `d", None)
        ("``a`b`` [c] `d", None, false, "[``a`b`` \\[c\\] `d]"),
        // oracle: xml._md_link("``a`b`` [c] `d", "http://x/a b")
        (
            "``a`b`` [c] `d",
            Some("http://x/a b"),
            false,
            "[``a`b`` \\[c\\] `d](<http://x/a b>)",
        ),
        // oracle: xml._md_link("`[x]", None)
        ("`[x]", None, false, "[`\\[x\\]]"),
        // oracle: xml._md_link("`[x]", "u", True)
        ("`[x]", Some("u"), true, "![`\\[x\\]](u)"),
        // oracle: xml._md_link("[a]`b`[c]`", None)
        ("[a]`b`[c]`", None, false, "[\\[a\\]`b`\\[c\\]`]"),
        // oracle: xml._md_link("[a]`b`[c]`", "http://x/a b")
        (
            "[a]`b`[c]`",
            Some("http://x/a b"),
            false,
            "[\\[a\\]`b`\\[c\\]`](<http://x/a b>)",
        ),
        // oracle: xml._md_link("\\`a[b]`", None)
        ("\\`a[b]`", None, false, "[\\`a\\[b\\]`]"),
        // oracle: xml._md_link("`a``b[c]`", None)
        ("`a``b[c]`", None, false, "[`a``b[c]`]"),
        // oracle: xml._md_link("``a` b[c] ``", None)
        ("``a` b[c] ``", None, false, "[``a` b[c] ``]"),
        // oracle: xml._md_link("a\\`[b]`c`[d]", None)
        ("a\\`[b]`c`[d]", None, false, "[a\\`\\[b\\]`c`\\[d\\]]"),
        // oracle: xml._md_link("`a\n[b]`", None)
        ("`a\n[b]`", None, false, "[`a\n[b]`]"),
        // oracle: xml._md_link("``[x]`` [y] `[z]`", None)
        ("``[x]`` [y] `[z]`", None, false, "[``[x]`` \\[y\\] `[z]`]"),
    ];
    for (text, url, image, expected) in cases {
        assert_eq!(md_link(text, url, image), expected, "{text:?} {url:?}");
    }
}

#[test]
fn md_link_backtick_runs_stay_linear() {
    // runs of strictly increasing length never close, each failing run once
    let text: String = (1..=2000).map(|n| format!("{} ", "`".repeat(n))).collect();
    assert_eq!(md_link(&text, None, false), format!("[{text}]"));
}

#[test]
fn code_fence_and_span_match_python() {
    // (text, code_fence min 1, code_fence min 3, code_span)
    let cases = [
        // oracle: xml._code_fence("a``b"), xml._code_fence("a``b", 3), xml._code_span("a``b")
        ("a``b", "```", "```", "```a``b```"),
        // oracle: xml._code_fence("`x"), xml._code_fence("`x", 3), xml._code_span("`x")
        ("`x", "``", "```", "`` `x ``"),
        // oracle: xml._code_fence("x`"), xml._code_fence("x`", 3), xml._code_span("x`")
        ("x`", "``", "```", "`` x` ``"),
        // oracle: xml._code_fence(""), xml._code_fence("", 3), xml._code_span("")
        ("", "`", "```", "``"),
        // oracle: xml._code_fence("```"), xml._code_fence("```", 3), xml._code_span("```")
        ("```", "````", "````", "```` ``` ````"),
        // oracle: xml._code_fence("a`b``c```d"), xml._code_fence("a`b``c```d", 3), xml._code_span("a`b``c```d")
        ("a`b``c```d", "````", "````", "````a`b``c```d````"),
    ];
    for (text, fence, fence_min_3, span) in cases {
        assert_eq!(code_fence(text, 1), fence, "{text:?}");
        assert_eq!(code_fence(text, 3), fence_min_3, "{text:?}");
        assert_eq!(code_span(text), span, "{text:?}");
    }
}

#[test]
fn md_wrap_python_whitespace() {
    // oracle: xml._md_wrap("\x1c a \x1f", "**")
    assert_eq!(
        md_wrap("\u{1c} a \u{1f}", "**", None),
        "\u{1c} **a** \u{1f}"
    );
    // oracle: xml._md_code("\x85x\u3000")
    assert_eq!(md_code("\u{85}x\u{3000}"), "\u{85}`x`\u{3000}");
    // oracle: xml._md_wrap("  ", "*")
    assert_eq!(md_wrap("  ", "*", None), "  ");
    // oracle: xml._md_wrap(" a b ", "<u>", "</u>")
    assert_eq!(md_wrap(" a b ", "<u>", Some("</u>")), " <u>a b</u> ");
    // oracle: xml._md_code(" `a` ")
    assert_eq!(md_code(" `a` "), " `` `a` `` ");
    // oracle: xml._md_code("  ")
    assert_eq!(md_code("  "), "  ");
    // oracle: xml._md_wrap("x", "~~", None)
    assert_eq!(md_wrap("x", "~~", None), "~~x~~");
}

#[test]
fn escape_cell_escapes_only_in_a_cell() {
    // upstream: xml.py::_escape_cell, `text.replace("|", "\\|").replace("\n", " ")`
    assert_eq!(escape_cell("a|b\nc", true), "a\\|b c");
    assert_eq!(escape_cell("a|b\nc", false), "a|b\nc");
    assert_eq!(escape_cell("plain", true), "plain");
}

/// Upstream's `_escape_inline_specials`: one `\\*c` substitution per character, in order.
fn escape_inline_specials_in_seven_passes(text: &str) -> String {
    let mut text = text.to_string();
    for ch in INLINE_SPECIAL_CHARS {
        if !text.contains(ch) {
            continue;
        }
        let pattern = Regex::new(&format!(r"\\*{}", regex::escape(&ch.to_string()))).unwrap();
        text = pattern
            .replace_all(&text, |m: &regex::Captures<'_>| {
                let run = m[0].matches('\\').count();
                format!("{}{ch}", "\\".repeat(2 * run + 1))
            })
            .into_owned();
    }
    text
}

#[test]
fn escape_inline_specials_single_pass_equals_seven() {
    // every sequence of one to three specials, each behind 0 to 3 backslashes
    let atoms: Vec<String> = INLINE_SPECIAL_CHARS
        .iter()
        .flat_map(|ch| (0..=3).map(move |run| format!("{}{ch}", "\\".repeat(run))))
        .collect();
    let mut texts: Vec<String> = Vec::new();
    for first in &atoms {
        texts.push(first.clone());
        for second in &atoms {
            texts.push(format!("{first}{second}"));
            for third in &atoms {
                texts.push(format!("{first}a{second}{third}\\"));
                texts.push(format!("{first}{second}{third}"));
            }
        }
    }
    for text in &texts {
        assert_eq!(
            escape_inline_specials(text),
            escape_inline_specials_in_seven_passes(text),
            "{text:?}"
        );
    }
    assert_eq!(escape_inline_specials("a\\b"), "a\\b");
}
