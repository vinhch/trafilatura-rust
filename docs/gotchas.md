# Gotchas

Traps found reading upstream; each one silently changes output. Read it before porting any upstream
function.

- lxml text and tail. Text after an element's closing tag belongs to that element (`.tail`), not to
  the parent. `main_extractor.py`, `xml.py` and `htmlprocessing.py` use `.tail` throughout. The Rust
  tree must model text and tail directly; emulating them through text nodes needs care at every move,
  strip and delete.
- Deleting keeps or drops the tail. Upstream's `delete_element(element, keep_tail=...)` (`xml.py`)
  decides per call site, `lxml.etree.strip_tags` merges children's text into the parent, and a plain
  `parent.remove(child)` drops the tail. Port the exact operation each call site uses.
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
- XPath semantics. `contains(@class, "x")` is a substring test, not a class-token test;
  `translate(...)` is the case-folding idiom; results come in document order. Port each predicate
  exactly, including the substrings that look like accidents.
- Python `re` is not Rust `regex`. Rust `regex` has no lookaround and no backreferences, which
  upstream uses (`xml.py` Markdown escaping uses `(?<!...)`, `(?!...)` and `\2`). Rewrite those as
  explicit code, and test them against upstream's regex on the same inputs. The selectors use regex
  too: EXSLT `re:test(...)` in `xpaths.py` and `settings.py` runs Python `re`, including the
  lookahead in `[Cc]omment(?!ary)` that keeps "commentary" sections out of the comment selector.
- `\s` and whitespace differ. Python's `\s`, `str.split()`, `str.strip()` and `str.isspace()` treat
  `\x1c`-`\x1f` as whitespace; Rust's `\s` and `char::is_whitespace` do not. Use one helper that
  matches Python's definition everywhere upstream does.
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
