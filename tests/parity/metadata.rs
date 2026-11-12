use super::*;

/// Metadata fields that differ from the fixtures for an explained reason: (page id, field, crate
/// value, deviation id). The crate value is the field's `to_string()` form, so a known field that
/// moves to another value fails as a port bug. Each entry carries its reason in a comment above it.
/// An entry whose field matches fails the test as stale.
pub(crate) const METADATA_KNOWN: &[(&str, &str, &str, &str)] = &[
    // dateparser date; without it upstream gives the crate's date, 2007-12-16
    (
        "tests__eval__archive.org.dimido.de.marketing.html",
        "date",
        "\"2007-12-16\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2019-02-09
    (
        "tests__eval__assabah.com.tn.article.html",
        "date",
        "\"2019-02-09\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2018-04-01
    (
        "tests__eval__diario1.com-legislativa.html",
        "date",
        "\"2018-04-01\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-04-21
    (
        "tests__eval__diarioextra.com-cidh.html",
        "date",
        "\"2022-04-21\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-02-01
    (
        "tests__eval__dtb-tennis.de-nominiert.html",
        "date",
        "\"2022-02-01\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-04-21
    (
        "tests__eval__elfaro.net-asamblea.html",
        "date",
        "\"2022-04-21\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2019-01-01
    (
        "tests__eval__football.ua.podolski.html",
        "date",
        "\"2019-01-01\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-04-19
    (
        "tests__eval__guetemala.com-nicole.html",
        "date",
        "\"2022-04-19\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-05-03
    (
        "tests__eval__lacuarta.com-loretoaravena.html",
        "date",
        "\"2022-05-03\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-04-26
    (
        "tests__eval__laestrella.com.pa-laboral.html",
        "date",
        "\"2022-04-26\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-05-04
    (
        "tests__eval__lanacion.com.ar-jugada.html",
        "date",
        "\"2022-05-04\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-01-01
    (
        "tests__eval__petri-heil-ch-hechte.html",
        "date",
        "\"2022-01-01\"",
        "D-13",
    ),
    // dateparser date; without it upstream gives 2022-01-17
    (
        "tests__eval__schweizerjaeger.ch-steinkauz.html",
        "date",
        "\"2022-01-17\"",
        "D-13",
    ),
    // Non-UTF-8, byte 0xB3: upstream reads ł (Central European), the crate reads ³ (latin-1)
    (
        "tests__eval__Unijne fundusze coraz bli%C5%BCej. Sejm zag%C5%82osowa%C5%82 _za_ - Polityka - rp.pl.html",
        "author",
        "\"PAP; Rafa³ Guz\"",
        "D-3",
    ),
    // Non-UTF-8, byte 0xB3: upstream reads ł (Central European), the crate reads ³ (latin-1)
    (
        "tests__eval__Unijne fundusze coraz bli%C5%BCej. Sejm zag%C5%82osowa%C5%82 _za_ - Polityka - rp.pl.html",
        "description",
        "\"Ostra debata w Sejmie przybli¿y³a Polskê do zatwierdzenia decyzji o zasobach w³asnych UE. Koalicji Obywatelskiej nie uda³o siê prze³o¿yæ g³osowania.\"",
        "D-3",
    ),
    // Non-UTF-8, byte 0xB3: upstream reads ł (Central European), the crate reads ³ (latin-1)
    (
        "tests__eval__Unijne fundusze coraz bli%C5%BCej. Sejm zag%C5%82osowa%C5%82 _za_ - Polityka - rp.pl.html",
        "title",
        r#""Unijne fundusze coraz bli¿ej. Sejm zag³osowa³ \"za\"""#,
        "D-3",
    ),
    // Non-UTF-8, byte 0xB1: upstream reads ± (latin-1), the crate reads ą (Central European)
    (
        "tests__eval___Ziemniaki na sz%C3%B3stej, sur%C3%B3wka na dziesi%C4%85tej_. Jak pomaga%C4%87, %C5%BCeby nie zaszkodzi%C4%87_ [PORADNIK W PIGU%C5%81CE].html",
        "description",
        "\"Jak pomóc osobie niewidomej, z którą jesteśmy restauracji? Co zrobić, jeśli w kontaktach z osobą głuchą towarzyszy nam tłumacz? Z okazji Dnia Walki z Dyskryminacją Osób z Niepełnosprawnościami przygotowaliśmy wraz z organizacjami pozarządowymi i na podstawie fachowych publikacji krótki poradnik.\"",
        "D-3",
    ),
    // Non-UTF-8, byte 0xB1: upstream reads ± (latin-1), the crate reads ą (Central European)
    (
        "tests__eval___Ziemniaki na sz%C3%B3stej, sur%C3%B3wka na dziesi%C4%85tej_. Jak pomaga%C4%87, %C5%BCeby nie zaszkodzi%C4%87_ [PORADNIK W PIGU%C5%81CE].html",
        "tags",
        r#"["Informacje z Polski, newsy, aktualności, wydarzenia"]"#,
        "D-3",
    ),
    // Non-UTF-8, byte 0xB1: upstream reads ± (latin-1), the crate reads ą (Central European)
    (
        "tests__eval___Ziemniaki na sz%C3%B3stej, sur%C3%B3wka na dziesi%C4%85tej_. Jak pomaga%C4%87, %C5%BCeby nie zaszkodzi%C4%87_ [PORADNIK W PIGU%C5%81CE].html",
        "title",
        r#""\"Ziemniaki na szóstej, surówka na dziesiątej\". Jak pomagać, żeby nie zaszkodzić? [PORADNIK W PIGUŁCE]""#,
        "D-3",
    ),
];

const METADATA_CLASSES: [&str; 5] = ["match", "decompress", "upstream-error", "known", "port-bug"];

/// A metadata field of a page that differs from the fixture.
pub(crate) struct MetaField {
    pub(crate) name: String,
    pub(crate) expected: Value,
    pub(crate) actual: Value,
    pub(crate) class: &'static str,
}

pub(crate) struct MetaPage {
    pub(crate) id: String,
    pub(crate) classes: Vec<&'static str>,
    pub(crate) valid_utf8: bool,
    /// Whether the fields were compared. False for `upstream-error`, `decompress` and a whole-page
    /// mismatch.
    pub(crate) checked: bool,
    pub(crate) fields: Vec<MetaField>,
    /// The fixture and the crate metadata of a whole-page mismatch, one side `null`.
    pub(crate) whole: Option<(Value, Value)>,
}

/// The metadata of `doc` in the shape of the fixtures: `categories` and `tags` as arrays (`None` as
/// `[]`), every other field as a string or `null`.
fn doc_metadata(doc: &Document) -> Value {
    json!({
        "title": doc.title,
        "author": doc.author,
        "url": doc.url,
        "hostname": doc.hostname,
        "description": doc.description,
        "sitename": doc.sitename,
        "date": doc.date,
        "categories": doc.categories.as_deref().unwrap_or_default(),
        "tags": doc.tags.as_deref().unwrap_or_default(),
        "fingerprint": doc.fingerprint,
        "id": doc.id,
        "license": doc.license,
        "language": doc.language,
        "image": doc.image,
        "pagetype": doc.pagetype,
        "filedate": doc.filedate,
    })
}

/// Runs one fixture page with one extraction and compares its metadata with the fixture of each
/// variant: the `html5` page and the `lxml` page.
pub(crate) fn run_metadata_page(
    upstream: &Path,
    fixtures: &Path,
    entry: &Value,
    max_date: &str,
) -> (MetaPage, MetaPage) {
    let id = entry["id"].as_str().unwrap();
    let source = entry["source"].as_str().unwrap();
    let valid_utf8 = entry["valid_utf8"].as_bool().unwrap();
    let fixture = read_json(&fixtures.join("pages").join(format!("{id}.json")));
    let has_error = |variant: Variant| fixture[variant.key()]["errors"]["metadata"].is_string();
    let actual = if has_error(Variant::Html5) && has_error(Variant::Lxml) {
        Value::Null
    } else {
        let bytes = std::fs::read(upstream.join(source)).unwrap();
        let call = Call::default().max_date(max_date);
        let extracted = Options::new()
            .with_metadata(true)
            .build()
            .bare_extraction(&Input::bytes(&bytes), &call);
        match extracted {
            Ok(doc) => doc_metadata(&doc),
            Err(ExtractError::Rejected(_)) => Value::Null,
            Err(other) => panic!("{id}: unexpected error {other:?}"),
        }
    };
    let page = |variant| metadata_page(id, source, valid_utf8, variant, &fixture, &actual);
    (page(Variant::Html5), page(Variant::Lxml))
}

/// Compares the crate metadata `actual` with the fixture of `variant`. The first class that applies
/// decides the page: `upstream-error`, `match` (equal, `null` on both sides included),
/// `decompress`, else per field `known` when `METADATA_KNOWN` lists it with this crate value and
/// `port-bug` when not (see `compare_fields`). A `null` on one side only is a whole-page `port-bug`.
fn metadata_page(
    id: &str,
    source: &str,
    valid_utf8: bool,
    variant: Variant,
    fixture: &Value,
    actual: &Value,
) -> MetaPage {
    let fixture = &fixture[variant.key()];
    let expected = &fixture["metadata"];
    let unchecked = |classes: Vec<&'static str>, whole: Option<(Value, Value)>| MetaPage {
        id: id.to_string(),
        classes,
        valid_utf8,
        checked: false,
        fields: vec![],
        whole,
    };
    if fixture["errors"]["metadata"].is_string() {
        return unchecked(vec!["upstream-error"], None);
    }
    if expected == actual {
        return MetaPage {
            id: id.to_string(),
            classes: vec!["match"],
            valid_utf8,
            checked: true,
            fields: vec![],
            whole: None,
        };
    }
    if source.ends_with(".gz") && cfg!(not(feature = "decompress")) {
        return unchecked(vec!["decompress"], None);
    }
    let Some((want, got)) = field_maps(expected, actual) else {
        return unchecked(vec!["port-bug"], Some((expected.clone(), actual.clone())));
    };
    let fields = compare_fields(id, want, got, METADATA_KNOWN);
    let mut classes: Vec<&'static str> = fields.iter().map(|f| f.class).collect();
    classes.sort_unstable();
    classes.dedup();
    if classes.is_empty() {
        classes.push("match");
    }
    MetaPage {
        id: id.to_string(),
        classes,
        valid_utf8,
        checked: true,
        fields,
        whole: None,
    }
}

/// The deviation id of a `METADATA_KNOWN` entry for this page and field.
fn deviation_of(id: &str, field: &str) -> &'static str {
    METADATA_KNOWN
        .iter()
        .find(|(page, name, _, _)| *page == id && *name == field)
        .map_or("", |(_, _, _, deviation)| deviation)
}

/// The metadata objects of a fixture page and of its crate metadata, by field name.
type FieldMaps<'a> = (&'a Map<String, Value>, &'a Map<String, Value>);

/// The field maps of a fixture page and of its crate metadata, or `None` when one side is `null`: a
/// whole-page mismatch, not a field one.
pub(crate) fn field_maps<'a>(expected: &'a Value, actual: &'a Value) -> Option<FieldMaps<'a>> {
    Some((expected.as_object()?, actual.as_object()?))
}

/// The differing fields of a fixture page and its crate metadata, over both sides' keys. A key on
/// one side only panics: both sides carry every metadata key. A differing field is `known` when
/// `known` lists its page, field and crate value, else `port-bug`.
pub(crate) fn compare_fields(
    id: &str,
    want: &Map<String, Value>,
    got: &Map<String, Value>,
    known: &[(&str, &str, &str, &str)],
) -> Vec<MetaField> {
    let names: std::collections::BTreeSet<&str> =
        want.keys().chain(got.keys()).map(String::as_str).collect();
    let mut fields = Vec::new();
    for name in names {
        let (Some(expected), Some(actual)) = (want.get(name), got.get(name)) else {
            panic!("{id}: field {name} is on one side only");
        };
        if expected == actual {
            continue;
        }
        let text = actual.to_string();
        let pinned = known
            .iter()
            .any(|(page, field, value, _)| *page == id && *field == name && *value == text);
        fields.push(MetaField {
            name: name.to_string(),
            expected: expected.clone(),
            actual: actual.clone(),
            class: if pinned { "known" } else { "port-bug" },
        });
    }
    fields
}

/// The entries of `known` that do not hold: the field matches the fixture, or the page is not in
/// `pages`, which reports a misspelt page id. Entries of a page that was not compared are not checked.
pub(crate) fn stale_known(pages: &[MetaPage], known: &[(&str, &str, &str, &str)]) -> Vec<String> {
    let differing: std::collections::HashSet<(&str, &str)> = pages
        .iter()
        .flat_map(|p| {
            p.fields
                .iter()
                .map(move |f| (p.id.as_str(), f.name.as_str()))
        })
        .collect();
    let unchecked: std::collections::HashSet<&str> = pages
        .iter()
        .filter(|p| !p.checked)
        .map(|p| p.id.as_str())
        .collect();
    known
        .iter()
        .filter(|(page, field, _, _)| {
            !differing.contains(&(*page, *field)) && !unchecked.contains(*page)
        })
        .map(|(page, field, _, _)| format!("{page} {field}"))
        .collect()
}

pub(crate) fn write_metadata_report(
    pages: &[MetaPage],
    stale: &[String],
    variant: Variant,
) -> PathBuf {
    let mut out = format!(
        "# Parity report: metadata against the {} fixtures\n\n",
        variant.key()
    );
    if variant == Variant::Lxml {
        let port_bugs = pages.iter().filter(|p| p.classes.contains(&"port-bug"));
        out.push_str(&gate_count_line("", port_bugs.map(|p| p.valid_utf8)));
        out.push('\n');
    }
    let _ = writeln!(out, "Pages: {}\n", pages.len());
    out.push_str("## Classes\n\n");
    for class in METADATA_CLASSES {
        let page_count = pages.iter().filter(|p| p.classes.contains(&class)).count();
        let field_count: usize = pages
            .iter()
            .map(|p| p.fields.iter().filter(|f| f.class == class).count())
            .sum();
        let _ = writeln!(out, "- {class}: {page_count} pages, {field_count} fields");
    }
    let known: Vec<(&MetaPage, &MetaField)> = pages
        .iter()
        .flat_map(|p| {
            p.fields
                .iter()
                .filter(|f| f.class == "known")
                .map(move |f| (p, f))
        })
        .collect();
    let mut deviations: Vec<&str> = METADATA_KNOWN.iter().map(|(_, _, _, d)| *d).collect();
    deviations.sort_unstable();
    deviations.dedup();
    out.push_str("\n## Known differences by deviation id\n\n");
    for deviation in deviations {
        let count = known
            .iter()
            .filter(|(p, f)| deviation_of(&p.id, &f.name) == deviation)
            .count();
        let _ = writeln!(out, "- {deviation}: {count} fields");
    }
    out.push_str("\n## Port-bug pages\n\n");
    for page in pages.iter().filter(|p| p.classes.contains(&"port-bug")) {
        let _ = writeln!(out, "### {}\n", page.id);
        if let Some((expected, actual)) = &page.whole {
            let _ = writeln!(out, "- whole page: expected {expected}, actual {actual}");
        }
        for f in page.fields.iter().filter(|f| f.class == "port-bug") {
            let _ = writeln!(
                out,
                "- {}: expected {}, actual {}",
                f.name, f.expected, f.actual
            );
        }
        out.push('\n');
    }
    out.push_str("## Known differences\n\n");
    for (page, f) in &known {
        let _ = writeln!(
            out,
            "- {} {} ({}): expected {}, actual {}",
            page.id,
            f.name,
            deviation_of(&page.id, &f.name),
            f.expected,
            f.actual
        );
    }
    out.push_str("\n## Stale entries\n\n");
    for entry in stale {
        let _ = writeln!(out, "- {entry}");
    }
    let dir = common::report_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(variant.report_name("metadata-report.md"));
    std::fs::write(&path, out).unwrap();
    path
}
