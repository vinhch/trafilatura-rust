"""Generates src/deduplication/cases.rs: upstream's dedup helpers on fixed inputs.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_dedup_cases import cases; cases()' > src/deduplication/cases.rs

FINGERPRINT_CASES: each row is an input, the tokens `sample_tokens(input)` returns and the fingerprint
`content_fingerprint(input)` returns, both with the default length of 64. The inputs cover the two paths of
`sample_tokens` (the primary split and the punctuation fallback), the length threshold at its edges, punctuation
and separators, decomposed and non-ASCII digits, a long paragraph and seeded random strings.

VALIDATE_CASES: each row is an input (`Str` or `Int`), a simhash length and `Simhash.validate` for it. The strings
cover the `int(s, 16)` rules (signs, `0x` prefixes, underscores, Unicode digits) and the decimal branch of 18 to
22 digits.

BOW_CASES: each row is an input, a digest length and `generate_bow_hash(input, length)` as hex.
"""

import random
import sys

from parity.gen_xpath_cases import rust_str

RANDOM_SEED = 4
RANDOM_STRINGS = 200
ALPHABET = (
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 "
    "!?,.;:'\"()-_。、！？《》«»　一二三四五六七八九十абвгдеёжз"
)
WORDS = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore".split()
PARAGRAPH_WORDS = 5000


def fixed_inputs() -> list[str]:
    "Inputs with a known reason to be in the table, in the order the cases read."
    return [
        "abcde ijk l, " * 10,
        "Hello world! This is a test string with some numbers 123.",
        "这是一个测试。我们在测试中文。",
        "Hello世界。This is混合文本!",
        "",
        "   ",
        "!!! ... ???",
        "。、！",
        "'hello' (test), \"world\" [x]",
        "don't stop, it's fine",
        "«mot»",
        "some «mot» words here",
        "一二三　四五六 七八",
        "alpha\x1cbeta\x1dgamma\x1egamma",
        "café",
        "café and more words",
        "٣٤٥",
        "x²",
        "ab " * 40,
        " ".join(f"token{i}" for i in range(31)) + " ab cd",
        " ".join(f"token{i}" for i in range(32)) + " ab cd",
        " ".join(f"token{i}" for i in range(33)) + " ab cd",
        " ".join(["ab"] * 40),
        " ".join(["ab"] * 40 + ["token"] * 3),
        "abcde " * 100,
    ]


def random_inputs() -> list[str]:
    "Seeded strings of the mixed alphabet, and one long paragraph."
    rng = random.Random(RANDOM_SEED)
    strings = ["".join(rng.choice(ALPHABET) for _ in range(rng.randint(0, 60))) for _ in range(RANDOM_STRINGS)]
    paragraph = " ".join(rng.choice(WORDS) for _ in range(PARAGRAPH_WORDS))
    return strings + [paragraph]


VALIDATE_STRINGS = [
    "0", "", " ", " 12 ", "+5", "-5", "-0", "1_000", "0x1f", "0X1F", "0x_ff", "1F", "ff", "-ff", "f" * 17,
    "aghj", "٣", "１２３", "¹" * 18, "1" * 17, "1" * 18, "1" * 22, "1" * 23, "0" * 20 + "10",
    "18446744073709551615", "18446744073709551616",
]
VALIDATE_INTS = [0, 2**64 - 1, 2**64, -1, -(10**18)]
VALIDATE_LENGTH = 64
VALIDATE_SHORT_LENGTH = 2
VALIDATE_SHORT_INTS = [3, 4]
BOW_TEXTS = ["", "   ", "Hello world! This is a test string with some numbers 123.", "这是一个测试。我们在测试中文。"]
BOW_LENGTHS = [1, 12, 24, 64]


def validate_rows() -> list[tuple[str, object, int, object]]:
    "(kind, input, length, Simhash.validate(input)) rows."
    from trafilatura.deduplication import Simhash

    rows = []
    for text in VALIDATE_STRINGS:
        rows.append(("Str", text, VALIDATE_LENGTH, Simhash(length=VALIDATE_LENGTH).validate(text)))
    for number in VALIDATE_INTS:
        rows.append(("Int", number, VALIDATE_LENGTH, Simhash(length=VALIDATE_LENGTH).validate(number)))
    for number in VALIDATE_SHORT_INTS:
        rows.append(("Int", number, VALIDATE_SHORT_LENGTH, Simhash(length=VALIDATE_SHORT_LENGTH).validate(number)))
    return rows


def rust_option(value: object) -> str:
    "`Some(n)` or `None` for an optional integer."
    return "None" if value is None else f"Some({value})"


def cases() -> None:
    from trafilatura.deduplication import content_fingerprint, generate_bow_hash, sample_tokens

    version = ".".join(map(str, sys.version_info[:3]))
    rows = []
    for text in fixed_inputs() + random_inputs():
        tokens = sample_tokens(text)
        fingerprint = content_fingerprint(text)
        rows.append((text, tokens, fingerprint))
    # a fingerprint with a leading zero nibble is shorter than 16 digits: the hex is not padded
    assert any(len(fingerprint) < 16 for _, _, fingerprint in rows)
    lines = [
        f"    ({rust_str(text)}, &[{', '.join(rust_str(token) for token in tokens)}], {rust_str(fingerprint)}),"
        for text, tokens, fingerprint in rows
    ]
    validate = [
        f"    (HashInput::{kind}({rust_str(value) if kind == 'Str' else value}), {length}, {rust_option(expected)}),"
        for kind, value, length, expected in validate_rows()
    ]
    bow = [
        f"    ({rust_str(text)}, {length}, {rust_str(generate_bow_hash(text, length).hex())}),"
        for text in BOW_TEXTS
        for length in BOW_LENGTHS
    ]
    out = [
        f"// @generated by tools/parity/parity/gen_dedup_cases.py with Python {version}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_dedup_cases import cases; cases()' > src/deduplication/cases.rs",
        "// Upstream's `sample_tokens`, `content_fingerprint`, `Simhash.validate` and `generate_bow_hash` on fixed inputs.",
        "",
        "use super::HashInput;",
        "",
        "/// `(input, sample_tokens(input), content_fingerprint(input))`, at the default length of 64.",
        "#[rustfmt::skip]",
        "pub(super) const FINGERPRINT_CASES: &[(&str, &[&str], &str)] = &[",
        *lines,
        "];",
        "",
        "/// `(input, length, Simhash(length).validate(input))`.",
        "#[rustfmt::skip]",
        "pub(super) const VALIDATE_CASES: &[(HashInput<'static>, usize, Option<u64>)] = &[",
        *validate,
        "];",
        "",
        "/// `(input, length, generate_bow_hash(input, length) as hex)`.",
        "#[rustfmt::skip]",
        "pub(super) const BOW_CASES: &[(&str, usize, &str)] = &[",
        *bow,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    cases()
