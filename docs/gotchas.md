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
- Python `re` is not Rust `regex`. Rust `regex` has no lookaround and no backreferences, which
  upstream uses (`xml.py` Markdown escaping uses `(?<!...)`, `(?!...)` and `\2`). Rewrite those as
  explicit code, and test them against upstream's regex on the same inputs. The selectors use regex
  too: EXSLT `re:test(...)` in `xpaths.py` and `settings.py` runs Python `re`, including the
  lookahead in `[Cc]omment(?!ary)` that keeps "commentary" sections out of the comment selector.
- Python `re` anchors differ from Rust `regex`. `$` also matches before a final `\n`; Rust needs
  `\n?\z` for that. `re.match` anchors at the start only, so a pattern used with it starts with
  `\A`; `re.search` does not. `{,n}` is written `{0,n}`.
- `\w`, `\W`, `\b` and `\s` differ. Python `\w` is letters, all numerics (`No` such as U+00B2
  included) and `_`, without combining marks; Rust `\w` includes marks and connector punctuation but
  not `No`. `\W` and `\b` follow from `\w`; `\s` is Python's whitespace set (whitespace bullet below).
  Interpolate `pystr::PY_SPACE_CLASS` and `pystr::PY_WORD_CLASS` into the pattern where upstream
  writes `\s` or `\w` (`IMAGE_EXTENSION` and `RE_FILTER` in `utils.rs`).
- `re.IGNORECASE` folds U+0130 (capital I with dot) and U+0131 (dotless i) to `i`; Rust `(?i)` does
  not. A pattern that can meet them rewrites each `i` to a class of `i`, U+0130 and U+0131
  (`ignore_case_i` in `utils.rs`); the `re:test(..., 'i')` selectors of Phase 3b need the same. The
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
