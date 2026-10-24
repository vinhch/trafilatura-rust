# Gotchas

Traps found reading upstream; each one silently changes output. Read it before porting any upstream
function.

- lxml text and tail. Text after an element's closing tag belongs to that element (`.tail`), not to
  the parent. `main_extractor.py`, `xml.py` and `htmlprocessing.py` use `.tail` throughout. The Rust
  tree models text and tail directly (`dom::Node`); emulating them through text nodes needs care at
  every move, strip and delete. The tail travels with its node on `append`, `insert`, `addnext` and
  `addprevious`, and `copy.deepcopy` copies it (`Tree::copy`). `clear()` drops the element's text,
  tail and attributes (`Tree::clear`).
- Deleting keeps or drops the tail. Upstream's `delete_element(element, keep_tail=...)` (`xml.py`)
  decides per call site, `lxml.etree.strip_tags` merges children's text into the parent, and a plain
  `parent.remove(child)` takes the tail out of the parent's content (it stays on the removed node).
  `lxml.etree.strip_elements(tree, tags, with_tail=True)` drops the tail with the subtree; with
  `with_tail=False` it joins the previous sibling's tail or the parent's text. Port the exact
  operation each call site uses.
- A live `iter` skips after a deleted subtree. lxml's `tree.iter(...)` computes the next node before
  it yields the current one, so `for e in tree.iter(x): delete_element(e)` continues at a successor
  that may lie inside the removed subtree, and the walk ends at that subtree's root: later matches
  are never visited. `Tree::iter` is the collected form (`list(tree.iter())`, an XPath result);
  `LiveIter` is the live one. Each call site ports the form upstream wrote.
- A live `iter` ends when its precomputed successor is stripped. lxml fixes the next match before the
  loop body runs; when the body then strips that node (`strip_tags` unlinks it and moves its children
  out), the walk continues from a childless, parentless node and stops. `handle_paragraphs` hits it
  (`<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>` gives `<p>A <hi>b c</hi> e </p>`), and so does
  `handle_lists` through `process_nested_elements`. Both are upstream bugs, ported as they are:
  `LiveIter` fixes its next match the same way, and `Tree::strip_tags` leaves the stripped node
  detached with no children, text or tail.
- `element.find("x")` is a child search, `element.find(".//x")` a descendant search. `Tree::find`
  is the descendant form; `is_code_block_element` (`find("code")`) and `test_htmlprocessing`
  (`find("hi")`) need `tree.children(x).find(..)`.
- `//` reads the whole document. `subtree.xpath("//p//text()")` is absolute: from a subtree it reads
  every `p` of the document the subtree was found in, and a subtree that pruning detached still
  reads the original document (libxml2 evaluates `/` against it). `extract_body` takes the root of
  the tree it received for this, never `root_of(subtree)`; the text nodes under nested `p` count
  once.
- On the html5 tree lxml names a foreign element `{namespace}name` (D-2), a plain string in the
  places upstream treats tags as strings: `.//*` includes SVG and MathML elements (`{ns}svg` in the
  `tags` set of `_extract`, so its `tags == {"lb"}` tests fail), `strip_tags(element, *{str(c.tag)
  ...})` in `handle_paragraphs` strips them, and `SubElement(orig, processed.tag)` in
  `define_newelem` keeps the namespace. The port compares by namespace and name: it strips every
  descendant whose tag is outside `potential_tags` and `done`, foreign ones included, and builds
  copies with `new_element_like`.
- `str.isdecimal` is Unicode `Nd`, and `int` reads any `Nd` digit (`"٣"` is 3, `"１２"` is 12;
  `"²"` is not `Nd`), then raises `ValueError` past 4300 digits (`"0" * 4301`, `"٣" * 4301`). `_span`
  parses with both, so `rowspan="٣"` spans three rows and a 4301-digit span rejects the page
  (`Rejection::InvalidValue`). `pystr::is_decimal`, `decimal_value` and `MAX_STR_DIGITS` hold
  them. `colspan="0"` gives 0, so `range(colspan - 1)` is empty.
- `itertext` yields `''` for an empty text node (`e[0].tail = ""` gives `['', '', '']`), and
  `" ".join` counts it: the comments text of `<div id='comments'><p>One comment here</p><br>Second
  line<p>third</p></div>` has two spaces before `third`, because `handle_textnode` leaves the former
  `lb` with tail `""`. `Tree::itertext` drops empty pieces; every `" ".join(... .itertext())` uses
  `Tree::itertext_lxml`. The joined strings feed the length gates of `extract_content` and
  `core.py`, so a dropped space can flip a branch.
- The parser is not html5. Upstream parses with `lxml.html.HTMLParser` (libxml2), configured in
  `utils.py` (`HTML_PARSER`: comments and processing instructions removed). libxml2 2.14+ tokenizes
  as HTML5 but builds the tree with its own rules: no implied `tbody`, no adoption agency, different
  handling of misnested and stray tags. This crate uses html5ever (D-2). lxml wheels bundle libxml2
  2.14.6 on Linux and macOS but 2.11.9 on Windows, so upstream itself differs by platform: fixtures
  come from Linux. Upstream may switch parsers (OI-5, checked in `upstream-sync.md`).
- html5ever's tree sink cannot stop the parse. After the depth cut, a node-limit abort (D-5), a
  cancellation or a deadline, the sink in `dom/parse.rs` must ignore every later call, and depth is
  checked again when html5ever moves a subtree (reparenting, foster parenting, the adoption agency).
  html5ever clones open formatting elements, attributes included, so attribute values are shared
  `Arc<str>`, never copied per clone (`StrTendril` is not `Send`). The doc comments in
  `dom/parse.rs` give the chunking and sharing details.
- Decode strictly. Upstream tries each guessed codec and falls back to UTF-8 with replacement only
  when all fail; `encoding_rs`'s `Encoding::decode` sniffs a BOM and replaces silently, which skips
  that fallback. Feed the detector the sample upstream uses (whole input under 10000 bytes, else the
  first and last 5000).
- XPath semantics. `contains(@class, "x")` is a substring test, not a class-token test; results come
  in document order. Port each predicate exactly, including the substrings that look like accidents.
  `translate(s, from, to)` maps only the listed characters, so it is not a lowercase:
  `contains(translate(@id,'CM','cm'), 'main-content')` matches `Main-Content` but not `MAIN-CONTENT`.
  Port each call as its own character map.
- Names and namespaces. In the html5 tree SVG and MathML elements carry a namespace; libxml2 has
  none. A plain tag name in XPath matches the HTML namespace only, and renaming an element
  drops its namespace (D-2).
- MathML and SVG survive cleaning. In the html5 tree `math` and `svg` carry a namespace, so
  `tree.iter("math")` in `recover_math` and the `MANUALLY_CLEANED` loop skip them: `tree_cleaning`
  leaves both in place and recovers no LaTeX, where upstream's libxml2 tree drops them and keeps the
  formula (D-2). The lxml tag of such an element is `{ns}name`, so a plain comparison upstream writes
  (`elem.tag == "graphic"`, `elem.tag in (...)`) is false for it. Port the test as
  `tree.tag(id) == Some(..)` and never return early on `None`: `handle_textnode` and `process_node`
  keep such elements.
- XPath position and union. `.//x[1]` selects every `x` that is the first `x` child of its parent;
  `(.//x)[1]` selects the first `x` in the document. A union in string context (`re:test(@id|@class,
  ...)`) reads only the first node in document order, which for two attributes is the one written
  first in the source.
- Python `re` is not Rust `regex`. Rust `regex` has no lookaround and no backreferences, which
  upstream uses (`xml.py` Markdown escaping uses `(?<!...)`, `(?!...)` and `\2`). Rewrite those as
  explicit code, and test them against upstream's regex on the same inputs. Filtering the matches of
  a lookbehind-free pattern afterwards is not the same: on `"x\[a \[b\]"` Python skips the first
  `\[` (preceded by `x`) and matches `\[b\]`, where a plain match takes `\[a \[b\]`, drops it and
  resumes after it. The selectors use regex
  too: EXSLT `re:test(...)` in `xpaths.py` and `settings.py` runs Python `re`, including the
  lookahead in `[Cc]omment(?!ary)` that keeps "commentary" sections out of the comment selector.
  `re:test(s, p)` is `re.search`, not a full match, and its `'i'` flag is `re.IGNORECASE`.
- Python `re` anchors differ from Rust `regex`. `$` also matches before a final `\n`; Rust needs
  `\n?\z` for that. In the block-start patterns of `xml.py` it decides output: `"#\n"` gives
  `"\#\n"` and `"1.\n"` gives `"1\.\n"`, while `"#\nx"` is unchanged. `re.match` anchors at the
  start only, so a pattern used with it starts with
  `\A`; `re.search` does not. `{,n}` is written `{0,n}`.
- `\w`, `\W`, `\b` and `\s` differ. Python `\w` is letters, all numerics (`No` such as U+00B2
  included) and `_`, without combining marks; Rust `\w` includes marks and connector punctuation but
  not `No`. `\W` and `\b` follow from `\w`; `\s` is Python's whitespace set (whitespace bullet below).
  Interpolate `pystr::PY_SPACE_CLASS` and `pystr::PY_WORD_CLASS` into the pattern where upstream
  writes `\s` or `\w` (`IMAGE_EXTENSION` and `RE_FILTER` in `utils.rs`).
- `\d` is any `Nd` digit in Python: the ordered-list block-start pattern of `xml.py` turns `"١. x"`
  into `"١\. x"`. The hand-written scanner reads digits with `pystr::decimal_value`, the oracle
  image's Unicode data, not the `regex` crate's.
- `re.IGNORECASE` folds U+0130 (capital I with dot) and U+0131 (dotless i) to `i`; Rust `(?i)` does
  not. A pattern that can meet them rewrites each `i` to a class of `i`, U+0130 and U+0131
  (`ignore_case_i` in `pystr.rs`); `COOKIE_CONSENT_RE` in `settings.rs` does the same. The
  other special folds (U+017F long s with `s`, U+212A Kelvin sign with `k`) agree in both engines and
  need no rewrite.
- Whitespace and line splitting differ. Python `str.split()`, `str.strip()`, `str.isspace()` and the
  `re` class `\s` treat `\x1c`-`\x1f` as whitespace; Rust's `char::is_whitespace` and `regex` `\s` do
  not. `str.splitlines()` splits on
  more than `\n`: `\r`, `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, U+2028 and U+2029, with `\r\n` one break;
  Rust's `lines` splits on `\n` and `\r\n` only. Use `pystr::is_space`, `split_whitespace`, `strip`
  and `splitlines` wherever upstream calls the Python method.
- `str.isprintable` has no Rust equivalent. It is false for unassigned, control, format, surrogate,
  private-use and separator characters other than the ASCII space (so false for U+00A0). `pystr`
  searches a table from the interpreter the fixtures were made with, because a Unicode crate would
  follow a newer Unicode version and differ on newly assigned characters.
- A character class upstream did not mean. `utils.py` `LINES_TRIMMING` is `(?<![p{P}>])\n`: Python
  `re` has no `\p{P}`, so the class is the literal characters `p`, `{`, `P`, `}`, `>`. Port the
  literal behavior and keep a comment saying so.
- Lengths are code points. Python `len(text)` and `text[:n]` count code points; Rust `str::len` counts
  bytes and byte slicing panics mid-character. Every size threshold (`MIN_EXTRACTED_SIZE` and the
  others) compares `chars().count()`. On an lxml element, `len(element)` is its child count, not a
  text length (`len(postbody)` in `core.py`).
- Truthiness. `if text:` is false for both `None` and `""`; `if element:` on an lxml element is
  false when it has no children, even when it has text. Port the test upstream wrote, not the one it
  reads like.
- Process-global state. `deduplication.py` keeps `LRU_TEST` as a module global, cleared by
  `meta.reset_caches()`. It becomes caller-owned state here. `functools.lru_cache` on pure functions
  is only memoization: drop it unless profiling says otherwise.
- Defaults come from `settings.cfg` as well as `settings.py`: `MAX_FILE_SIZE = 20000000`,
  `MIN_FILE_SIZE = 10`, an empty `MAX_TREE_SIZE` meaning no limit.
- Ordering. Python `dict` keeps insertion order and `set` does not. Where upstream output depends on
  iterating a dict, keep the order (an ordered map or a `Vec`); where it iterates a set, the order is
  undefined upstream too, so sort and record it in `deviations.md`.
- `urllib.parse` is not WHATWG. `courlan` builds on Python's `urlsplit`, `urlunsplit`, `urljoin` and
  `html.unescape`, which differ from the `url` crate on dot segments, schemes outside `uses_relative`,
  backslashes and what they reject, so the crate never uses the `url` crate; `pyurl.rs` ports CPython.
  Two behaviors a port misses: `html.unescape` raises `ValueError` on a long decimal character
  reference (next bullet), and `urlsplit` raises when the NFKC form of a netloc holds one of `/?#@:`
  (`_checknetloc`), so `safe_base_url` and `safe_relative_url` return `""` for both. Python 3.13 patch
  releases and 3.14 differ (`_check_bracketed_netloc`, `urljoin`): the oracle image's Python is the
  reference, a local interpreter is not.
- `html.unescape` is not a table lookup. A name missing from the table falls back to its longest
  prefix that is in it, which only a legacy name without `;` can be: `&ampx` gives `&x`, `&notin`
  gives `¬in`. A decimal reference of more than 4300 digits raises `ValueError`, leading zeros
  counted (`"&#" + "0" * 4299 + "65;"` raises, 4298 zeros give `A`); a hex reference has no limit
  (`"&#x" + "0" * 5000 + "41;"` gives `A`) and a value past 0x10FFFF gives U+FFFD. Some codes do not
  decode to themselves: `&#128;` is `€`, `&#0;` is U+FFFD, `&#1;` is the empty string. `xmltotxt` ends
  with it outside any `try`, so the raise escapes `extract` (`Rejection::InvalidValue`).
- `web_atoms::NAMED_ENTITIES` is not Python's `html5` table. Besides its 2231 names it holds every
  proper prefix of a name with the value `(0, 0)` (`"am"`, `""`), which is not an entity, and
  `C1_REPLACEMENTS` has `None` for 0x81, 0x8D, 0x8F, 0x90 and 0x9D, where Python maps the code to
  itself. `pyhtml::unescape` skips the first and fills the second; `entity_table_matches_python`
  fails when a `web_atoms` bump changes the names.
- lxml keeps `""` apart from `None`. `elem.text, elem.tail = elem.tail, ""` leaves the tail an empty
  string, and `handle_textnode` guards its trim with `if elem.tail:` so it stays one. The tree keeps
  `Some("")` apart from `None` for that reason; a tree dump omits both, so only an assertion on the
  field shows the difference (`test_htmlprocessing`).
- `etree.tostring(pretty_print=True)` follows libxml2's format rule, not "indent every child". An
  element with any text node among its children (its text, or a child's tail, even `""` or
  whitespace) is written without indentation inside, down its whole subtree: `<body><div>
  <p>x</p></div></body>` gives `<body>\n  <div> <p>x</p></div>\n</body>\n`. Indentation stops at 60
  spaces (35 nested `div` stay at 60 from level 30 on). `<x/>` needs a `None` text and no children:
  `p.text = ""` gives `<p></p>`. `Tree::tostring` follows all three.
- A dict built by inversion keeps the last key. `HTML_TAG_MAPPING` inverts `REND_TAG_MAPPING`, so
  `#i` gives `i` (not `em`), `#b` gives `strong` and `#t` gives `var`.
- Python `int(str)` strips ASCII `" \t\n\v\f\r"` and the non-ASCII `isspace` characters, but not
  `\x1c`-`\x1f` (`int("\x1c1\x1f")` raises, `int("\x851\u3000")` gives 1), so it is not `strip`.
  `convert_to_html` reads a heading level with it: `<details><summary rend="hx">` raises,
  `rend="h 2"` gives `<h2>` and `rend="h9"` gives `<h9>`. It also takes a sign and single `_`
  between digits (`int("1_" * 2500 + "1")` is accepted); only digits count toward the 4300 limit,
  leading zeros included. `pystr::int_str` ports it.
