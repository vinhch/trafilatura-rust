#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Shared by `measure_time.rs`, `measure_heap.rs`, `measure_sample.rs` and `benches/extract.rs`: the
//! page list and its sample, the `load_html` and `extract` calls, the synthetic pages and the JSON
//! files. The two measure targets are separate binaries because the heap one installs the dhat
//! allocator, which locks on every allocation and would inflate the timings.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};
use trafilatura_rust::__internals::{HtmlInput, MAX_FILE_SIZE, ParseLimits, Tree, load_html};
use trafilatura_rust::{Call, ExtractError, Extractor, Focus, Format, Input, Options, Rejection};

#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;

/// Whether the run was asked for; otherwise the target prints a notice and exits.
pub fn requested(name: &str) -> bool {
    let on = std::env::var_os("TRAFILATURA_MEASURE").is_some_and(|value| value == "1");
    if !on {
        println!("{name}: skipped; run scripts/measure.sh (or set TRAFILATURA_MEASURE=1)");
    }
    on
}

pub fn load(bytes: &[u8]) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
}

/// `extract` over a page with the options the fixtures use: `Options::new()` and `Format::Txt`.
pub fn extract(bytes: &[u8]) -> Result<String, ExtractError> {
    static EXTRACTOR: OnceLock<Extractor> = OnceLock::new();
    EXTRACTOR
        .get_or_init(|| Options::new().format(Format::Txt).build())
        .extract(&Input::bytes(bytes), &Call::default())
}

/// What a synthetic page must give.
#[derive(Clone, Copy)]
pub enum Expect {
    /// `Ok(_)`.
    Ok,
    /// `Err(Rejected(TreeTooLarge))`.
    TooLarge,
}

impl Expect {
    fn outcome(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::TooLarge => "too_large",
        }
    }
}

/// A generated page that probes a limit or a growth path of `extract`.
pub struct Synthetic {
    pub name: &'static str,
    pub html: String,
    pub options: Options,
    pub expect: Expect,
}

impl Synthetic {
    pub fn extract(&self) -> Result<String, ExtractError> {
        self.options
            .build()
            .extract(&Input::bytes(self.html.as_bytes()), &Call::default())
    }
}

/// The result of a call as the word `Expect::outcome` uses, or the error's debug form.
pub fn outcome<T>(result: &Result<T, ExtractError>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(ExtractError::Rejected(Rejection::TreeTooLarge)) => "too_large".to_string(),
        Err(err) => format!("{err:?}"),
    }
}

fn document(body: &str) -> String {
    format!("<html><body>{body}</body></html>")
}

fn paragraphs(count: usize) -> String {
    let mut out = String::new();
    for n in 0..count {
        write!(out, "<p>w{n}x</p>").unwrap();
    }
    out
}

/// One `<table>` of `rows` rows, each one cell spanning `MAX_SPAN` (100) columns.
fn wide_table(rows: usize) -> String {
    let mut out = String::from("<table>");
    for n in 0..rows {
        write!(out, "<tr><td colspan=\"100\">w{n}x</td></tr>").unwrap();
    }
    out.push_str("</table>");
    out
}

/// An `<ol>` of `items` items inside an `<article>`: a bare `<ol>` in the body comes out as plain
/// lines, so the numbering of `xml::list_marker` would never run.
fn ordered_list(items: usize) -> String {
    let mut out = String::from("<article><ol>");
    for n in 0..items {
        write!(out, "<li>w{n}x</li>").unwrap();
    }
    out.push_str("</ol></article>");
    out
}

/// A Markdown page of `items` ordered-list items: Markdown is the only format where the numbering of
/// `xml::list_marker` runs.
pub fn ordered_list_page(name: &'static str, items: usize) -> Synthetic {
    Synthetic {
        name,
        html: document(&ordered_list(items)),
        options: Options::new().format(Format::Markdown),
        expect: Expect::Ok,
    }
}

/// `prefix`, then `unit` repeated until the length reaches `bytes`, then `suffix`.
fn filled(prefix: &str, unit: &str, suffix: &str, bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + prefix.len() + unit.len() + suffix.len());
    out.push_str(prefix);
    while out.len() < bytes {
        out.push_str(unit);
    }
    out.push_str(suffix);
    out
}

/// A short paragraph after a large element, so the page has text to extract.
fn short_paragraph() -> String {
    format!("<p>{}</p>", "Short page text. ".repeat(20))
}

/// The page of the recall-revert test with its script body (`var a = 1;`) grown to `bytes`: with
/// `Focus::Recall` the cleaning reverts to its pre-cleaning copy, which still holds the script.
fn recall_page(bytes: usize) -> String {
    let head = format!(
        "<form><aside><p>Short aside paragraph here.</p></aside><div>{}</div></form><div>{}</div><script>",
        "Form body sentence with several words in it. ".repeat(8),
        "Outside text that is part of the page as well. ".repeat(3),
    );
    filled(&head, "var a = 1;", "</script>", bytes)
}

const MIB: usize = 1 << 20;
const JSON_LD_PREFIX: &str = r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"Article","articleBody":""#;

/// Pages for the limits and growth paths no corpus page reaches (D-5, D-9, Phase 8), and the shapes of
/// large real pages: an inline script, JSON-LD, preformatted text, a data table and a page that
/// extracts twice. `Nm` in a page name means `N` MiB of input.
/// `large_page` is above the default parse limit and raises it. `node_limit` is just under
/// `DEFAULT_MAX_NODES` and `node_limit_over` just over it. `cell_budget` is exactly `MAX_OUTPUT_NODES`
/// (1,000,000) cells and `cell_budget_over` one row more.
pub fn synthetic() -> Vec<Synthetic> {
    let mut large = paragraphs(1_100_000);
    large.push_str("<table>");
    for row in 0..10 {
        large.push_str("<tr>");
        for col in 0..10 {
            write!(large, "<td>c{row}x{col}</td>").unwrap();
        }
        large.push_str("</tr>");
    }
    large.push_str("</table>");
    let mut pages = vec![
        Synthetic {
            name: "large_page",
            html: document(&large),
            options: Options::new().max_nodes(Some(2_000_000)),
            expect: Expect::Ok,
        },
        Synthetic {
            name: "node_limit",
            html: document(&paragraphs(499_000)),
            options: Options::new(),
            expect: Expect::Ok,
        },
        Synthetic {
            name: "node_limit_over",
            html: document(&paragraphs(501_000)),
            options: Options::new(),
            expect: Expect::TooLarge,
        },
        Synthetic {
            name: "cell_budget",
            html: document(&wide_table(10_000)),
            options: Options::new(),
            expect: Expect::Ok,
        },
        Synthetic {
            name: "cell_budget_over",
            html: document(&wide_table(10_001)),
            options: Options::new(),
            expect: Expect::TooLarge,
        },
        ordered_list_page("ordered_list_50k", 50_000),
        ordered_list_page("ordered_list_200k", 200_000),
    ];
    let inline_script = |bytes: usize| {
        filled(
            "<script>",
            "var a = [1, 2, 3];\n",
            &format!("</script>{}", short_paragraph()),
            bytes,
        )
    };
    let json_ld = |bytes: usize| {
        filled(
            JSON_LD_PREFIX,
            "word ",
            &format!("\"}}</script>{}", short_paragraph()),
            bytes,
        )
    };
    let csv_pre = |bytes: usize| filled("<pre>", "1,alpha,2.5,beta\n", "</pre>", bytes);
    for (name, body, options) in [
        ("inline_script_20m", inline_script(20 * MIB), Options::new()),
        ("inline_script_50m", inline_script(50 * MIB), Options::new()),
        ("json_ld_20m", json_ld(20 * MIB), Options::new()),
        ("json_ld_50m", json_ld(50 * MIB), Options::new()),
        ("csv_pre_20m", csv_pre(20 * MIB), Options::new()),
        // Just above a power of two, where a text buffer grown piece by piece has the most spare capacity.
        ("csv_pre_33m", csv_pre(33 * MIB), Options::new()),
        ("csv_pre_50m", csv_pre(50 * MIB), Options::new()),
        (
            "data_table_20m",
            filled(
                "<table>",
                "<tr><td>1</td><td>alpha</td><td>2.5</td></tr>",
                "</table>",
                20 * MIB,
            ),
            Options::new().max_nodes(Some(2_000_000)),
        ),
        (
            "recall_rerun_20m",
            recall_page(20 * MIB),
            Options::new().focus(Focus::Recall),
        ),
    ] {
        pages.push(Synthetic {
            name,
            html: document(&body),
            options,
            expect: if name == "data_table_20m" {
                Expect::TooLarge
            } else {
                Expect::Ok
            },
        });
    }
    pages
}

/// One page of the parity corpus, as `index.json` lists it.
pub struct CorpusPage {
    pub id: String,
    /// The path under the upstream checkout.
    pub source: String,
    /// The file on disk: the upstream checkout joined with `source`.
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Every page of `index.json` in index order; `None` when the upstream checkout or the fixture set
/// is missing.
pub fn corpus() -> Option<Vec<CorpusPage>> {
    let upstream = common::upstream_root()?;
    let fixtures = common::fixture_dir()?;
    let index: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures.join("index.json")).unwrap())
            .unwrap();
    let pages = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let source = entry["source"].as_str().unwrap();
            let path = upstream.join(source);
            CorpusPage {
                id: entry["id"].as_str().unwrap().to_owned(),
                source: source.to_owned(),
                size_bytes: std::fs::metadata(&path).unwrap().len(),
                path,
            }
        })
        .collect();
    Some(pages)
}

/// Pages the sample takes from the large end of the corpus.
const SAMPLE_LARGEST: usize = 3;

/// A few pages that stand for the corpus, for the bench and the CI regression job, which cannot run
/// all of it: the smallest, the median and the `SAMPLE_LARGEST` largest by `(size_bytes, id)`, each
/// once, in that order.
pub fn sample(pages: &[CorpusPage]) -> Vec<&CorpusPage> {
    let mut sorted: Vec<&CorpusPage> = pages.iter().collect();
    sorted.sort_by(|a, b| (a.size_bytes, &a.id).cmp(&(b.size_bytes, &b.id)));
    let len = sorted.len();
    let picked: BTreeSet<usize> = [0, len / 2]
        .into_iter()
        .chain(len.saturating_sub(SAMPLE_LARGEST)..len)
        .collect();
    picked
        .into_iter()
        .filter_map(|index| sorted.get(index).copied())
        .collect()
}

/// `$TRAFILATURA_MEASURE_ONLY` value that selects `sample` of the corpus and no synthetic page. The
/// `@` is in no corpus path, which a plain word such as `sample` would match (`http_sample.html`).
const ONLY_SAMPLE: &str = "@sample";

/// Runs the corpus with `measure_page` and the synthetic pages `include` accepts with `synthetic`, as
/// `$TRAFILATURA_MEASURE_ONLY` says: unset, both; `synthetic`, only the synthetic pages (the corpus
/// file is left alone); `@sample`, only `sample` of the corpus, into the `-sample` file; anything
/// else, the corpus pages whose source contains it.
pub fn run_all(
    kind: &str,
    extra: Value,
    measure_page: impl Fn(&[u8]) -> Value,
    include: impl Fn(&Synthetic) -> bool,
    synthetic: impl Fn(&Synthetic) -> Value,
) {
    let only = std::env::var("TRAFILATURA_MEASURE_ONLY").unwrap_or_default();
    if only == ONLY_SAMPLE {
        run(&format!("{kind}-sample"), extra, measure_page, sample);
        return;
    }
    if only != "synthetic" {
        run(kind, extra.clone(), measure_page, |pages| {
            pages
                .iter()
                .filter(|page| page.source.contains(&only))
                .collect()
        });
    }
    if only.is_empty() || only == "synthetic" {
        run_synthetic(kind, extra, include, synthetic);
    }
}

fn output_path(file: &str) -> PathBuf {
    let out: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(".temp/measure");
    std::fs::create_dir_all(&out).unwrap();
    out.join(format!("{}-rust-{file}.json", common::upstream_release()))
}

fn write_document(path: &Path, extra: Value, pages: &[Value]) {
    // Same variable and default as `measure.host()` in tools/parity, so the merged table can compare the hosts
    let runtime =
        std::env::var("TRAFILATURA_MEASURE_RUNTIME").unwrap_or_else(|_| "native".to_owned());
    let document = json!({
        "pin": common::pin(),
        "upstream_version": common::upstream_release(),
        "host": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "runtime": runtime },
        "extra": extra,
        "pages": pages,
    });
    std::fs::write(path, serde_json::to_string_pretty(&document).unwrap()).unwrap();
}

/// Measures each included synthetic page once (no warm-up, no median), writes
/// `.temp/measure/<version>-rust-<kind>-synthetic.json` and panics, after writing, when an
/// outcome differs from the page's `Expect`.
fn run_synthetic(
    kind: &str,
    extra: Value,
    include: impl Fn(&Synthetic) -> bool,
    measure: impl Fn(&Synthetic) -> Value,
) {
    let mut mismatches = Vec::new();
    let pages: Vec<Value> = synthetic()
        .iter()
        .filter(|page| include(page))
        .map(|page| {
            let mut result = measure(page);
            result["name"] = json!(page.name);
            result["input_bytes"] = json!(page.html.len());
            result["expected"] = json!(page.expect.outcome());
            println!("synthetic {result}");
            if result["outcome"] != page.expect.outcome() {
                mismatches.push(page.name);
            }
            result
        })
        .collect();
    let path = output_path(&format!("{kind}-synthetic"));
    write_document(&path, extra, &pages);
    println!(
        "wrote {} synthetic pages to {}",
        pages.len(),
        path.display()
    );
    assert!(
        mismatches.is_empty(),
        "synthetic pages with an unexpected outcome: {mismatches:?}"
    );
}

/// Calls `measure` on each corpus page `select` keeps, then writes
/// `.temp/measure/<version>-rust-<file>.json`.
fn run(
    file: &str,
    extra: Value,
    measure: impl Fn(&[u8]) -> Value,
    select: impl for<'a> Fn(&'a [CorpusPage]) -> Vec<&'a CorpusPage>,
) {
    common::upstream_root().expect("upstream checkout not found");
    common::fixture_dir().expect("fixtures missing: run scripts/fixtures.sh first");
    let all = corpus().expect("the upstream checkout and the fixtures were found just above");
    let pages: Vec<Value> = select(&all)
        .into_iter()
        .map(|entry| {
            let bytes = std::fs::read(&entry.path).unwrap();
            let mut page = measure(&bytes);
            page["id"] = json!(entry.id);
            page["source"] = json!(entry.source);
            page["input_bytes"] = json!(bytes.len());
            println!("{}", entry.source);
            page
        })
        .collect();
    let path = output_path(file);
    write_document(&path, extra, &pages);
    println!("wrote {} pages to {}", pages.len(), path.display());
}
