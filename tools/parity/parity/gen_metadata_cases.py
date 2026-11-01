"""Generates `src/json_metadata/cases.rs` (`json_cases`) and `tests/metadata_cases/cases.rs` (`cases`): what upstream returns on fixed inputs.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_metadata_cases import json_cases; json_cases()' > src/json_metadata/cases.rs
    scripts/oracle.sh 'from parity.gen_metadata_cases import cases; cases()' > tests/metadata_cases/cases.rs

`JSON_CASES` run `extract_json(json.loads(text, strict=False), Document())`. The `Document` is a
recording document that applies deviation D-19: a value that is neither a string nor `None` is not
stored in `title`, `author`, `sitename` or `pagetype`, and a list item of `categories` that is not a
string is dropped; `d19` says one of those happened. The expected values are what the rest of upstream's
code then does with that document, so a case holds the D-19 result, not the Python one.

A script with a lone surrogate is left out of the cases because a Rust string cannot hold one, and page text
decoded from bytes cannot carry one.

A backslash in an input is written `^` in this file and replaced by `bs`, so that no source line holds a
backslash-u escape that a tool might rewrite.
"""

import ast
import json
import os
import re
import sys
from pathlib import Path

from parity.gen_xpath_cases import rust_str as ascii_rust_str

SCHEMA = "https://schema.org"
STRING_FIELDS = ("title", "author", "sitename", "pagetype")


class SurrogateError(ValueError):
    "A value holds a lone surrogate, which no Rust string literal can."


def rust_str(text: str) -> str:
    if any(0xD800 <= ord(c) <= 0xDFFF for c in text):
        raise SurrogateError(repr(text[:40]))
    return ascii_rust_str(text)


def version() -> str:
    return f"Python {sys.version.split()[0]}"


def bs(text: str) -> str:
    return text.replace("^", chr(92))


def uni(*names: str) -> str:
    import unicodedata

    return "".join(unicodedata.lookup(name) for name in names)


def recording_document():
    """A `trafilatura.settings.Document` with the D-19 rule applied at assignment."""
    from trafilatura.settings import Document

    class Recording(Document):
        def __init__(self):
            object.__setattr__(self, "d19", False)
            super().__init__()

        def __setattr__(self, name, value):
            if name in STRING_FIELDS and value is not None and not isinstance(value, str):
                object.__setattr__(self, "d19", True)
                return
            if name == "categories" and value is not None:
                kept = [item for item in value if isinstance(item, str)]
                if len(kept) != len(value):
                    object.__setattr__(self, "d19", True)
                value = kept
            super().__setattr__(name, value)

    return Recording()


def option(text: str | None) -> str:
    return "None" if text is None else f"Some({rust_str(text)})"


def categories(items: list[str] | None) -> str:
    return "None" if items is None else f"Some(&[{', '.join(rust_str(item) for item in items)}])"


def dumps(value) -> str:
    return json.dumps(value)


def schema(**fields) -> dict:
    return {"@context": SCHEMA, **fields}


def article(**fields) -> dict:
    return schema(**{"@type": "Article"}, **fields)


def upstream_scripts() -> list[str]:
    """Every `application/ld+json` script in the string constants of upstream's JSON-LD tests."""
    path = Path(os.environ["TRAFILATURA_UPSTREAM"]) / "tests" / "json_metadata_tests.py"
    found: list[str] = []
    for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            for match in re.finditer(r'<script[^>]*type="application/ld\+json"[^>]*>(.*?)</script>', node.value, re.S):
                if match[1] not in found:
                    found.append(match[1])
    return found


AUTHOR_VALUES = [
    "Jane Doe",
    "",
    "John Doe; Jane Smith",
    "http://example.com/jane",
    "Jane Doe and John Smith",
    dumps({"name": "Jane Doe"}),
    dumps([{"name": "Ann Lee"}, {"@type": "Person", "name": "Bob Ray"}]),
    dumps("Quoted Name"),
    "123",
    "null",
    "true",
    "[]",
    "0",
    "[1]",
    '["John Doe", "Jane Smith"]',
    '"Jane\x01Doe"',
    "[" + "1" * 5000 + "]",
    "1" * 4300,
    "[" + "1" * 4300 + "]",
    {"name": "Jane Doe"},
    {"name": ["Jane", "Doe"]},
    {"name": ["Ann Lee; ", " Bob Ray;"]},
    {"name": []},
    {"name": ["Jane", 1]},
    {"name": {"name": "Jane Doe"}},
    {"name": {"name": 5}},
    {"name": {"other": "Jane Doe"}},
    {"name": 5},
    {"name": None},
    {"name": ""},
    {"givenName": "Jane", "familyName": "Doe"},
    {"givenName": "Jane", "additionalName": "Quinn", "familyName": "Doe"},
    {"givenName": "Jane", "familyName": 5},
    {"givenName": "Jane", "additionalName": None, "familyName": "Doe"},
    {"givenName": "Jane"},
    {"@type": "Person", "givenName": "Ann", "familyName": "Lee"},
    {"@type": ["Person"], "name": "Jane Doe"},
    {"@type": ["Thing", "Person"], "name": "Jane Doe"},
    {"@type": "Organization", "name": "Example News"},
    {"@type": [], "name": "Jane Doe"},
    {"@type": None, "name": "Jane Doe"},
    {"@type": 5, "name": "Jane Doe"},
    {"@type": {"a": 1}, "name": "Jane Doe"},
    {"@type": ["Person", 1], "name": "Jane Doe"},
    {},
    [],
    ["Jane Doe", "John Smith"],
    ["Jane Doe", {"name": "John Smith"}, ""],
    [["name"]],
    [["givenName", "familyName"]],
    [["givenName"]],
    [["@type"]],
    [["x"]],
    [[]],
    [None],
    [1],
    [True],
    [0],
    5,
    True,
    0,
    None,
]


def author_cases() -> list[str]:
    texts = []
    for value in AUTHOR_VALUES:
        texts.append(dumps(article(author=value)))
        texts.append(dumps(article(author=[value])))
    texts.append(dumps(schema(**{"@type": "Person", "name": "Jane Doe"}, author="Ann Lee")))
    texts.append(dumps(schema(**{"@graph": [{"@type": "Person", "name": "Jane Doe"}, article(author="Ann Lee")]})))
    texts.append(dumps(article(author="Ann Lee", name="Title A", headline="Title B")))
    return texts


def type_cases() -> list[str]:
    types = [
        "NewsArticle",
        ["NewsArticle"],
        ["NewsArticle", "Thing"],
        ["Thing", "NewsArticle"],
        ["CreativeWork", "Article"],
        ["Thing", "nEwSaRtIcLe"],
        "ARTICLE",
        "Person",
        ["Person"],
        ["Thing", "Person"],
        ["Thing", "Organization"],
        ["Thing", "WebSite"],
        ["Thing", "FAQPage"],
        ["Person", "Article"],
        ["Article", "Person"],
        ["WebPage", "NewsArticle"],
        ["NewsArticle", "WebPage"],
        ["TechArticle", "Article"],
        ["NewsArticle", None],
        ["Thing", "Unknown"],
        ["Thing", None],
        "Thing",
        [],
        None,
        "",
        0,
        1,
        True,
        False,
        {},
        {"a": 1},
        [1],
        [1, "Article"],
        [["Article"]],
        [None, "Article"],
        [0, "WebPage"],
        "JobPosting",
        "Blog",
        "LiveBlogPosting",
        "SocialMediaPosting",
        "NewsMediaOrganization",
        "Organization",
        "webpage",
        "WEBSITE",
        "ImageObject",
    ]
    texts = []
    for node_type in types:
        texts.append(
            dumps(
                schema(
                    **{"@type": node_type},
                    name="Example Name",
                    headline="Example Headline",
                    articleSection="News",
                    author={"@type": "Person", "name": "Nested Author"},
                    publisher={"@type": "Organization", "name": "Example News"},
                )
            )
        )
        texts.append(dumps(schema(**{"@type": node_type}, name="Example Name", legalName="Legal Name")))
    return texts


def sitename_cases() -> list[str]:
    publishers = [
        "the name of co",
        {"name": "Acme"},
        {"name": ""},
        {"name": 5},
        {"name": ["Acme"]},
        {"name": None},
        {},
        {"@type": "Organization", "name": "Example News"},
        {"name": "http://example.com"},
        [],
        None,
        5,
    ]
    texts = [dumps(schema(publisher=value)) for value in publishers]
    texts += [dumps(article(publisher=value)) for value in publishers]
    org = lambda **fields: {"@type": "Organization", **fields}  # noqa: E731
    sets = [
        [org(name="AB"), org(name="ABCD Corp")],
        [org(name="ABCD Corp"), org(name="AB")],
        [org(name="AB"), {"@type": "WebPage", "name": "ABCDEFGH"}],
        [{"@type": "WebPage", "name": "AB"}, {"@type": "WebPage", "name": "ABCDEFGH"}],
        [org(name="http://example.com"), org(name="Example")],
        [org(name="Example"), org(name="http://example.com")],
        [org(name=""), org(legalName="Legal")],
        [org(name=0, legalName="Legal")],
        [org(name=5, legalName="Legal")],
        [org(name=["List"], legalName="Legal")],
        [org(name={"a": 1})],
        [org(legalName="", alternateName="Alt")],
        [org(alternateName="Alt Name")],
        [org(name=None, legalName=None, alternateName=None)],
        [org(name="Org A", publisher={"name": "Pub B Long"})],
        [{"publisher": {"name": "Short"}}, {"publisher": {"name": "Much Longer Name"}}],
        [{"publisher": {"name": "Much Longer Name"}}, {"publisher": {"name": "Short"}}],
    ]
    texts += [dumps(schema(**{"@graph": items})) for items in sets]
    return texts


def title_category_cases() -> list[str]:
    sections = ["News", "", ["News", "Politics"], [], ["", "News"], [0, "News"], ["News", 1, ["x"], {"a": 1}, True],
                {"News": 1, "": 2, "Politics": 3}, {}, 5, None, True, 0, [None], [[]], [{"a": 1}]]
    texts = [dumps(article(articleSection=value)) for value in sections]
    texts.append(dumps(schema(**{"@graph": [article(articleSection=["A"]), article(articleSection=["B"])]})))
    texts.append(dumps(schema(**{"@graph": [article(articleSection=[]), article(articleSection=["B"])]})))
    texts.append(dumps(schema(**{"@graph": [article(articleSection=["x", 1]), article(articleSection="B")]})))
    titles = [
        {"name": "Name Title"},
        {"headline": "Headline Title"},
        {"name": "Name Title", "headline": "Headline Title"},
        {"name": 5, "headline": "Headline Title"},
        {"name": None, "headline": "Headline Title"},
        {"name": "", "headline": "Headline Title"},
        {"headline": ""},
        {"headline": 5},
        {"headline": None},
        {"headline": ["x"]},
        {"headline": {"a": 1}},
        {"name": {"a": 1}},
        {"name": ["x"]},
        {"name": True},
    ]
    for fields in titles:
        texts.append(dumps(article(**fields)))
        texts.append(dumps(schema(**{"@type": "NewsArticle"}, **fields)))
    texts.append(dumps(schema(**{"@graph": [article(headline=5), article(headline="Second")]})))
    texts.append(dumps(schema(**{"@graph": [article(headline=""), article(headline="Second")]})))
    texts.append(dumps(schema(**{"@graph": [article(headline="First"), article(headline="Second")]})))
    texts.append(dumps(schema(**{"@graph": [article(headline=None), article(headline=None, name="Z")]})))
    return texts


def shape_cases() -> list[str]:
    """Wrappers, `@context` forms, container and item types."""
    news = {"@type": "NewsArticle", "headline": "Wrapped Headline", "author": {"name": "Jane Doe"}}
    texts = [
        dumps([schema(**news)]),
        dumps(schema(**{"@graph": [news]})),
        dumps(schema(**{"@graph": news})),
        dumps(schema(**{"@graph": None})),
        dumps(schema(**{"@graph": "text"})),
        dumps(schema(**{"@graph": ""})),
        dumps(schema(**{"@graph": []})),
        dumps(schema(**{"@graph": [news, news]}, headline="Outer", **{"@type": "Article"})),
        dumps(schema(**{"@type": "LiveBlogPosting", "liveBlogUpdate": [news]})),
        dumps(schema(**{"@type": "LiveBlogPosting", "liveBlogUpdate": news})),
        dumps(schema(**{"@type": "liveblogposting", "liveBlogUpdate": [news], "headline": "Outer"})),
        dumps(schema(**{"@type": "LIVEBLOGPOSTING", "liveBlogUpdate": None, "headline": "Outer"})),
        dumps(schema(**{"@type": "LiveBlogPosting", "headline": "No update"})),
        dumps(schema(**{"@type": ["LiveBlogPosting"], "liveBlogUpdate": [news], "headline": "Outer"})),
        dumps(schema(**{"@type": "Article", "liveBlogUpdate": [news], "headline": "Outer"})),
        dumps(schema(**{"@type": "LiveBlogPosting", "liveBlogUpdate": [news]}, **{"@graph": [news]})),
        dumps(news),
        dumps({"@type": "NewsArticle", "headline": "No Context"}),
        dumps({"@context": "", "@type": "NewsArticle", "headline": "Empty Context"}),
        dumps({"@context": "http://schema.org", "@type": "NewsArticle", "headline": "Http Context"}),
        dumps({"@context": "http://schema.org/", "@type": "NewsArticle", "headline": "Slash Context"}),
        dumps({"@context": "https://SCHEMA.org/", "@type": "NewsArticle", "headline": "Upper Context"}),
        dumps({"@context": "HTTP://SCHEMA.ORG/x", "@type": "NewsArticle", "headline": "Upper Http"}),
        dumps({"@context": "ftp://schema.org", "@type": "NewsArticle", "headline": "Ftp Context"}),
        dumps({"@context": "https://schema.org.example", "@type": "NewsArticle", "headline": "Longer Host"}),
        dumps({"@context": " https://schema.org", "@type": "NewsArticle", "headline": "Space Context"}),
        dumps({"@context": "https://sch\N{LATIN SMALL LETTER LONG S}ema.org", "@type": "NewsArticle", "headline": "Long S"}),
        dumps({"@context": ["https://schema.org"], "@type": "NewsArticle", "headline": "List Context"}),
        dumps({"@context": {"@vocab": SCHEMA}, "@type": "NewsArticle", "headline": "Object Context"}),
        dumps({"@context": 5, "@type": "NewsArticle", "headline": "Number Context"}),
        dumps({"@context": None, "@type": "NewsArticle", "headline": "Null Context"}),
        dumps([news, schema(**news), {"@context": "http://schema.org", **news}]),
        dumps([schema(**{"@type": "Article", "headline": "First"}), schema(**{"@type": "Article", "headline": "Second"})]),
        dumps([]),
        dumps({}),
        dumps([{}]),
        dumps([schema()]),
        "null",
        "[null]",
        "[0]",
        "[1]",
        "[true]",
        '[""]',
        '["text"]',
        "[[]]",
        "[[1]]",
        '"text"',
        "123",
        "true",
        "0",
        dumps([schema(**{"@type": "Article", "headline": "Before"}), 5]),
        dumps([5, schema(**{"@type": "Article", "headline": "After"})]),
        dumps(schema(**{"@graph": [0, False, None, "", [], {}, news]})),
        dumps(schema(**{"@graph": [news, 1]})),
        dumps(schema(**{"@graph": [news, "text"]})),
        dumps(schema(**{"@graph": [news, [1]]})),
        dumps(schema(**{"@graph": [news, True]})),
        '{"@context":"https://schema.org","@graph":[0,1e-400,-0,0.0,0e5,{"@type":"Article","headline":"Zero Items"}]}',
        '{"@context":"https://schema.org","@graph":[NaN,{"@type":"Article","headline":"Nan Item"}]}',
        '{"@context":"https://schema.org","@graph":[Infinity]}',
        '{"@context":"https://schema.org","@graph":[0.5]}',
        '{"@context":"https://schema.org","@type":"Article","headline":"Raw\x01Control"}',
        '{"@context":"https://schema.org","@type":"Article","headline":"Dup","headline":"Last Duplicate"}',
        '{"@context":"https://schema.org","@type":"Article","author":"[\\"John Doe\\"]"}',
    ]
    texts.append('{"@context":"https://schema.org","@type":"Article","name":"Number Name","headline":1e-400}')
    texts.append('{"@context":"https://schema.org","@type":"Article","articleSection":[1e-400,0,"A"]}')
    texts.append(
        dumps(schema(**{"@type": "Article", "author": {"@type": "Person", "name": "Jane Doe"}, "articleSection": "News"}))
    )
    return texts


def json_texts() -> list[str]:
    texts = []
    parse_errors = []
    for script in upstream_scripts():
        try:
            json.loads(script, strict=False)
        except json.JSONDecodeError:
            parse_errors.append(script)
        else:
            texts.append(script)
    print(f"upstream scripts: {len(texts)} parse, {len(parse_errors)} do not", file=sys.stderr)
    with_surrogate = [text for text in texts if any(0xD800 <= ord(c) <= 0xDFFF for c in text)]
    print(f"upstream scripts with a lone surrogate, left out: {len(with_surrogate)}", file=sys.stderr)
    texts = [text for text in texts if text not in with_surrogate]
    texts += shape_cases() + author_cases() + type_cases() + sitename_cases() + title_category_cases()
    return list(dict.fromkeys(texts))


def run_json(text: str) -> tuple[bool, object]:
    from trafilatura.json_metadata import extract_json

    document = recording_document()
    raised = False
    try:
        extract_json(json.loads(text, strict=False), document)
    except json.JSONDecodeError as error:
        raise SystemExit(f"JSON_CASES text is not JSON: {text[:80]!r}: {error}")
    except Exception:
        raised = True
    return raised, document


def json_case_lines() -> list[str]:
    lines = []
    raised_count = d19_count = 0
    for text in json_texts():
        raised, document = run_json(text)
        raised_count += raised
        d19_count += document.d19
        lines.append(
            "    JsonCase { "
            + f"text: {rust_str(text)}, raised: {str(raised).lower()}, title: {option(document.title)}, "
            + f"author: {option(document.author)}, sitename: {option(document.sitename)}, "
            + f"pagetype: {option(document.pagetype)}, categories: {categories(document.categories)}, "
            + f"d19: {str(document.d19).lower()}"
            + " },"
        )
    print(f"json cases: {len(lines)}, {raised_count} raise, {d19_count} hit D-19", file=sys.stderr)
    return lines


def parse_error_inputs() -> list[str]:
    texts = []
    for script in upstream_scripts():
        texts += [script, script[: len(script) * 7 // 10]]
    texts = [text for text in texts if not any(0xD800 <= ord(c) <= 0xDFFF for c in text)]
    texts += [bs(text) for text in [
        '{"@context":"https://schema.org","@type":"Article","headline":"Cut Headline',
        '{"@type":"Article", "name": "Spaced Name","headline":"H"}',
        '{"@type":"Article","name":"Escaped ^u00e9 Name ^"quoted^" x"}',
        '{"@type":"Article","headline":"Tag <b>bold</b> ^u0026amp; more"}',
        '{"@type":"Article","headline":"^ud83d^ude00 only"}',
        '{"@type":"Article","headline":"Entity &#233; &amp; end"}',
        '{"@type":"Article","headline":"Entity ^&#233;"}',
        '{"@type":"NewsArticle" , "headline" : "Loose"}',
        '{"@type"  :  "NewsArticle"}',
        '{"@type":"newsarticle"}',
        '{"@type":"NEWSARTICLE"}',
        '{"@type":"WebSite"}',
        '{"@type":"Person"}',
        '{"@type":"N^u0065wsArticle"}',
        '{"@type":""}',
        '{"@type":"X"',
        '"@type" only',
        '{"publisher":{"name":"Pub Name"},"headline":',
        '{"publisher": {"@type":"Organization","name":"Pub, Name"}',
        '{"publisher":{"name":"Pub ^"Q^" Name"}',
        '{"publisher":{"name":"http://example.com"}',
        '{"publisher":{"name":"Pub ^u00e9"}',
        '{"publisher":{"name":"Pub"},"name":"x"} {"publisher":{"name":"Longer Pub"}',
        '{"articleSection": "Section A","x":',
        '{"articleSection":"Sec ^u00e9 B"',
        '{"articleSection": "A^"B"',
        '{"articleSection":"News","articleSection":"Other"',
        '{"author":{"name":"Jane Doe"},"headline":"H',
        '{"author":{"names":"Jane Doe"},"x":',
        '{"author":{"name": "Jane Doe"},',
        '{"author":{"name^":"Jane Doe"},',
        '{"author":{"name^": ^"Jane Doe"},',
        '{"author":{"name^": ^"Jane Doe^"},',
        '{"author":{"nam":"Jane Doe"},',
        '{"author":{"name":"Cher"},"x":',
        '{"author":[{"name":"Jane Doe"},{"name":"John Smith"}],',
        '{"author":[{"name":"Jane Doe"},{"name":"Solo"},{"name":"John Smith"}],',
        '{"author":{"@type":"Person","name":"Jane Doe"},"x":',
        '{"author":{"@type":"Person","name":"Jane Doe"},"publisher":{"@type":"Organization","name":"Org One"},',
        '{"author":{"@type":"Organization","name":"Org One Two"},"x":',
        '{"author":{"@type":"ImageObject","url":"x","name":"Jane Doe"},"x":',
        '{"author":{"@type":"WebPage","url":"x"}},"x":',
        '{"author":{"name":"Jane Doe"},"@type":"Person","name":"John Smith"',
        '{"@type":"Person","name":"Lone Person"',
        '{"x":"Person","name":"Jane Doe"',
        '{"@type":"person","name":"Lower Person"',
        '{"@type":"Person","names":"Plural Person"',
        '{"@type":"Person",^n"name"^n:^n"Newline Person"',
        '{"author":{^n"name":^n"Jane Doe"^n},',
        '{"author":{"name":"Jane ^u00e9 Doe"},',
        '{"author":{"name":"Jane Doe"},"author":{"name":"John Smith"},',
        '{"author":{"names":"Plural Author"},',
        '{"author":{"name":"A B","names":"C D"},',
        '{"author":{"name":"By Jane Doe"},',
        '{"author":{"name":"jane doe"},',
        '{"author":{"name":"Jane Doe 123"},',
        '{"headline":"H"} {"@type":"Article","name":"Article Name"',
        '{"@type":"Article","name":"Article Name"',
        '{"@type":"Article", "name":"Article Name"',
        '{"@type":"article","name":"Lower Article Name"',
        '{"@type":"Article","name": "Article Name"',
        '{"@type": "Article","name":"Spaced Type"',
        '{"headline": "Spaced H"',
        '{"headline":"Esc ^u00e9"',
        '{"headline":"Esc ^"x"',
        '{"headline":""',
        '{"headline":"x"',
        '{"@type":"BlogPosting","headline":"Blog Headline"',
        '{"name":"Plain Name","headline":"Plain Headline"',
        '"headline" "x"',
        "",
        "{",
        "[",
        "}{",
        '"@type"',
        '"publisher"',
        '"articleSection"',
    ]]
    return list(dict.fromkeys(texts))


def parse_error_lines() -> list[str]:
    from trafilatura.json_metadata import extract_json_parse_error
    from trafilatura.settings import Document

    lines = []
    for text in parse_error_inputs():
        document = Document()
        extract_json_parse_error(text, document)
        lines.append(
            f"    ({rust_str(text)}, ParseErrorFields {{ author: {option(document.author)}, pagetype: {option(document.pagetype)}, "
            f"sitename: {option(document.sitename)}, categories: {categories(document.categories)}, title: {option(document.title)} }}),"
        )
    print(f"parse error cases: {len(lines)}", file=sys.stderr)
    return lines


def normalize_authors_inputs() -> list[tuple[str | None, str]]:
    smile = uni("SMILING FACE WITH SMILING EYES")
    dotted_i = uni("LATIN CAPITAL LETTER I WITH DOT ABOVE")
    dotless_i = uni("LATIN SMALL LETTER DOTLESS I")
    kelvin = uni("KELVIN SIGN")
    combining_acute = uni("COMBINING ACUTE ACCENT")
    superscript_two = uni("SUPERSCRIPT TWO")
    quote_left, quote_right = uni("LEFT SINGLE QUOTATION MARK"), uni("RIGHT SINGLE QUOTATION MARK")
    e_acute = uni("LATIN SMALL LETTER E WITH ACUTE")
    pairs: list[tuple[str | None, str]] = [(None, text) for text in [
        "John Doe", "abc", "Steve Steve 123", "By Steve Steve", "123", "^u00e9tienne", "Jane^uZZ Doe", "&#233;tienne",
        "Alice &amp; Bob", "<b>John Doe</b>", f"John {smile} Doe", "words by John Doe", "John Doe123", "John_Doe",
        "John Doe* ", "John Doe of John Doe", "John Doe \N{EM DASH} John Doe", 'John "The King" Doe',
        "written by Jane Doe", "Written By Jane Doe", "von Jane Doe", "VON jane doe", "By jane", "Words by X Y",
        "words X Y", "from Berlin", "edited by Jane Doe", "Published by Jane Doe", "Geschrieben von Jane",
        "Verfasst von Jane Doe", "Gesehent von Jane", "Bearbeitet Von Jane Doe", "By", "by ", "by x", "Byx Jane",
        "wr" + dotted_i + "tten by Jane Doe", "wr" + dotless_i + "tten by Jane Doe", kelvin + "ed by Jane Doe",
        "\N{LATIN SMALL LETTER SHARP S}t by Jane Doe", "\N{LATIN CAPITAL LETTER SHARP S}t by Jane Doe",
        "\N{LATIN CAPITAL LETTER A WITH DIAERESIS}ed von Jane Doe", "\N{LATIN SMALL LETTER LONG S}ed by Jane Doe",
        "jane@example.com", "Jane.Doe@Example.COM", "a@b.cc", "a@b.c", "_a@b.co", ".a@b.co", "a@b.cc" + e_acute,
        "a@b.cc_", "a@b.cc1", "a@b.cc " + e_acute, "a@b.cc.", "x@y.zz Jane Doe", "a@b.cc" + combining_acute,
        "a@b.cc" + superscript_two, "-a@b.co", "a-b@c-d.org", "a@b..cc", "a@b-.cc", e_acute + "a@b.cc", "1@2.co",
        "http://x.com", "HTTPS://X", "Http", "httpx name", "http", " http", "ht tp", "\N{FULLWIDTH LATIN SMALL LETTER H}ttp",
        "@jane_doe", "Jane Doe @jdoe", "John @handle Doe", "@", "John @ Doe", "@" + superscript_two + "x Jane",
        "John 'JJ' Doe", "Bob (Robert) Smith", "Jane [x] Doe", "Jane {x} Doe", f"John {quote_left}JJ{quote_right} Doe",
        f"John {quote_right}x{quote_right} Doe", "Jane ( Doe", "Jane ) Doe", 'Jane " Doe', 'Jane "a" Doe "b" Eve',
        "Jane (a) (b) Doe", "Jane (a\nb) Doe",
        "Jane \N{GRINNING FACE} Doe", "Jane \N{HEAVY CHECK MARK} Doe", "Jane \N{ROCKET} Doe", "Jane \N{SPARKLES} Doe",
        "Jane \N{BLACK SUN WITH RAYS} Doe", "Jane \N{FACE PALM}" + " Doe", "Jane \N{ZIPPER-MOUTH FACE} Doe",
        "Jane \N{WHITE HEAVY CHECK MARK} Doe", "\N{GRINNING FACE}", "Jane \N{COPYRIGHT SIGN} Doe", "Jane \N{SNOWMAN}",
        "A B and C D", "A B und C D", "A B & C D", "A B | C D", "A B / C D", "A B; C D", "A B, C D", "AND", "and Bob",
        "Bob and", "Bob And Jane", "Bob\nand\nJane", "Bob.and.Jane", "Sandy Anderson", "Brand and Co", "und",
        "Hans und Franz", "A B and\n", "A B\nand", "A B and C D and E F", "A B and and C D", "A B And C D", "A B AND C D",
        e_acute + "and b", superscript_two + "and b", combining_acute + "and b", "a" + combining_acute + " and Jane Doe",
        "Jane Doe and" + combining_acute, "x" + superscript_two + " and y", "A B &amp; C D", "A B &#38; C D",
        "A B and" + e_acute, "A B and_C D", "A B and1 C D", "A B and-C D", "A B,", ",", ";;", "/", "A B ,, C D",
        "John Doe of Acme", "John Doe at Acme", "John Doe - Acme", "John Doe \N{EN DASH} Acme", "John Doe \N{EM DASH} Acme",
        "John Doe in Berlin", "John Doe via Twitter", "John Doe VIA twitter", "John Doe V" + dotted_i + "A twitter",
        "John Doe w" + dotted_i + "th Jane", "John Doe " + dotted_i + "n Berlin", "John Doe " + dotless_i + "n Berlin",
        "John Doe on", "John Doe on ", "- in x", "a - in x", "John Doe " + superscript_two + " in X",
        "x" + combining_acute + " in y", "e" + combining_acute + " in X", "John-Doe in Berlin", "in Berlin", "am Rhein",
        "John Doe to the world", "John Doe from Mars", "John Doe am Rhein", "John Doe at", "John Doe at  ", "Jo in at x",
        "A in B on C", "A in B\nC on D", "A\tin\tB", "A\x1cin\x1cB", "A in\x1cB", "A" + superscript_two + " in B",
        "Jane\N{NO-BREAK SPACE}in\N{NO-BREAK SPACE}Berlin", "Jane Doe international", "Jane Doe inn Berlin",
        "Jane Doe \N{LATIN CAPITAL LETTER I WITH DOT ABOVE}N Berlin", "A \N{HYPHEN} B", "A - B - C",
        "John 5", "John 55", "John5", "5 John", "John \N{ARABIC-INDIC DIGIT ONE}\N{ARABIC-INDIC DIGIT TWO}",
        "John \N{ARABIC-INDIC DIGIT ONE}", "John " + superscript_two, "John " + superscript_two + "x", "1999",
        "John 5 and Jane 6", "John Doe 2nd", "Jane 5x", "Jane \N{FULLWIDTH DIGIT ONE}\N{FULLWIDTH DIGIT TWO}",
        "John Doe!", "John Doe?!", "John: Doe", "Doe, John", "John~Doe", "John Doe...", "John Doe!!!   ", "\N{INVERTED QUESTION MARK}Jane?",
        "John Doe -", "John Doe \N{EM DASH}", "(John Doe)", "John Doe$", "John#Doe", "John%Doe", "<John Doe>",
        "John Doe " + combining_acute, "John Doe \N{NO-BREAK SPACE}\N{EM DASH}", "Jane Doe." , "Jane Doe .", "Jane. Doe",
        "A" * 50, "A" * 49, "A" * 50 + " B", "a-" * 25, "a" * 25 + "-" + "b" * 24, e_acute * 50, e_acute * 49,
        "ab " * 20, "x" * 60 + "-" + "y", "x" * 100,
        "john doe", "jane doe", "\N{LATIN SMALL LETTER SHARP S}test", "\N{LATIN SMALL LETTER DZ WITH CARON}ungla",
        "\N{LATIN SMALL LETTER E WITH ACUTE}tienne dupont", "\N{LATIN CAPITAL LETTER I WITH DOT ABOVE}stanbul",
        "\N{GREEK SMALL LETTER SIGMA}\N{GREEK SMALL LETTER FINAL SIGMA}", "\N{GREEK CAPITAL LETTER SIGMA}\N{GREEK CAPITAL LETTER SIGMA}",
        "\N{ROMAN NUMERAL ONE} roman", "\N{LATIN CAPITAL LETTER DZ WITH CARON}ungla", "\N{LATIN CAPITAL LETTER D WITH SMALL LETTER Z WITH CARON}ungla",
        "x" + combining_acute + "y z", "o'neil mc'donald", "jean-luc picard", "mcdonald", "\N{DIGIT ONE}",
        "1", "\x0b", "x\x0by z", "John\x1cDoe", "John\N{ZERO WIDTH SPACE}Doe", "John\N{IDEOGRAPHIC SPACE}Doe",
        " ", "", "   ", "\t", "\n",
        "^u00e9tienne Dupont ^ud83d^ude00", "^u0041^u0042 Cd", "^ud83d^ude00", "Jane ^u00e9 Doe", "^u00zz", "^u12", "^x41bc", "^q",
        "trailing^", "^U0001f600 Jane", "^U00110000", "^n", "J^tDoe", "J^nDoe",
        "^u0026amp; Bob", "&#9731; Snow", "&#xD800; Bob Lee", "&#0; Zero", "&#128512; Jane Doe", "&amp;amp; Jane Doe",
        "&amp Jane", "&AMP; Bob Lee", "&#x41; Bob", "&#65 Jane",
        "<!-- c --> Jane Doe", "Jane <i>Doe</i>", "Jane < Doe", "Jane > Doe", "Jane <br/> Doe", "<>", "<Jane Doe", "Jane Doe>",
        "x <!-- a\nb --> y", "<!-- a --", "<!---->Jane Doe", "Jane <a href='x'>Doe</a>",
    ]]
    pairs += [
        ("Alice; Bob", "John Doe"), ("Alice; Bob", "john.doe@example.com"), ("Smith", "John Smith"),
        ("John Smith", "Smith"), ("A B; A B", "A B"), ("A B", "A B; C D; A B"), ("", "x y"), (";", "x y"),
        ("A; ; B", "C D"), ("Jane Doe; ", "John Smith"), ("Jane Doe", ""), ("  Jane  ", "123"), ("Jane Doe", "Jane Doe"),
        ("Jane", "Jane Doe"), ("Jane Doe", "Jane"), ("Jane Doe", "jane doe"), ("jane doe", "Jane Doe"), ("A", "B"),
        ("Ann Lee; Bob Ray", "Ann"), ("Ann Lee; Bob Ray", "Lee; Ray; New Person"), ("Ann Lee", "http://x"),
        ("Ann Lee", "a@b.cc"), (None, "Ann Lee; Bob Ray; Ann Lee"), ("Ann Lee;Bob Ray", "New One"),
        ("Ann Lee; Bob Ray", "Ann Lee; Bob Ray"), ("; Ann Lee", "New One"), ("Ann Lee; ", ""), ("A  B", "A B"),
        ("A", ";"), ("", ""), ("", "123"), (";", "123"), ("; ", ""), ("; ;", "a b"),
        ("ab", "abc"), ("abc", "ab"), ("a", "b; a b"), ("Jane Doe", "Doe; Jane"),
    ]
    return pairs


def normalize_authors_lines() -> list[str]:
    from trafilatura.json_metadata import normalize_authors

    lines = []
    skipped = 0
    for current, text in dict.fromkeys((c, bs(t)) for c, t in normalize_authors_inputs()):
        try:
            lines.append(f"    ({option(current)}, {rust_str(text)}, {option(normalize_authors(current, text))}),")
        except Exception as error:
            skipped += 1
            print(f"normalize_authors skipped: {text[:40]!r}: {error!r}", file=sys.stderr)
    print(f"normalize_authors cases: {len(lines)}, {skipped} skipped", file=sys.stderr)
    return lines


def normalize_json_inputs() -> list[str]:
    e_acute = uni("LATIN SMALL LETTER E WITH ACUTE")
    return [
        "Test ^nthis", "^uD800^uDC00Hello^uDBFF^uDFFF", "Test \N{LEFT BLACK LENTICULAR BRACKET}ABC\N{RIGHT BLACK LENTICULAR BRACKET}",
        "Se\N{LATIN SMALL LETTER A WITH ACUTE}n Federico O'Murch\N{LATIN SMALL LETTER U WITH ACUTE}",
        "^u00e9tienne Dupont ^ud83d^ude00", "a^tb^rc^nd", "&amp;^u0026amp;", "<b>x</b>^u003cb^u003ey", "<!-- c --> x",
        "x <!-- a\nb --> y", "  spaced   out ", "&#233;", "^&#233;", "^u00e9^&amp;", "^^n", "^u0041^u0042", "^U0041", "^u00zz",
        "^ud83d^ude00x", "x&#128512;^", "&lt;b&gt;^", "^", "^^", "^^u0041", "^u004", "^u0041^u004", "plain", "", " ", "\t^n\t",
        "a\x1cb", "a\N{NO-BREAK SPACE}b ^n", "Caf" + e_acute + " ^u00e9", "^u00A0x^u00a0", "^u0000x", "^u0009y", "^u2028z",
        "^uDBFF^uDFFFx^uD800", "^uFFFF", "^ud7ff^ue000", "Tag <a href=\"x\">link</a>^n", "<a\nhref='x'>y</a>^t",
        "<", ">", "<>", "< >", "a < b > c^n", "^&#x41;", "^&#0;", "^&#xD800;", "^&notit;", "^&amp", "^&AMP;", "^&#65",
        "^u0026#65;", "^u0026lt;b^u0026gt;", "^u003c!-- x --^u003e", "x ^u003c!-- x --^u003e y", "a^n^n b", "a^\nb",
        "newsarticle", "NewsArticle".lower(), "Spa^u0020ce", "^u00e9" * 3, "^u00E9^u00C9",
    ]


def normalize_json_lines() -> list[str]:
    from trafilatura.json_metadata import normalize_json

    lines = []
    for text in dict.fromkeys(bs(t) for t in normalize_json_inputs()):
        try:
            lines.append(f"    ({rust_str(text)}, {rust_str(normalize_json(text))}),")
        except Exception as error:
            print(f"normalize_json skipped: {text[:40]!r}: {error!r}", file=sys.stderr)
    print(f"normalize_json cases: {len(lines)}", file=sys.stderr)
    return lines


def json_cases() -> None:
    out = [
        f"// @generated by tools/parity/parity/gen_metadata_cases.py with {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_metadata_cases import json_cases; json_cases()' > src/json_metadata/cases.rs",
        "// Upstream's `extract_json`, `extract_json_parse_error`, `normalize_authors` and `normalize_json` run on fixed inputs.",
        "",
        "/// A JSON-LD text and the `Document` upstream's `extract_json` leaves after `json.loads(text, strict=False)`. `raised` says",
        "/// an exception stopped it; the fields are what had been set by then. `d19` says upstream stored a value that is not a",
        "/// string (or a non-string list item), so the fields hold the D-19 result.",
        "pub(super) struct JsonCase {",
        "    pub text: &'static str,",
        "    pub raised: bool,",
        "    pub title: Option<&'static str>,",
        "    pub author: Option<&'static str>,",
        "    pub sitename: Option<&'static str>,",
        "    pub pagetype: Option<&'static str>,",
        "    pub categories: Option<&'static [&'static str]>,",
        "    pub d19: bool,",
        "}",
        "",
        "/// The fields `extract_json_parse_error` sets on an empty `Document`.",
        "pub(super) struct ParseErrorFields {",
        "    pub author: Option<&'static str>,",
        "    pub pagetype: Option<&'static str>,",
        "    pub sitename: Option<&'static str>,",
        "    pub categories: Option<&'static [&'static str]>,",
        "    pub title: Option<&'static str>,",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub(super) const JSON_CASES: &[JsonCase] = &[",
        *json_case_lines(),
        "];",
        "",
        "#[rustfmt::skip]",
        "pub(super) const PARSE_ERROR_CASES: &[(&str, ParseErrorFields)] = &[",
        *parse_error_lines(),
        "];",
        "",
        "/// Current authors, the author string and `normalize_authors` of them.",
        "#[rustfmt::skip]",
        "pub(super) const NORMALIZE_AUTHORS_CASES: &[(Option<&str>, &str, Option<&str>)] = &[",
        *normalize_authors_lines(),
        "];",
        "",
        "/// An input and `normalize_json` of it.",
        "#[rustfmt::skip]",
        "pub(super) const NORMALIZE_JSON_CASES: &[(&str, &str)] = &[",
        *normalize_json_lines(),
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# The `Document` fields a `MetadataCase` holds, in upstream slot order; `filedate` (copied from the config) is
# left out.
CASE_STRING_FIELDS = (
    "title", "author", "url", "hostname", "description", "sitename", "fingerprint", "id", "license",
    "comments", "raw_text", "text", "language", "image", "pagetype", "date",
)
MAX_DATE = "2024-01-31"


def page(head: str = "", body: str = "") -> str:
    return f"<html><head>{head}</head><body>{body}</body></html>"


def ld(**fields) -> str:
    return f'<script type="application/ld+json">{dumps(fields)}</script>'


def metadata_snippets() -> list[tuple[str, str | None, tuple[str, ...]]]:
    "Pages with a default URL and an author blacklist, one for each branch of `extract_metadata`."
    long_title = "a" * 30000
    long_description = "\N{LATIN SMALL LETTER E WITH ACUTE}" * 10001
    author = lambda name: f'<meta name="author" content="{name}">'  # noqa: E731
    cats = (
        '<div class="postinfo"><a href="/category/news">News</a></div>'
        '<div class="tags"><a href="/tag/rust">Rust</a> <a href="/tags/web">Web</a> <a href="/tag/rust">Rust</a></div>'
    )
    raising = (
        '<script type="application/ld+json">{"@context": "https://schema.org", "@graph": '
        '[{"@type": "NewsArticle", "headline": "Wrapped Headline", "author": {"name": "Jane Doe"}}, 1]}</script>'
    )
    article_json = ld(**article(author={"name": "Json Author"}, headline="Json Title"))
    return [
        (page(), None, ()),
        (page(), "https://www.example.org/start", ()),
        (page(author("Jane")), None, ()),
        (page(author("Jane Doe")), None, ()),
        (page(author("Jane Doe")), None, ("Jane Doe",)),
        (page(author("Jane Doe; John Smith")), None, ("Jane Doe",)),
        (page(author("Jane Doe"), '<a rel="author">John Smith</a>'), None, ("Jane Doe",)),
        (page("", '<a rel="author">Jane Doe</a>'), None, ("Jane Doe",)),
        (page("", '<a rel="author">Jane Doe</a>'), None, ("Other Person",)),
        (page("", '<a rel="author">Jane</a>'), None, ()),
        (page('<meta property="og:url" content="https://example.org/og">'), "https://default.example/x", ()),
        (page('<link rel="canonical" href="https://www.example.co.uk/canonical/page">'), "https://default.example/x", ()),
        (page(), "https://default.example/x", ()),
        (page(), "default.example.co.uk/page", ()),
        (page(), "example.org", ()),
        (page(), "http://localhost/page", ()),
        (page(), "http://192.168.0.1/page", ()),
        (page(), "https://w3.example.org/page", ()),
        (page('<meta property="og:site_name" content="@x">'), None, ()),
        (page('<meta property="og:site_name" content="@">'), "https://www.example.org/a", ()),
        (page('<meta property="og:site_name" content="mysite">'), None, ()),
        (page('<meta property="og:site_name" content="mySITE name">'), None, ()),
        (page('<meta property="og:site_name" content="my-site.com">'), None, ()),
        (page('<meta property="og:site_name" content="Already Upper">'), None, ()),
        (page('<meta property="og:site_name" content="\N{LATIN SMALL LETTER SHARP S}ite">'), None, ()),
        (page("<title>Some headline | example.com</title>"), "https://www.example.org/a", ()),
        (page("<title>Some headline</title>"), "https://www.example.org/a", ()),
        (page(ld(**article(articleSection="Sports")), cats), None, ()),
        (page(ld(**article(articleSection=["Sports", "Golf"])), cats), None, ()),
        (page("", cats), None, ()),
        (page('<meta property="article:section" content="Politics">'), None, ()),
        (page('<meta property="article:section" content="Politics">', cats), None, ()),
        (page("", '<a href="/tag/">empty</a>'), None, ()),
        (page(f'<meta property="og:title" content="{long_title}">'), None, ()),
        (page(f"<title>{long_title}</title>"), None, ()),
        (page(f'<meta name="description" content="{long_description}">'), None, ()),
        (page('<meta name="description" content="Tom &amp;amp; Jerry &#233; &amp;nbsp;">'), None, ()),
        (page('<meta name="description" content="  &amp;nbsp;  ">'), None, ()),
        (page('<meta property="og:title" content="A &amp;amp; B">'), None, ()),
        (page(raising), None, ()),
        (page(raising + '<meta name="author" content="Meta Author">'), None, ()),
        (page(article_json), None, ()),
        (page('<meta property="og:title" content="Meta Title">' + ld(**article(headline="Json Title"))), None, ()),
        (page("", '<footer><a href="https://creativecommons.org/licenses/by-nc/4.0/">x</a></footer>'), None, ()),
        (page('<link rel="license" href="https://creativecommons.org/licenses/by/4.0/">'), None, ()),
        (page("", '<a rel="license" href="https://creativecommons.org/licenses/by-sa/4.0/">CC</a>'), None, ()),
        (page('<meta property="og:image" content="https://example.org/i.png"><meta property="og:type" content="article"><meta name="description" content="Desc">'), None, ()),
    ]


def run_metadata(html: str, default_url: str | None, blacklist: tuple[str, ...]):
    from parity import html5tree
    from trafilatura.metadata import extract_metadata

    tree = html5tree.parse_document(html)
    assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {html[:80]!r}"
    date_config = {"original_date": True, "extensive_search": False, "max_date": MAX_DATE}
    return extract_metadata(tree, default_url, date_config, author_blacklist=set(blacklist))


def cases() -> None:
    from parity.gen_htmldate_cases import oracle_setup
    from parity.generate import html5_parsers

    oracle_setup()

    lines = []
    with html5_parsers():
        for html, default_url, blacklist in metadata_snippets():
            document = run_metadata(html, default_url, blacklist)
            strings = ", ".join(f"{name}: {option(getattr(document, name))}" for name in CASE_STRING_FIELDS)
            blacklisted = ", ".join(rust_str(name) for name in blacklist)
            lines.append(
                f"    MetadataCase {{ html: {rust_str(html)}, default_url: {option(default_url)}, author_blacklist: &[{blacklisted}], "
                f"{strings}, categories: {categories(document.categories)}, tags: {categories(document.tags)} }},"
            )
    print(f"metadata cases: {len(lines)}", file=sys.stderr)
    fields = [f"    pub {name}: Option<&'static str>," for name in CASE_STRING_FIELDS]
    out = [
        f"// @generated by tools/parity/parity/gen_metadata_cases.py with {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_metadata_cases import cases; cases()' > tests/metadata_cases/cases.rs",
        "// Upstream's `extract_metadata` on pages: the metadata it returns.",
        "",
        "/// A page parsed by html5lib and the `Document` fields upstream's `extract_metadata` returns for it with the default",
        "/// URL and the author blacklist; `filedate` is not compared.",
        "pub struct MetadataCase {",
        "    pub html: &'static str,",
        "    pub default_url: Option<&'static str>,",
        "    pub author_blacklist: &'static [&'static str],",
        *fields,
        "    pub categories: Option<&'static [&'static str]>,",
        "    pub tags: Option<&'static [&'static str]>,",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub const METADATA_CASES: &[MetadataCase] = &[",
        *lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))
