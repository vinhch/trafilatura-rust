"""Generates src/dom/parse/tables.rs: libxml2 2.14.6's HTML parser tables, read by the libxml2 tree builder.

Regenerate with (from the repository root, Git Bash or Linux):

    uv run --frozen --no-dev --directory tools/parity python -m parity.gen_libxml2_tables > src/dom/parse/tables.rs

The sources are fetched from the v2.14.6 tag and checked against the sha256 constants below; a mismatch
fails the run. No lxml runs here, so this does not go through scripts/oracle.sh.
"""

import hashlib
import re
import sys
import urllib.request

LIBXML2_VERSION = "2.14.6"
SOURCE_BASE = f"https://gitlab.gnome.org/GNOME/libxml2/-/raw/v{LIBXML2_VERSION}"

# File name and sha256 at the tag. Each URL is SOURCE_BASE + "/" + name.
SOURCES = {
    # https://gitlab.gnome.org/GNOME/libxml2/-/raw/v2.14.6/HTMLparser.c
    "HTMLparser.c": "8946b146f6164610ae1501e18e4db532ee67944dde0d0c6f2057103b50a42f2a",
    # https://gitlab.gnome.org/GNOME/libxml2/-/raw/v2.14.6/HTMLtree.c
    "HTMLtree.c": "302f4154db8dbc354893b2934567c20554b1a7653e1573b76493ad9f6e702943",
    # https://gitlab.gnome.org/GNOME/libxml2/-/raw/v2.14.6/Copyright
    "Copyright": "5d4873884a890122a4b9b20ad56ac6f7da1d796a5bfcf04a427970ac96217626",
}

# The `dataMode` column of html40ElementTable, by the DATA_ name the source uses.
DATA_MODES = {
    "0": "Normal",
    "DATA_RCDATA": "RcData",
    "DATA_RAWTEXT": "RawText",
    "DATA_SCRIPT": "Script",
    "DATA_PLAINTEXT": "PlainText",
}

# The `empty` column is the fourth of the seven flag columns after the name; the rest are not read.
ELEMENT_ENTRY = re.compile(
    r'\{\s*"(?P<name>[a-z0-9]+)",'
    r"\s*\d,\s*\d,\s*\d,\s*(?P<empty>[01]),"
    r"\s*\d,\s*\d,\s*\d,"
    r'\s*"(?:[^"\\]|\\.)*"\s*,'
    r"\s*NULL,\s*NULL,\s*NULL,\s*NULL,\s*NULL,"
    r"\s*(?P<mode>\w+)\s*\}"
)
START_CLOSE_ENTRY = re.compile(r'\{\s*"(?P<open>[a-z0-9]+)",\s*"(?P<new>[a-z0-9]+)"\s*\}')
END_PRIORITY_ENTRY = re.compile(r'\{\s*(?:"(?P<name>[a-z0-9]+)"|NULL),\s*(?P<priority>\d+)\s*\}')


def fetch(name: str) -> bytes:
    """The bytes of `name` at the tag, after its sha256 check."""
    request = urllib.request.Request(f"{SOURCE_BASE}/{name}", headers={"User-Agent": "trafilatura-parity"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SOURCES[name]:
        raise RuntimeError(f"{name}: sha256 {digest}, expected {SOURCES[name]}")
    return data


def c_array(source: str, header: str) -> str:
    """The text between the braces of the C array that starts with `header`."""
    if source.count(header) != 1:
        raise RuntimeError(header)
    start = source.index(header) + len(header)
    return source[start : source.index("\n};", start)]


def check_entries(body: str, matches: list[re.Match[str]], label: str) -> None:
    """Every `{` of the array is one matched entry, so no entry escapes its pattern."""
    if body.count("{") != len(matches):
        raise RuntimeError(f"{label}: {body.count('{')} entries, {len(matches)} matched")


def check_sorted(keys: list[tuple[str, ...]], label: str) -> None:
    """Keys are unique and in byte order, the order binary_search needs."""
    if keys != sorted(set(keys)):
        raise RuntimeError(f"{label}: not sorted or not unique")


def element_table(parser: str) -> list[tuple[str, bool, str]]:
    """(name, empty, DataMode variant) of each html40ElementTable entry."""
    body = c_array(parser, "html40ElementTable[] = {")
    matches = list(ELEMENT_ENTRY.finditer(body))
    check_entries(body, matches, "html40ElementTable")
    entries = [(m["name"], m["empty"] == "1", DATA_MODES[m["mode"]]) for m in matches]
    check_sorted([(name,) for name, _, _ in entries], "html40ElementTable")
    return entries


def start_close_table(parser: str) -> list[tuple[str, str]]:
    """(open, new) of each htmlStartClose entry: `new` closes an open `open`."""
    body = c_array(parser, "htmlStartClose[] = {")
    matches = list(START_CLOSE_ENTRY.finditer(body))
    check_entries(body, matches, "htmlStartClose")
    pairs = [(m["open"], m["new"]) for m in matches]
    check_sorted(pairs, "htmlStartClose")
    return pairs


def end_priority_table(parser: str) -> tuple[list[tuple[str, int]], int]:
    """The named htmlEndPriority entries in source order, and the terminating entry's priority."""
    body = c_array(parser, "htmlEndPriority[] = {")
    matches = list(END_PRIORITY_ENTRY.finditer(body))
    check_entries(body, matches, "htmlEndPriority")
    if matches[-1]["name"] is not None or any(m["name"] is None for m in matches[:-1]):
        raise RuntimeError("htmlEndPriority: NULL is not the terminating entry")
    named = [(m["name"], int(m["priority"])) for m in matches[:-1]]
    return named, int(matches[-1]["priority"])


def boolean_attributes(tree: str) -> list[str]:
    """The names of htmlBooleanAttrs, which ends with NULL."""
    body = c_array(tree, "htmlBooleanAttrs[] = {")
    if not re.fullmatch(r'\s*(?:"[a-z]+",\s*)*NULL\s*', body):
        raise RuntimeError("htmlBooleanAttrs: not a list of names ending in NULL")
    return re.findall(r'"([a-z]+)"', body)


def copyright_lines(text: str) -> list[str]:
    """The Copyright file, one `//` comment line per line of it."""
    if "Permission is hereby granted, free of charge" not in text:
        raise RuntimeError("Copyright: no MIT permission grant")
    return [f"// {line}".rstrip() for line in text.splitlines()]


def render(parser: str, tree: str, copyright_text: str) -> str:
    """The Rust module, with the lines of its header and each table."""
    elements = element_table(parser)
    start_close = start_close_table(parser)
    end_priorities, default_priority = end_priority_table(parser)
    booleans = boolean_attributes(tree)
    out = [
        f"// @generated by tools/parity/parity/gen_libxml2_tables.py from libxml2 {LIBXML2_VERSION}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   uv run --frozen --no-dev --directory tools/parity python -m parity.gen_libxml2_tables > src/dom/parse/tables.rs",
        "//",
        f"// Sources, tag v{LIBXML2_VERSION}, checked against their sha256 in the generator:",
        f"//   {SOURCE_BASE}/HTMLparser.c",
        f"//   {SOURCE_BASE}/HTMLtree.c",
        f"//   {SOURCE_BASE}/Copyright",
        "//",
        *copyright_lines(copyright_text),
        "",
        "//! libxml2's HTML tables: element descriptions, start-tag closing, end-tag priorities and",
        "//! boolean attributes. Lookups take lowercase ASCII names.",
        "",
        "/// The `htmlElemDesc` columns a tree rule reads.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub(in crate::dom) struct ElemDesc {",
        "    /// The `empty` column: the element has no content and no end tag.",
        "    pub(in crate::dom) empty: bool,",
        "    /// The `dataMode` column.",
        "    pub(in crate::dom) data_mode: DataMode,",
        "}",
        "",
        "/// How the content after a start tag is read.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub(in crate::dom) enum DataMode {",
        "    /// Ordinary markup.",
        "    Normal,",
        "    /// Text with character references and no tags: `title`, `textarea`.",
        "    RcData,",
        "    /// Text with no references and no tags.",
        "    RawText,",
        "    /// Script data: text up to `</script`.",
        "    Script,",
        "    /// Everything after the start tag is text.",
        "    PlainText,",
        "}",
        "",
        "/// `html40ElementTable`, sorted by name.",
        "#[rustfmt::skip]",
        "pub(in crate::dom) const HTML40_ELEMENT_TABLE: &[(&str, ElemDesc)] = &[",
    ]
    for name, empty, mode in elements:
        out.append(
            f'    ("{name}", ElemDesc {{ empty: {str(empty).lower()}, data_mode: DataMode::{mode} }}),'
        )
    out += [
        "];",
        "",
        "/// The description of the element `name`, `None` for a name the table lacks.",
        "// libxml2: HTMLparser.c::htmlTagLookup",
        "pub(in crate::dom) fn element(name: &str) -> Option<&'static ElemDesc> {",
        "    HTML40_ELEMENT_TABLE",
        "        .binary_search_by(|(known, _)| known.cmp(&name))",
        "        .ok()",
        "        .and_then(|index| HTML40_ELEMENT_TABLE.get(index))",
        "        .map(|(_, desc)| desc)",
        "}",
        "",
        "/// `htmlStartClose` as `(open, new)` pairs, sorted: the start tag `new` closes an open `open`.",
        "pub(in crate::dom) const HTML_START_CLOSE: &[(&str, &str)] = &[",
    ]
    for open, new in start_close:
        out.append(f'    ("{open}", "{new}"),')
    out += [
        "];",
        "",
        "/// Whether the start tag `new_tag` closes the open element `open_tag`.",
        "// libxml2: HTMLparser.c::htmlCheckAutoClose",
        "pub(in crate::dom) fn start_closes(new_tag: &str, open_tag: &str) -> bool {",
        "    HTML_START_CLOSE",
        "        .binary_search_by(|(open, new)| (*open, *new).cmp(&(open_tag, new_tag)))",
        "        .is_ok()",
        "}",
        "",
        "/// `htmlEndPriority` in source order: the named elements and their priorities.",
        "pub(in crate::dom) const HTML_END_PRIORITY: &[(&str, u8)] = &[",
    ]
    for name, priority in end_priorities:
        out.append(f'    ("{name}", {priority}),')
    out += [
        "];",
        "",
        "/// The priority of a name the table lacks: its terminating entry.",
        f"const DEFAULT_END_PRIORITY: u8 = {default_priority};",
        "",
        "/// The end-tag priority of `name`.",
        "// libxml2: HTMLparser.c::htmlGetEndPriority",
        "pub(in crate::dom) fn end_priority(name: &str) -> u8 {",
        "    HTML_END_PRIORITY",
        "        .iter()",
        "        .find(|(known, _)| *known == name)",
        "        .map_or(DEFAULT_END_PRIORITY, |(_, priority)| *priority)",
        "}",
        "",
        "/// `htmlBooleanAttrs`: attributes written without a value in the HTML method.",
        "// libxml2: HTMLtree.c::htmlIsBooleanAttr",
        "#[rustfmt::skip]",
        "pub(in crate::dom) const BOOLEAN_ATTRIBUTES: &[&str] = &[",
    ]
    out += [f'    "{name}",' for name in booleans]
    out.append("];")
    return "\n".join(out) + "\n"


def main() -> None:
    parser = fetch("HTMLparser.c").decode("utf-8")
    tree = fetch("HTMLtree.c").decode("utf-8")
    copyright_text = fetch("Copyright").decode("utf-8")
    sys.stdout.buffer.write(render(parser, tree, copyright_text).encode("utf-8"))


if __name__ == "__main__":
    main()
