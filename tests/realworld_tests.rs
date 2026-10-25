#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! The text tests of upstream's `tests/realworld_tests.py` over the cached real pages of the
//! upstream checkout (`tests/cache`), found with `common::upstream_root()`; no page is copied into
//! this repository. Upstream runs them with `fast=False`; the crate has fast mode only (D-1), so an
//! assertion that fails here is checked against upstream's `fast=True` output and, where upstream
//! fails it too, asserts the fast-mode outcome with a comment citing D-1. The metadata test
//! `test_pages` is not ported.

#[allow(dead_code)]
mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use trafilatura_rust::__internals::{
    HtmlInput, MAX_FILE_SIZE, PY_WORD_CLASS, ParseLimits, load_html, normalize_unicode, splitlines,
    trim,
};
use trafilatura_rust::{Call, ExtractError, Extractor, Format, Input, Options};

// upstream: realworld_tests.py::MOCK_PAGES
const MOCK_PAGES: &[(&str, &str)] = &[
    (
        "https://die-partei.net/luebeck/2012/05/31/das-ministerium-fur-club-kultur-informiert/",
        "die-partei.net.luebeck.html",
    ),
    (
        "https://www.bmjv.de/DE/Verbraucherportal/KonsumImAlltag/TransparenzPreisanpassung/TransparenzPreisanpassung_node.html",
        "bmjv.de.konsum.html",
    ),
    (
        "https://denkanstoos.wordpress.com/2012/04/11/denkanstoos-april-2012/",
        "denkanstoos.com.2012.html",
    ),
    (
        "https://www.demokratiewebstatt.at/thema/thema-umwelt-und-klima/woher-kommt-die-dicke-luft",
        "demokratiewebstatt.at.luft.html",
    ),
    (
        "http://www.toralin.de/schmierfett-reparierend-verschlei-y-910.html",
        "toralin.de.schmierfett.html",
    ),
    (
        "https://www.ebrosia.de/beringer-zinfandel-rose-stone-cellars-lieblich-suess",
        "ebrosia.de.zinfandel.html",
    ),
    (
        "https://www.landwirt.com/Precision-Farming-Moderne-Sensortechnik-im-Kuhstall,,4229,,Bericht.html",
        "landwirt.com.sensortechnik.html",
    ),
    (
        "http://schleifen.ucoz.de/blog/briefe/2010-10-26-18",
        "schleifen.ucoz.de.briefe.html",
    ),
    (
        "https://kulu-media.com/meta-outage-hits-facebook-instagram-and-messenger/",
        "kulu-media.com.meta-outage.html",
    ),
    (
        "https://www.infobae.com/colombia/2026/08/24/terremoto-sacudio-el-bolsillo-de-los-colombianos-el-consumo-cayo-89-tras-el-fuerte-sismo-del-10-de-agosto/",
        "infobae.com.terremoto.html",
    ),
    (
        "https://www.eluniverso.com/noticias/seguridad/kenia-brindara-apoyo-para-repatriar-restos-de-michele-sensi-contugi-y-su-esposa-tras-accidente-de-helicoptero-nota/",
        "eluniverso.com.repatriar.html",
    ),
    (
        "https://www.aljazeera.com/news/2026/6/11/thousands-of-malawians-flee-homes-in-south-africa-amid-xenophobic-threats",
        "aljazeera.com.malawians.html",
    ),
    (
        "http://www.rs-ingenieure.de/de/hochbau/leistungen/tragwerksplanung",
        "rs-ingenieure.de.tragwerksplanung.html",
    ),
    (
        "http://www.simplyscience.ch/teens-liesnach-archiv/articles/wie-entsteht-erdoel.html",
        "simplyscience.ch.erdoel.html",
    ),
    (
        "http://www.shingon-reiki.de/reiki-und-schamanismus/",
        "shingon-reiki.de.schamanismus.html",
    ),
    (
        "http://love-hina.ch/news/0409.html",
        "love-hina.ch.0409.html",
    ),
    (
        "http://www.cdu-fraktion-erfurt.de/inhalte/aktuelles/entwicklung-der-waldorfschule-ermoeglicht/index.html",
        "cdu-fraktion-erfurt.de.waldorfschule.html",
    ),
    (
        "http://www.wehranlage-horka.de/veranstaltung/887/",
        "wehranlage-horka.de.887.html",
    ),
    (
        "https://piratenpartei-mv.de/blog/2013/09/12/grundeinkommen-ist-ein-menschenrecht/",
        "piratenpartei-mv.de.grundeinkommen.html",
    ),
    (
        "https://www.rnz.de/nachrichten_artikel,-zz-dpa-Schlaglichter-Frank-Witzel-erhaelt-Deutschen-Buchpreis-2015-_arid,133484.html",
        "rnz.de.witzel.html",
    ),
    (
        "https://www.austria.info/de/aktivitaten/radfahren/radfahren-in-der-weltstadt-salzburg",
        "austria.info.radfahren.html",
    ),
    ("https://www.fairkom.eu/about", "fairkom.eu.about.html"),
    (
        "https://futurezone.at/digital-life/uber-konkurrent-lyft-startet-mit-waymo-robotertaxis-in-usa/400487461",
        "futurezone.at.lyft.html",
    ),
    (
        "http://www.hundeverein-kreisunna.de/unserverein.html",
        "hundeverein-kreisunna.de.html",
    ),
    (
        "https://viehbacher.com/de/steuerrecht",
        "viehbacher.com.steuerrecht.html",
    ),
    (
        "http://www.jovelstefan.de/2011/09/11/gefallt-mir/",
        "jovelstefan.de.gefallt.html",
    ),
    (
        "https://www.stuttgart.de/item/show/132240/1",
        "stuttgart.de.html",
    ),
    (
        "https://www.otto.de/twoforfashion/strohtasche/",
        "otto.de.twoforfashion.html",
    ),
    (
        "http://www.womencantalksports.com/top-10-women-talking-sports/",
        "womencantalksports.com.top10.html",
    ),
    (
        "https://www.luxuryhaven.co/2019/05/nam-nghi-phu-quoc-unbound-collection-by-hyatt-officially-opens.html",
        "luxuryhaven.co.hyatt.html",
    ),
    (
        "https://www.luxuriousmagazine.com/2019/06/royal-salute-polo-rome/",
        "luxuriousmagazine.com.polo.html",
    ),
    (
        "https://www.chip.de/tests/akkuschrauber-werkzeug-co,82197/5",
        "chip.de.tests.html",
    ),
    (
        "https://www.gruen-digital.de/2015/01/digitalpolitisches-jahrestagung-2015-der-heinrich-boell-stiftung-baden-wuerttemberg/",
        "gruen-digital.de.jahrestagung.html",
    ),
    (
        "https://www.rechtambild.de/2011/10/bgh-marions-kochbuch-de/",
        "rechtambild.de.kochbuch.html",
    ),
    (
        "http://www.internet-law.de/2011/07/verstost-der-ausschluss-von-pseudonymen-bei-google-gegen-deutsches-recht.html",
        "internet-law.de.pseudonymen.html",
    ),
    (
        "https://correctiv.org/aktuelles/neue-rechte/2019/05/14/wir-haben-bereits-die-zusage",
        "correctiv.org.zusage.html",
    ),
    (
        "https://www.sueddeutsche.de/wirtschaft/bahn-flixbus-flixtrain-deutschlandtakt-fernverkehr-1.4445845",
        "sueddeutsche.de.flixtrain.html",
    ),
    (
        "https://www.adac.de/rund-ums-fahrzeug/tests/kindersicherheit/kindersitztest-2018/",
        "adac.de.kindersitze.html",
    ),
    (
        "https://www.caktusgroup.com/blog/2015/06/08/testing-client-side-applications-django-post-mortem/",
        "caktusgroup.com.django.html",
    ),
    (
        "https://www.basicthinking.de/blog/2018/12/05/erfolgreiche-tweets-zutaten/",
        "basicthinking.de.tweets.html",
    ),
    (
        "https://www.incurvy.de/trends-grosse-groessen/wellness-gesichtsbehandlung-plaisir-daromes/",
        "incurvy.de.wellness.html",
    ),
    (
        "https://www.dw.com/en/uncork-the-mystery-of-germanys-fr%C3%BChburgunder/a-16863843",
        "dw.com.uncork.html",
    ),
    (
        "https://www.jolie.de/stars/adele-10-kilo-abgenommen-sie-zeigt-sich-schlanker-denn-je-200226.html",
        "jolie.de.adele.html",
    ),
    (
        "https://www.speicherguide.de/digitalisierung/faktor-mensch/schwierige-gespraeche-so-gehts-24376.aspx",
        "speicherguide.de.schwierige.html",
    ),
    (
        "https://novalanalove.com/ear-candy/",
        "novalanalove.com.ear-candy.html",
    ),
    (
        "http://www.franziska-elea.de/2019/02/10/das-louis-vuitton-missgeschick/",
        "franziska-elea.de.vuitton.html",
    ),
    (
        "https://www.brigitte.de/liebe/persoenlichkeit/ikigai-macht-dich-sofort-gluecklicher--10972896.html",
        "brigitte.de.ikigai.html",
    ),
    (
        "https://www.changelog.blog/zwischenbilanz-jan-kegelberg-ueber-tops-und-flops-bei-der-transformation-von-sportscheck/",
        "changelog.blog.zwischenbilanz.html",
    ),
    (
        "https://threatpost.com/android-ransomware-spreads-via-sex-simulation-game-links-on-reddit-sms/146774/",
        "threatpost.com.android.html",
    ),
    (
        "https://www.theverge.com/2019/7/3/20680681/ios-13-beta-3-facetime-attention-correction-eye-contact",
        "theverge.com.ios13.html",
    ),
    (
        "https://en.wikipedia.org/wiki/T-distributed_stochastic_neighbor_embedding",
        "en.wikipedia.org.tsne.html",
    ),
    (
        "https://mixed.de/vrodo-deals-vr-taugliches-notebook-fuer-83215-euro-99-cent-leihfilme-bei-amazon-psvr/",
        "mixed.de.vrodo.html",
    ),
    (
        "https://majkaswelt.com/top-5-fashion-must-haves-2018-werbung/",
        "majkaswelt.com.fashion.html",
    ),
    (
        "https://erp-news.info/erp-interview-mit-um-digitale-assistenten-und-kuenstliche-intelligenz-ki/",
        "erp-news.info.interview.html",
    ),
    (
        "https://lady50plus.de/2019/06/19/sekre-mystery-bag/",
        "lady50plus.de.sekre.html",
    ),
    (
        "https://www.psl.eu/actualites/luniversite-psl-quand-les-grandes-ecoles-font-universite",
        "psl.eu.luniversite.html",
    ),
    (
        "http://www.sauvonsluniversite.fr/spip.php?article8532",
        "sauvonsluniversite.com.spip.html",
    ),
    (
        "https://www.franceculture.fr/emissions/le-journal-des-idees/le-journal-des-idees-emission-du-mardi-14-janvier-2020",
        "franceculture.fr.idees.html",
    ),
    (
        "https://vancouversun.com/technology/microsoft-moves-to-erase-its-carbon-footprint-from-the-atmosphere-in-climate-push/wcm/76e426d9-56de-40ad-9504-18d5101013d2",
        "vancouversun.com.microsoft.html",
    ),
    (
        "https://www.lanouvellerepublique.fr/indre-et-loire/commune/saint-martin-le-beau/family-park-la-derniere-saison-a-saint-martin-le-beau",
        "lanouvellerepublique.fr.martin.html",
    ),
    (
        "http://blog.python.org/2016/12/python-360-is-now-available.html",
        "blog.python.org.html",
    ),
    (
        "https://creativecommons.org/about/",
        "creativecommons.org.html",
    ),
    (
        "https://www.creativecommons.at/faircoin-hackathon",
        "creativecommons.at.faircoin.html",
    ),
    (
        "https://en.blog.wordpress.com/2019/06/19/want-to-see-a-more-diverse-wordpress-contributor-community-so-do-we/",
        "blog.wordpress.com.diverse.html",
    ),
    (
        "https://netzpolitik.org/2016/die-cider-connection-abmahnungen-gegen-nutzer-von-creative-commons-bildern/",
        "netzpolitik.org.abmahnungen.html",
    ),
    (
        "https://www.befifty.de/home/2017/7/12/unter-uns-montauk",
        "befifty.montauk.html",
    ),
    (
        "https://www.soundofscience.fr/1927",
        "soundofscience.fr.1927.html",
    ),
    (
        "https://laviedesidees.fr/L-evaluation-et-les-listes-de.html",
        "laviedesidees.fr.evaluation.html",
    ),
    (
        "https://www.theguardian.com/education/2020/jan/20/thousands-of-uk-academics-treated-as-second-class-citizens",
        "theguardian.com.academics.html",
    ),
    (
        "https://phys.org/news/2019-10-flint-flake-tool-partially-birch.html",
        "phys.org.tool.html",
    ),
    (
        "https://gregoryszorc.com/blog/2020/01/13/mercurial%27s-journey-to-and-reflections-on-python-3/",
        "gregoryszorc.com.python3.html",
    ),
    (
        "https://www.pluralsight.com/tech-blog/managing-python-environments/",
        "pluralsight.com.python.html",
    ),
    (
        "https://stackoverflow.blog/2020/01/20/what-is-rust-and-why-is-it-so-popular/",
        "stackoverflow.com.rust.html",
    ),
    (
        "https://www.dw.com/en/berlin-confronts-germanys-colonial-past-with-new-initiative/a-52060881",
        "dw.com.colonial.html",
    ),
    (
        "https://www.theplanetarypress.com/2020/01/management-of-intact-forestlands-by-indigenous-peoples-key-to-protecting-climate/",
        "theplanetarypress.com.forestlands.html",
    ),
    (
        "https://wikimediafoundation.org/news/2020/01/15/access-to-wikipedia-restored-in-turkey-after-more-than-two-and-a-half-years/",
        "wikimediafoundation.org.turkey.html",
    ),
    (
        "https://www.reuters.com/article/us-awards-sag/parasite-scores-upset-at-sag-awards-boosting-oscar-chances-idUSKBN1ZI0EH",
        "reuters.com.parasite.html",
    ),
    (
        "https://www.nationalgeographic.co.uk/environment-and-conservation/2020/01/ravenous-wild-goats-ruled-island-over-century-now-its-being",
        "nationalgeographic.co.uk.goats.html",
    ),
    (
        "https://www.nature.com/articles/d41586-019-02790-3",
        "nature.com.telescope.html",
    ),
    (
        "https://www.salon.com/2020/01/10/despite-everything-u-s-emissions-dipped-in-2019_partner/",
        "salon.com.emissions.html",
    ),
    (
        "https://www.gofeminin.de/abnehmen/wie-kann-ich-schnell-abnehmen-s1431651.html",
        "gofeminin.de.abnehmen.html",
    ),
    (
        "https://crazy-julia.de/beauty-tipps-die-jede-braut-kennen-sollte/",
        "crazy-julia.de.tipps.html",
    ),
    (
        "https://www.politische-bildung-brandenburg.de/themen/land-und-leute/homo-brandenburgensis",
        "brandenburg.de.homo-brandenburgensis.html",
    ),
    (
        "https://skateboardmsm.de/news/the-captains-quest-2017-contest-auf-schwimmender-miniramp-am-19-august-in-dormagen.html",
        "skateboardmsm.de.dormhagen.html",
    ),
    (
        "https://knowtechie.com/rocket-pass-4-in-rocket-league-brings-with-it-a-new-rally-inspired-car/",
        "knowtechie.com.rally.html",
    ),
    (
        "https://boingboing.net/2013/07/19/hating-millennials-the-preju.html",
        "boingboing.net.millenials.html",
    ),
    (
        "http://www.spreeblick.com/blog/2006/07/29/aus-aus-alles-vorbei-habeck-macht-die-stahnke/",
        "spreeblick.com.habeck.html",
    ),
    (
        "https://github.blog/2019-03-29-leader-spotlight-erin-spiceland/",
        "github.blog.spiceland.html",
    ),
    (
        "https://www.sonntag-sachsen.de/emanuel-scobel-wird-thomanerchor-geschaeftsfuehrer",
        "sonntag-sachsen.de.emanuel.html",
    ),
    (
        "https://www.spiegel.de/spiegel/print/d-161500790.html",
        "spiegel.de.albtraum.html",
    ),
    (
        "https://lemire.me/blog/2019/08/02/json-parsing-simdjson-vs-json-for-modern-c/",
        "lemire.me.json.html",
    ),
    (
        "https://www.zeit.de/mobilitaet/2020-01/zugverkehr-christian-lindner-hochgeschwindigkeitsstrecke-eu-kommission",
        "zeit.de.zugverkehr.html",
    ),
    (
        "https://www.computerbase.de/2007-06/htc-touch-bald-bei-o2-als-xda-nova/",
        "computerbase.de.htc.html",
    ),
    (
        "http://www.chineselyrics4u.com/2011/07/zhi-neng-xiang-nian-ni-jam-hsiao-jing.html",
        "chineselyrics4u.com.zhineng.html",
    ),
    (
        "https://meedia.de/2016/03/08/einstieg-ins-tv-geschaeft-wie-freenet-privatkunden-fuer-antennen-tv-in-hd-qualitaet-gewinnen-will/",
        "meedia.de.freenet.html",
    ),
    (
        "https://www.telemedicus.info/article/2766-Rezension-Haerting-Internetrecht,-5.-Auflage-2014.html",
        "telemedicus.info.rezension.html",
    ),
    (
        "https://www.cnet.de/88130484/so-koennen-internet-user-nach-dem-eugh-urteil-fuer-den-schutz-sensibler-daten-sorgen",
        "cnet.de.schutz.html",
    ),
    (
        "https://www.vice.com/en_uk/article/d3avvm/the-amazon-is-on-fire-and-the-smoke-can-be-seen-from-space",
        "vice.com.amazon.html",
    ),
    (
        "https://www.heise.de/newsticker/meldung/Lithium-aus-dem-Schredder-4451133.html",
        "heise.de.lithium.html",
    ),
    (
        "https://www.chip.de/test/Beef-Maker-von-Aldi-im-Test_154632771.html",
        "chip.de.beef.html",
    ),
    (
        "https://plentylife.blogspot.com/2017/05/strong-beautiful-pamela-reif-rezension.html",
        "plentylife.blogspot.pamela-reif.html",
    ),
    (
        "https://www.modepilot.de/2019/05/21/geht-euch-auch-so-oder-auf-reisen-nie-ohne-meinen-duschkopf/",
        "modepilot.de.duschkopf.html",
    ),
    (
        "http://iloveponysmag.com/2018/05/24/barbour-coastal/",
        "iloveponysmag.com.barbour.html",
    ),
    (
        "https://moritz-meyer.net/blog/vreni-frost-instagram-abmahnung/",
        "moritz-meyer.net.vreni.html",
    ),
    (
        "https://scilogs.spektrum.de/engelbart-galaxis/die-ablehnung-der-gendersprache/",
        "spektrum.de.engelbart.html",
    ),
    (
        "https://buchperlen.wordpress.com/2013/10/20/leandra-lou-der-etwas-andere-modeblog-jetzt-auch-zwischen-buchdeckeln/",
        "buchperlen.wordpress.com.html",
    ),
    (
        "http://kulinariaathome.wordpress.com/2012/12/08/mandelplatzchen/",
        "kulinariaathome.com.mandelplätzchen.html",
    ),
    (
        "https://de.creativecommons.org/index.php/2014/03/20/endlich-wird-es-spannend-die-nc-einschraenkung-nach-deutschem-recht/",
        "de.creativecommons.org.endlich.html",
    ),
    (
        "https://www.scmp.com/comment/opinion/article/3046526/taiwanese-president-tsai-ing-wens-political-playbook-should-be",
        "scmp.com.playbook.html",
    ),
    (
        "https://www.faz.net/aktuell/wirtschaft/nutzerbasierte-abrechnung-musik-stars-fordern-neues-streaming-modell-16604622.html",
        "faz.net.streaming.html",
    ),
    (
        "https://www.ndr.de/nachrichten/info/16-Coronavirus-Update-Wir-brauchen-Abkuerzungen-bei-der-Impfstoffzulassung,podcastcoronavirus140.html",
        "ndr.de.podcastcoronavirus140.html",
    ),
    (
        "https://www.mercurynews.com/2023/01/16/letters-1119/",
        "mercurynews.com.2023.01.16.letters-1119.html",
    ),
    (
        "http://www.pcgamer.com/2012/08/09/skyrim-part-1/",
        "pcgamer.com.skyrim.html",
    ),
];

/// A cached page: text when its bytes are valid UTF-8, else the bytes, which the crate's own
/// detector decodes (upstream's test decodes with charset_normalizer there, D-3).
enum Page {
    Text(String),
    Bytes(Vec<u8>),
}

impl Page {
    fn input(&self) -> Input<'_> {
        match self {
            Page::Text(text) => Input::text(text),
            Page::Bytes(bytes) => Input::bytes(bytes),
        }
    }

    fn html_input(&self) -> HtmlInput<'_> {
        match self {
            Page::Text(text) => HtmlInput::Str(text),
            Page::Bytes(bytes) => HtmlInput::Bytes(bytes),
        }
    }
}

/// The upstream checkout, or `None` (skip) when it is absent and fixtures are not required.
fn upstream() -> Option<PathBuf> {
    let root = common::upstream_root();
    if root.is_none() {
        assert!(
            !common::require_fixtures(),
            "PARITY_REQUIRE_FIXTURES is set but the upstream checkout is missing"
        );
        eprintln!(
            "realworld_tests: the upstream checkout is missing; set TRAFILATURA_UPSTREAM or put it at ../trafilatura to enable this test"
        );
    }
    root
}

// upstream: realworld_tests.py::load_mock_page_meta
fn read_page(root: &Path, url: &str) -> Page {
    let file = MOCK_PAGES
        .iter()
        .find(|(known, _)| *known == url)
        .map(|(_, file)| *file)
        .unwrap_or_else(|| panic!("{url} is not in MOCK_PAGES"));
    let bytes = std::fs::read(root.join("tests/cache").join(file)).unwrap();
    match String::from_utf8(bytes) {
        Ok(text) => Page::Text(text),
        Err(error) => Page::Bytes(error.into_bytes()),
    }
}

/// What `extract` returns, with upstream's `None` as `None`; any other error is a failure.
fn extract(extractor: &Extractor, input: &Input<'_>, url: &str) -> Option<String> {
    match extractor.extract(input, &Call::default()) {
        Ok(text) => Some(text),
        Err(ExtractError::Rejected(_)) => None,
        Err(error) => panic!("{url}: extract failed: {error:?}"),
    }
}

static MARKDOWN_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\([^)]*\)").unwrap());
static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{PY_WORD_CLASS}]+")).unwrap());

/// Word 10-grams within each line, markdown link targets and soft hyphens removed.
// upstream: realworld_tests.py::_ngrams
fn ngrams(text: &str) -> HashMap<String, usize> {
    const N: usize = 10;
    let text = text.replace('\u{ad}', "").to_lowercase();
    let text = MARKDOWN_TARGET.replace_all(&text, "]");
    let mut grams = HashMap::new();
    for line in splitlines(&text) {
        let tokens: Vec<&str> = WORD.find_iter(line).map(|found| found.as_str()).collect();
        for window in tokens.windows(N) {
            *grams.entry(window.join(" ")).or_insert(0) += 1;
        }
    }
    grams
}

/// Source text without scripts and styles, whitespace-collapsed and NFC like the output.
// upstream: realworld_tests.py::_source_text
fn source_text(page: &Page) -> String {
    let mut tree = load_html(
        page.html_input(),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let root = tree.root();
    tree.strip_elements(root, &["script", "style"], false);
    let text = tree.itertext_lxml(root).collect::<Vec<_>>().join(" ");
    normalize_unicode(&trim(&text)).into_owned()
}

/// Text segments must not be output more often than they appear in the source (GH#951).
// upstream: realworld_tests.py::test_no_repeated_segments
#[test]
fn test_no_repeated_segments() {
    let Some(root) = upstream() else { return };
    let default = Options::new().build();
    let markdown = Options::new()
        .format(Format::Markdown)
        .formatting(true)
        .links(true)
        .build();
    let mut failures = Vec::new();
    for (url, _) in MOCK_PAGES {
        let page = read_page(&root, url);
        let source = ngrams(&source_text(&page));
        for (name, extractor) in [("default", &default), ("markdown", &markdown)] {
            let output =
                ngrams(&extract(extractor, &page.input().url(url), url).unwrap_or_default());
            let mut repeated: Vec<&String> = output
                .iter()
                .filter(|(gram, count)| {
                    let in_source = source.get(*gram).copied().unwrap_or(0);
                    0 < in_source && in_source < **count
                })
                .map(|(gram, _)| gram)
                .collect();
            // a map has no insertion order: sorted, the three listed are stable
            repeated.sort();
            if !repeated.is_empty() {
                repeated.truncate(3);
                failures.push(format!("{url} ({name}): {repeated:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} pages repeat a segment:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// One extraction config over the pages; collects the failed assertions instead of stopping at the
/// first, so one run lists them all.
struct Checks<'a> {
    root: &'a Path,
    extractor: Extractor,
    label: &'static str,
    url: &'static str,
    result: Option<String>,
    failures: Vec<String>,
}

impl<'a> Checks<'a> {
    fn new(root: &'a Path, options: &Options, label: &'static str) -> Self {
        Self {
            root,
            extractor: options.build(),
            label,
            url: "",
            result: None,
            failures: Vec::new(),
        }
    }

    // upstream: realworld_tests.py::load_mock_page
    fn load(&mut self, url: &'static str) {
        let page = read_page(self.root, url);
        let input = page.input().url(url).record_id("0000");
        self.url = url;
        self.result = extract(&self.extractor, &input, url);
    }

    /// `needle in result` for upstream; a `None` result raises there, so it fails here.
    fn check(&mut self, needle: &str, expect_present: bool) {
        let holds = self
            .result
            .as_ref()
            .is_some_and(|result| result.contains(needle) == expect_present);
        if !holds {
            let kind = if expect_present { "missing" } else { "present" };
            let size = self.result.as_ref().map(String::len);
            self.failures.push(format!(
                "[{}] {}: {kind} {needle:?} (result length {size:?})",
                self.label, self.url
            ));
        }
    }

    fn has(&mut self, needle: &str) {
        self.check(needle, true);
    }

    fn lacks(&mut self, needle: &str) {
        self.check(needle, false);
    }
}

fn assert_no_failures(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} assertions fail:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The `xmloutput=False` variants of upstream's `test_extract`: formatting off, then on. An
/// upstream assertion made for `xmloutput is False` or unconditionally is made here, in its order.
// upstream: realworld_tests.py::test_extract
#[test]
fn test_extract() {
    let Some(root) = upstream() else { return };
    let mut failures = Vec::new();
    for formatting in [false, true] {
        let options = Options::new().format(Format::Txt).formatting(formatting);
        let label = if formatting { "formatting" } else { "plain" };
        let mut c = Checks::new(&root, &options, label);
        extract_cases(&mut c, formatting);
        failures.append(&mut c.failures);
    }
    assert_no_failures(&failures);
}

#[test]
#[ignore = "phase 6"]
fn test_extract_xml() {
    // todo(phase 6): xml
}

// upstream: realworld_tests.py::test_extract_links_formatting
#[test]
fn test_extract_links_formatting() {
    let Some(root) = upstream() else { return };
    let options = Options::new()
        .format(Format::Txt)
        .formatting(true)
        .links(true);
    let mut c = Checks::new(&root, &options, "formatting and links");
    c.load("http://www.pcgamer.com/2012/08/09/skyrim-part-1/");
    c.has("In [Skyrim](https://www.pcgamer.com/best-skyrim-mods/), a mage");
    // the source has a trailing space inside the em tag; it must stay outside the markers (valid CommonMark)
    c.has("*Legends* don't destroy *houses*,");
    assert_no_failures(&c.failures);
}

#[allow(clippy::too_many_lines)]
fn extract_cases(c: &mut Checks<'_>, formatting: bool) {
    c.load("https://die-partei.net/luebeck/2012/05/31/das-ministerium-fur-club-kultur-informiert/");
    c.lacks("Impressum");
    c.has("Die GEMA dreht völlig am Zeiger!");
    c.load("https://www.bmjv.de/DE/Verbraucherportal/KonsumImAlltag/TransparenzPreisanpassung/TransparenzPreisanpassung_node.html");
    c.lacks("Impressum");
    c.has("Anbieter von Fernwärme haben innerhalb ihres Leitungsnetzes ein Monopol");
    c.load("https://denkanstoos.wordpress.com/2012/04/11/denkanstoos-april-2012/");
    c.has("Two or three 10-15 min");
    c.has("What type? Etc. (30 mins)");
    c.lacks("Dieser Eintrag wurde veröffentlicht");
    c.lacks("Mit anderen Teillen");
    c.load("https://www.ebrosia.de/beringer-zinfandel-rose-stone-cellars-lieblich-suess");
    c.has("Das Bukett präsentiert sich");
    c.lacks("Kunden kauften auch");
    c.lacks("Gutschein sichern");
    c.has("Besonders gut passt er zu asiatischen Gerichten");
    c.load("https://www.landwirt.com/Precision-Farming-Moderne-Sensortechnik-im-Kuhstall,,4229,,Bericht.html");
    c.has("Überwachung der somatischen Zellen");
    c.has("tragbaren Ultraschall-Geräten");
    c.has("Kotkonsistenz");
    c.lacks("Anzeigentarife");
    c.lacks("Aktuelle Berichte aus dieser Kategorie");
    c.load("http://www.rs-ingenieure.de/de/hochbau/leistungen/tragwerksplanung");
    c.has("Wir bearbeiten alle Leistungsbilder");
    c.lacks("Brückenbau");
    c.load("http://www.shingon-reiki.de/reiki-und-schamanismus/");
    c.lacks("Catch Evolution");
    c.lacks("und gekennzeichnet mit");
    c.has("Heut geht es");
    c.has("Ich komme dann zu dir vor Ort.");
    c.load("http://love-hina.ch/news/0409.html");
    c.has("Kapitel 121 ist");
    c.lacks("Besucher online");
    c.lacks("Kommentare schreiben");
    c.load("http://www.cdu-fraktion-erfurt.de/inhalte/aktuelles/entwicklung-der-waldorfschule-ermoeglicht/index.html");
    c.has("der steigenden Nachfrage gerecht zu werden.");
    c.lacks("Zurück zur Übersicht");
    c.lacks("Erhöhung für Zoo-Eintritt");
    c.load("https://de.creativecommons.org/index.php/2014/03/20/endlich-wird-es-spannend-die-nc-einschraenkung-nach-deutschem-recht/");
    c.has("das letzte Wort sein kann.");
    c.lacks("Ähnliche Beiträge");
    c.load("https://piratenpartei-mv.de/blog/2013/09/12/grundeinkommen-ist-ein-menschenrecht/");
    c.has("Unter diesem Motto findet am 14. September");
    c.has("Volksinitiative Schweiz zum Grundeinkommen.");
    c.lacks("getaggt mit:");
    c.lacks("Was denkst du?");
    c.load("https://scilogs.spektrum.de/engelbart-galaxis/die-ablehnung-der-gendersprache/");
    c.has("Zweitens wird der Genderstern");
    c.has("alldem leider – nichts.");
    c.load("http://www.wehranlage-horka.de/veranstaltung/887/");
    c.has("In eine andere Zeit");
    c.has("Während Sie über den Markt schlendern");
    c.lacks("Infos zum Verein");
    c.lacks("nach oben");
    c.lacks("Datenschutzerklärung");
    c.load(
        "https://www.demokratiewebstatt.at/thema/thema-umwelt-und-klima/woher-kommt-die-dicke-luft",
    );
    c.has("Millionen Menschen fahren jeden Tag");
    c.lacks("Clipdealer");
    c.lacks("Teste dein Wissen");
    c.lacks("Thema: Fußball");
    c.load("http://www.simplyscience.ch/teens-liesnach-archiv/articles/wie-entsteht-erdoel.html");
    c.has("Erdöl bildet nach Millionen");
    c.has("Warum wird das Erdöl knapp?");
    c.lacks("Die Natur ist aus chemischen Elementen aufgebaut");
    c.load("https://www.rnz.de/nachrichten_artikel,-zz-dpa-Schlaglichter-Frank-Witzel-erhaelt-Deutschen-Buchpreis-2015-_arid,133484.html");
    // D-1: the slow default (fast=False) keeps both sentences, fast mode drops them
    // oracle: extract(page, url, fast=True) contains "Für einen Roman" -> False, "Auszeichnung der Branche." -> False
    c.lacks("Für einen Roman");
    c.lacks("Auszeichnung der Branche.");
    c.load("https://buchperlen.wordpress.com/2013/10/20/leandra-lou-der-etwas-andere-modeblog-jetzt-auch-zwischen-buchdeckeln/");
    c.has("Dann sollten Sie erst recht");
    c.has("als saure Gürkchen entlarvte Ex-Boyfriends.");
    c.lacks("Ähnliche Beiträge");
    c.load("http://www.toralin.de/schmierfett-reparierend-verschlei-y-910.html");
    c.has("künftig das XADO-Schutzfett verwenden.");
    c.has("bis zu 50% Verschleiß.");
    c.has("Die Lebensdauer von Bauteilen erhöht sich beträchtlich.");
    c.lacks("Newsletter");
    c.lacks("Sie könnten auch an folgenden Artikeln interessiert sein");
    c.load("https://www.fairkom.eu/about");
    c.has("ein gemeinwohlorientiertes Partnerschaftsnetzwerk");
    c.has("Stimmberechtigung bei der Generalversammlung.");
    c.lacks("support@fairkom.eu");
    c.load("https://futurezone.at/digital-life/uber-konkurrent-lyft-startet-mit-waymo-robotertaxis-in-usa/400487461");
    c.has("Einige Kunden des Fahrdienst-Vermittler Lyft");
    c.has("zeitweise rund vier Prozent.");
    c.lacks("Allgemeine Nutzungsbedingungen");
    c.lacks("Waymo bittet Autohersteller um Geld");
    c.load("http://www.hundeverein-kreisunna.de/unserverein.html");
    c.has("Beate und Norbert Olschewski");
    c.has("ein Familienmitglied und unser Freund.");
    c.lacks("zurück zur Startseite");
    c.load("https://viehbacher.com/de/steuerrecht");
    c.has("und wirtschaftlich orientierte Privatpersonen");
    c.has("rund um die Uhr.");
    c.has("Mensch im Mittelpunkt.");
    c.lacks("Was sind Cookies?");
    c.load("http://www.jovelstefan.de/2011/09/11/gefallt-mir/");
    c.has("Manchmal überrascht einen");
    c.has("kein Meisterwerk war!");
    c.lacks("Pingback von");
    c.lacks("Kommentare geschlossen");
    c.load("https://www.stuttgart.de/item/show/132240/1");
    c.has("Das Bohnenviertel entstand");
    c.has("sich herrlich entspannen.");
    c.lacks("Nützliche Links");
    c.lacks("Mehr zum Thema");
    c.load("http://kulinariaathome.wordpress.com/2012/12/08/mandelplatzchen/");
    c.has("zu einem glatten Teig verarbeiten.");
    c.has("goldbraun sind.");
    c.has("200 g Zucker");
    c.has("Ein Backblech mit Backpapier auslegen.");
    c.lacks("Sei der Erste");
    c.lacks("Gefällt mir");
    c.lacks("Trotz sorgfältiger inhaltlicher Kontrolle");
    c.load("https://kulu-media.com/meta-outage-hits-facebook-instagram-and-messenger/");
    c.has("Many users reported being automatically logged out of their accounts");
    c.lacks("Artemis");
    c.lacks("splashdown");
    c.load("https://www.infobae.com/colombia/2026/08/24/terremoto-sacudio-el-bolsillo-de-los-colombianos-el-consumo-cayo-89-tras-el-fuerte-sismo-del-10-de-agosto/");
    c.has("el consumo real del país cayó 8,9%");
    c.lacks("PUBLICIDAD");
    c.load("https://www.eluniverso.com/noticias/seguridad/kenia-brindara-apoyo-para-repatriar-restos-de-michele-sensi-contugi-y-su-esposa-tras-accidente-de-helicoptero-nota/");
    c.has("Kenia facilitará el apoyo necesario");
    c.lacks("Publicidad");
    c.load("https://www.aljazeera.com/news/2026/6/11/thousands-of-malawians-flee-homes-in-south-africa-amid-xenophobic-threats");
    c.has("More than 3,000 Malawians");
    c.lacks("Recommended Stories");
    c.lacks("Fearful foreign nationals in South Africa forced out of their homes");
    c.load("http://schleifen.ucoz.de/blog/briefe/2010-10-26-18");
    c.has("Es war gesagt,");
    c.has("Symbol auf dem Finger haben");
    c.lacks("Aufrufe:");
    c.load("https://www.austria.info/de/aktivitaten/radfahren/radfahren-in-der-weltstadt-salzburg");
    c.has("Salzburg liebt seine Radfahrer.");
    c.has("Puls einsaugen zu lassen.");
    c.lacks("Das könnte Sie auch interessieren ...");
    c.lacks("So macht Radfahren sonst noch Spaß");
    c.load("https://www.modepilot.de/2019/05/21/geht-euch-auch-so-oder-auf-reisen-nie-ohne-meinen-duschkopf/");
    c.has("Allerdings sieht es wie ein Dildo aus,");
    c.has("gibt Bescheid, ne?");
    c.lacks("Ähnliche Beiträge");
    c.lacks("Deine E-Mail (bleibt natürlich unter uns)");
    c.load("https://www.otto.de/twoforfashion/strohtasche/");
    c.has("Ob rund oder kastenförmig, ob dezent oder auffällig");
    c.has("XX, Die Redaktion");
    c.lacks(" Kommentieren");
    c.lacks("Dienstag, 4. Juni 2019");
    c.load("http://iloveponysmag.com/2018/05/24/barbour-coastal/");
    c.has("Eine meiner besten Entscheidungen bisher:");
    c.has("Verlassenes Gewächshaus meets versteckter Deich");
    c.has("Der Hundestrand in Stein an der Ostsee");
    c.lacks("Tags: Barbour,");
    c.has("Bitte (noch) mehr Bilder von Helle");
    c.lacks("Hinterlasse einen Kommentar");
    c.load("https://moritz-meyer.net/blog/vreni-frost-instagram-abmahnung/");
    c.has("Das ist alles nicht gekennzeichnet, wie soll ich wissen");
    c.has("Instagramshops machen es Abmahnanwälten leicht");
    c.lacks("Diese Geschichte teilen");
    c.lacks("Ähnliche Beiträge ");
    c.has("Ich bin der Ansicht, abwarten und Tee trinken.");
    c.has("Danke für dein Feedback. Auch zum Look meiner Seite.");
    c.lacks("Diese Website verwendet Akismet, um Spam zu reduzieren.");
    c.load("http://www.womencantalksports.com/top-10-women-talking-sports/");
    c.has("Keep Talking Sports!");
    c.lacks("Category: Blog Popular");
    c.lacks("Copyright Women Can Talk Sports.");
    c.lacks("Submit your sports question below");
    c.has("3.Charlotte Jones Anderson");
    c.load("https://plentylife.blogspot.com/2017/05/strong-beautiful-pamela-reif-rezension.html");
    c.has("Schönheit kommt für Pamela von Innen und Außen");
    c.has("Die Workout Übungen kannte ich bereits");
    c.has("Great post, I like your blog");
    c.lacks("Links zu diesem Post");
    c.lacks("mehr über mich ♥");
    c.lacks("Bitte beachte auch die Datenschutzerklärung von Google.");
    c.load("https://www.luxuryhaven.co/2019/05/nam-nghi-phu-quoc-unbound-collection-by-hyatt-officially-opens.html");
    c.has("Grounded in sustainable architecture and refined Vietnamese craftsmanship,");
    c.has("and Carmelo Resort");
    c.has("OMG what a beautiful place to stay! ");
    c.lacks("Food Advertising by");
    c.has("Dining and Drinking");
    c.lacks("A lovely note makes a beautiful day!");
    c.load("https://www.luxuriousmagazine.com/2019/06/royal-salute-polo-rome/");
    c.has("Argentina, the birthplace of polo.");
    c.has("Simon Wittenberg travels to the Eternal City in Italy");
    c.lacks("Luxury and lifestyle articles");
    c.lacks("Pinterest");
    c.load("https://www.chip.de/tests/akkuschrauber-werkzeug-co,82197/5");
    c.has("Werkzeug für Heimwerker und Baumarkt-Artikel");
    c.lacks("Newsletter");
    c.load("https://www.gruen-digital.de/2015/01/digitalpolitisches-jahrestagung-2015-der-heinrich-boell-stiftung-baden-wuerttemberg/");
    c.has("Prof. Dr. Caja Thimm");
    c.has("zur Anmeldung.");
    c.lacks("Next post");
    c.lacks("Aus den Ländern");
    c.load("https://www.rechtambild.de/2011/10/bgh-marions-kochbuch-de/");
    c.has("Leitsätze des Gerichts");
    // upstream asserts the non-empty literal "III. Die Revision der Beklagten", which is always true
    c.lacks("twittern");
    c.lacks("Ähnliche Beiträge");
    c.lacks("d.toelle[at]rechtambild.de");
    c.load("http://www.internet-law.de/2011/07/verstost-der-ausschluss-von-pseudonymen-bei-google-gegen-deutsches-recht.html");
    c.has("Wann Blogs einer Impressumspflicht unterliegen,");
    c.lacks("Über mich");
    c.lacks("Gesetzes- und Rechtsprechungszitate werden automatisch");
    c.has("Mit Verlaub, ich halte das für groben Unsinn.");
    c.load("https://www.telemedicus.info/article/2766-Rezension-Haerting-Internetrecht,-5.-Auflage-2014.html");
    c.has("Aufbau und Inhalt");
    c.has("Verlag Dr. Otto Schmidt");
    c.lacks("Handbuch");
    c.lacks("Drucken");
    c.lacks("Ähnliche Artikel");
    c.lacks("Anzeige:");
    c.load("https://www.cnet.de/88130484/so-koennen-internet-user-nach-dem-eugh-urteil-fuer-den-schutz-sensibler-daten-sorgen");
    c.has("Auch der Verweis auf ehrverletzende Bewertungen");
    c.lacks("Fanden Sie diesen Artikel nützlich?");
    c.lacks("Kommentar hinzufügen");
    c.lacks("Anja Schmoll-Trautmann");
    c.lacks("Aktuell");
    c.load("https://correctiv.org/aktuelles/neue-rechte/2019/05/14/wir-haben-bereits-die-zusage");
    c.lacks("Alle Artikel zu unseren Recherchen");
    c.has("Vorweg: Die beteiligten AfD-Politiker");
    c.has("ist heute Abend um 21 Uhr auch im ZDF-Magazin Frontal");
    c.lacks("Wir informieren Sie regelmäßig zum Thema Neue Rechte");
    c.lacks("Kommentar verfassen");
    c.lacks("weiterlesen");
    c.load("https://www.sueddeutsche.de/wirtschaft/bahn-flixbus-flixtrain-deutschlandtakt-fernverkehr-1.4445845");
    c.lacks("05:28 Uhr");
    c.has("Bahn-Konkurrenten wie Flixbus fürchten durch den geplanten Deutschlandtakt");
    c.lacks("ICE im S-Bahn-Takt");
    c.lacks("Diskussion zu diesem Artikel auf:");
    c.lacks("Berater-Affäre bringt Bahnchef Lutz in Bedrängnis");
    c.has("auch der Bus ein klimafreundliches Verkehrsmittel sei");
    c.load("https://www.adac.de/rund-ums-fahrzeug/tests/kindersicherheit/kindersitztest-2018/");
    c.lacks("Rund ums Fahrzeug");
    c.has("in punkto Sicherheit, Bedienung, Ergonomie");
    c.has("Grenzwert der Richtlinie 2014/79/EU");
    c.lacks("Diesel-Umtauschprämien");
    c.has("Besonders bei Babyschalen sollte geprüft werden");
    c.load("https://www.caktusgroup.com/blog/2015/06/08/testing-client-side-applications-django-post-mortem/");
    c.has("Was I losing my mind?");
    c.has("being cached after their first access.");
    c.has("Finding a Fix");
    c.has("from django.conf import settings");
    c.lacks("New Call-to-action");
    c.lacks("Contact us");
    c.lacks("Back to blog");
    c.lacks("You might also like:");
    c.load("https://www.computerbase.de/2007-06/htc-touch-bald-bei-o2-als-xda-nova/");
    c.has("Vor knapp zwei Wochen");
    c.has("gibt es in der dazugehörigen Vorstellungs-News.");
    c.lacks("Themen:");
    c.lacks("bis Januar 2009 Artikel für ComputerBase verfasst.");
    c.lacks("Warum Werbebanner?");
    c.lacks("71 Kommentare");
    c.load("http://www.chineselyrics4u.com/2011/07/zhi-neng-xiang-nian-ni-jam-hsiao-jing.html");
    c.has("就放心去吧");
    c.has("Repeat Chorus");
    c.lacks("Older post");
    c.lacks("Thank you for your support!");
    c.load("https://www.basicthinking.de/blog/2018/12/05/erfolgreiche-tweets-zutaten/");
    c.has("Frank Thelen, Investor");
    c.has("Female founders must constantly consider");
    c.has("Thema des öffentlichen Interesses");
    c.lacks("Nach langjähriger Tätigkeit im Ausland");
    c.has("Schaut man ganz genau hin, ist der Habeck-Kommentar");
    c.lacks("Mit Absendung des Formulars willige ich");
    c.lacks("Kommentieren");
    c.load("https://meedia.de/2016/03/08/einstieg-ins-tv-geschaeft-wie-freenet-privatkunden-fuer-antennen-tv-in-hd-qualitaet-gewinnen-will/");
    c.has("Welche Werbeeinnahmen erwarten Sie hier langfristig?");
    c.has("wir haben keinerlei Pläne, das zu verändern.");
    c.lacks("Nachrichtenüberblick abonnieren");
    c.lacks("über alle aktuellen Entwicklungen auf dem Laufenden.");
    c.lacks("Schlagworte");
    c.lacks("Teilen");
    c.lacks("Dauerzoff um drohenden UKW-Blackout");
    c.has("Mobilcom Debitel has charged me for third party");
    c.load("https://www.incurvy.de/trends-grosse-groessen/wellness-gesichtsbehandlung-plaisir-daromes/");
    c.has("Zeit für Loslassen und Entspannung.");
    c.has("Wie sieht dein Alltag aus?");
    c.has("Erfrischende, abschwellende Augencreme Phyto Contour");
    c.has("Vielen Dank Anja für deine Tipps rund um Beauty");
    c.lacks("Betreiberin von incurvy Plus Size");
    c.lacks("Wir verwenden Cookies");
    c.load("https://www.dw.com/en/uncork-the-mystery-of-germanys-fr%C3%BChburgunder/a-16863843");
    c.has("No grape variety invites as much intrigue");
    c.has("With just 0.9 hectares");
    c.lacks("Related Subjects");
    c.lacks("Audios and videos on the topic");
    c.load("https://www.jolie.de/stars/adele-10-kilo-abgenommen-sie-zeigt-sich-schlanker-denn-je-200226.html");
    c.has("Adele feierte ausgelassen mit den Spice Girls");
    c.has("wie sich Adele weiterentwickelt.");
    c.lacks("Sommerzeit ist Urlaubszeit,");
    c.lacks("Lade weitere Inhalte");
    c.load("https://www.speicherguide.de/digitalisierung/faktor-mensch/schwierige-gespraeche-so-gehts-24376.aspx");
    c.has("Konflikte mag keiner.");
    c.has("Gespräche meistern können.");
    c.lacks("Weiterführender Link");
    c.lacks("Flexible Wege in die");
    c.lacks("Storage für den Mittelstand");
    c.load("https://novalanalove.com/ear-candy/");
    c.has("Earcuff: Zoeca");
    c.has("mit längeren Ohrringen (:");
    c.has("Kreole: Stella Hoops");
    c.lacks("Jetzt heißt es schnell sein:");
    c.lacks("Diese Website speichert Cookies");
    c.lacks("VON Sina Giebel");
    c.load("http://www.franziska-elea.de/2019/02/10/das-louis-vuitton-missgeschick/");
    c.has("Zuerst dachte ich, ich könnte das");
    c.has("x Franzi");
    if !formatting {
        c.has("Flauschjacke: Bershka");
    } else {
        c.has("Flauschjacke: **Bershka**");
    }
    c.lacks("Palm Springs Mini (links)");
    c.lacks("Diese Website verwendet Akismet");
    c.lacks("New York, New York");
    c.load("https://www.gofeminin.de/abnehmen/wie-kann-ich-schnell-abnehmen-s1431651.html");
    c.has("Die Psyche spielt eine nicht unerhebliche Rolle");
    c.lacks("Sportskanone oder Sportmuffel");
    c.lacks("PINNEN");
    c.has("2. Satt essen bei den Mahlzeiten");
    c.lacks("Bringt die Kilos zum Purzeln!");
    c.lacks("Crash-Diäten ziehen meist den Jojo-Effekt");
    c.load("https://www.brigitte.de/liebe/persoenlichkeit/ikigai-macht-dich-sofort-gluecklicher--10972896.html");
    c.has("Glücks-Trend Konkurrenz");
    c.has("Praktiziere Dankbarkeit");
    c.has("dein Ikigai schon gefunden?");
    c.has("14,90 Euro.");
    c.lacks("Neu in Liebe");
    c.lacks("Erfahre mehr");
    c.lacks("Erfahrung mit privater Arbeitsvermittlung?");
    c.load("https://www.changelog.blog/zwischenbilanz-jan-kegelberg-ueber-tops-und-flops-bei-der-transformation-von-sportscheck/");
    c.has("Gibt es weitere Top-Maßnahmen für Multi-Channel?");
    c.has("Vielen Dank für das interessante Interview!");
    c.lacks("akzeptiere die Datenschutzbestimmungen");
    c.lacks("Diese Beiträge solltest du nicht verpassen");
    c.lacks("Annette Henkel");
    c.load("https://threatpost.com/android-ransomware-spreads-via-sex-simulation-game-links-on-reddit-sms/146774/");
    c.has("These messages include links to the ransomware");
    c.has("using novel techniques to exfiltrate data.");
    c.lacks("Share this article:");
    c.lacks("Write a comment");
    c.lacks("Notify me when new comments are added.");
    c.lacks("uses Akismet to reduce spam.");
    c.load("https://www.vice.com/en_uk/article/d3avvm/the-amazon-is-on-fire-and-the-smoke-can-be-seen-from-space");
    c.has("Brazil went dark.");
    c.has("the highest number of deforestation warnings.”");
    c.lacks("Tagged:");
    c.lacks("to the VICE newsletter.");
    c.lacks("Watch this next");
    c.load("https://www.heise.de/newsticker/meldung/Lithium-aus-dem-Schredder-4451133.html");
    c.has("Die Ökobilanz von Elektroautos");
    c.has("Nur die Folie bleibt zurück");
    c.lacks("Forum zum Thema:");
    c.load("https://www.theverge.com/2019/7/3/20680681/ios-13-beta-3-facetime-attention-correction-eye-contact");
    c.has("Normally, video calls tend to");
    c.has("across both the eyes and nose.");
    c.has("Added ARKit explanation and tweet.");
    c.lacks("Singapore’s public health program");
    c.lacks("Command Line delivers daily updates");
    c.load("https://crazy-julia.de/beauty-tipps-die-jede-braut-kennen-sollte/");
    c.has("in keinem Braut-Beauty-Programm fehlen darf?");
    c.has("nicht nur vor der Hochzeit ein absolutes Muss.");
    c.has("Gesundes, glänzendes Haar");
    c.lacks("Neue Wandbilder von Posterlounge");
    c.lacks("mit meinen Texten und mit meinen Gedanken.");
    c.lacks("Erforderliche Felder sind mit * markiert.");
    c.load(
        "https://www.politische-bildung-brandenburg.de/themen/land-und-leute/homo-brandenburgensis",
    );
    c.has("Stilles Rackern, statt lautem Deklamieren.");
    c.has("Watt jibt’s n hier zu lachen?");
    c.has("Das Brandenbuch. Ein Land in Stichworten.");
    c.lacks("Bürgerbeteiligung");
    c.lacks("Anmelden");
    c.lacks("Foto: Timur");
    c.lacks("Schlagworte");
    c.lacks("Zeilenumbrüche und Absätze werden automatisch erzeugt.");
    c.load("https://skateboardmsm.de/news/the-captains-quest-2017-contest-auf-schwimmender-miniramp-am-19-august-in-dormagen.html");
    c.has("Wakebeach 257");
    c.has("Be there or be square!");
    c.has("Hier geht’s zur Facebook Veranstaltung");
    c.lacks("More from News");
    c.lacks("von Redaktion MSM");
    c.lacks("add yours.");
    c.load("https://knowtechie.com/rocket-pass-4-in-rocket-league-brings-with-it-a-new-rally-inspired-car/");
    c.has("Rocket Pass 4 will begin at 10:00 a.m. PDT");
    c.has("Holy shit, Mortal Kombat 11");
    c.has("Let us know down below in the comments");
    c.lacks("Related Topics");
    c.lacks("You can keep up with me on Twitter");
    c.lacks("Hit the track today with Mario Kart Tour");
    c.load("https://en.wikipedia.org/wiki/T-distributed_stochastic_neighbor_embedding");
    c.has("Given a set of high-dimensional objects");
    c.has("Herein a heavy-tailed Student t-distribution");
    c.lacks("Categories:");
    c.lacks("Conditional random field");
    c.load("https://mixed.de/vrodo-deals-vr-taugliches-notebook-fuer-83215-euro-99-cent-leihfilme-bei-amazon-psvr/");
    c.has("Niedlicher Roboter-Spielkamerad: Anki Cozmo");
    c.has("Empfehlungen von Dennis:");
    c.lacks("Unterstütze unsere Arbeit");
    c.lacks("Deepfake-Hollywood");
    c.lacks("Avengers");
    c.lacks("Katzenschreck");
    c.load(
        "http://www.spreeblick.com/blog/2006/07/29/aus-aus-alles-vorbei-habeck-macht-die-stahnke/",
    );
    c.has("Hunderttausende von jungen Paaren");
    c.has("wie flatterhaft das Mädl ist? :)");
    c.lacks("Malte Welding");
    c.lacks("YouTube und die Alten");
    c.lacks("Autokorrektur");
    c.load("https://majkaswelt.com/top-5-fashion-must-haves-2018-werbung/");
    c.has("Rüschen und Volants.");
    c.has("ihr jedes Jahr tragen könnt?");
    c.lacks("Das könnte dich auch interessieren");
    c.lacks("Catherine Classic Lac 602");
    c.load("https://erp-news.info/erp-interview-mit-um-digitale-assistenten-und-kuenstliche-intelligenz-ki/");
    if !formatting {
        c.has("Einblicke in die Vision zukünftiger Softwaregenerationen.");
    } else {
        c.has("Einblicke in die **Vision zukünftiger Softwaregenerationen**.\n");
    }
    c.has("Frage 4: Welche Rolle spielt Big Data in Bezug auf Assistenz-Systeme und KI?");
    if !formatting {
        c.has("von The unbelievable Machine Company (*um) zur Verfügung gestellt.");
    } else {
        c.has("von **The unbelievable Machine Company (\\*um)** zur Verfügung gestellt.\n");
    }
    c.lacks("Matthias Weber ist ERP-Experte mit langjähriger Berufserfahrung.");
    c.lacks("Die Top 5 digitalen Trends für den Mittelstand");
    c.lacks(", leading edge,");
    c.load("https://boingboing.net/2013/07/19/hating-millennials-the-preju.html");
    c.has("Click through for the whole thing.");
    c.has("The generation we love to dump on");
    c.lacks("GET THE BOING BOING NEWSLETTER");
    c.load("https://github.blog/2019-03-29-leader-spotlight-erin-spiceland/");
    c.has("Erin Spiceland is a Software Engineer for SpaceX.");
    c.has("make effective plans and goals for the future");
    c.has("looking forward to next?");
    c.has("Research Consultant at Adelard LLP");
    c.lacks("Related posts");
    c.lacks("Jeremy Epling");
    c.lacks("Missed the main event?");
    c.lacks("Privacy");
    c.load("https://lady50plus.de/2019/06/19/sekre-mystery-bag/");
    c.has("ist eine echte Luxushandtasche");
    c.has("Insgesamt 160 weibliche „Designerinnen“");
    c.has("Sei herzlich gegrüßt");
    c.has("Ein Mann alleine hätte niemals");
    c.lacks("Erforderliche Felder sind mit");
    c.lacks("Benachrichtige mich");
    c.lacks("Reisen ist meine große Leidenschaft");
    c.lacks("Styling Tipps für Oktober");
    c.has("in den Bann ziehen!");
    c.load("https://www.sonntag-sachsen.de/emanuel-scobel-wird-thomanerchor-geschaeftsfuehrer");
    c.has("Neuer Geschäftsführender Leiter");
    c.has("nach Leipzig wechseln.");
    c.lacks("Mehr zum Thema");
    c.lacks("Folgen Sie uns auf Facebook und Twitter");
    c.lacks("Aktuelle Ausgabe");
    c.load(
        "https://www.psl.eu/actualites/luniversite-psl-quand-les-grandes-ecoles-font-universite",
    );
    c.has("Le décret n°2019-1130 validant");
    c.has("restructurant à cet effet ».");
    c.lacks(" utilise des cookies pour");
    c.lacks("En savoir plus");
    c.load("https://www.chip.de/test/Beef-Maker-von-Aldi-im-Test_154632771.html");
    c.has("Starke Hitze nur in der Mitte");
    c.has("ca. 35,7×29,4 cm");
    c.has("Wir sind im Steak-Himmel!");
    c.lacks("Samsung Galaxy S10 128GB");
    c.lacks("Für Links auf dieser Seite");
    c.load("http://www.sauvonsluniversite.fr/spip.php?article8532");
    c.has("L’AG Éducation Île-de-France inter-degrés");
    c.has("Grève et mobilisation pour le climat");
    c.has("suivi.reformes.blanquer@gmail.com");
    c.lacks("Sauvons l’Université !");
    c.lacks("La semaine de SLU");
    c.load("https://www.spiegel.de/spiegel/print/d-161500790.html");
    // D-1: the slow default (fast=False) keeps the two sentences and drops the footer; fast mode
    // drops the sentences and keeps the footer
    // oracle: extract(page, url, fast=True) contains "Wie konnte es dazu kommen?" -> False,
    // "Die Geschichte beginnt am 26. Oktober" -> False, "Die digitale Welt der Nachrichten." -> True,
    // "Vervielfältigung nur mit Genehmigung" -> True
    c.lacks("Wie konnte es dazu kommen?");
    c.lacks("Die Geschichte beginnt am 26. Oktober");
    c.has("Es stützt seine Version.");
    c.lacks("und Vorteile sichern!");
    c.lacks("Verschickt");
    c.has("Die digitale Welt der Nachrichten.");
    c.has("Vervielfältigung nur mit Genehmigung");
    c.load("https://lemire.me/blog/2019/08/02/json-parsing-simdjson-vs-json-for-modern-c/");
    c.has("I use a Skylake processor with GNU GCC 8.3.");
    c.has("gsoc-2018");
    c.has("0.091 GB/s");
    c.has("version 0.2 on vcpkg.");
    c.lacks("Leave a Reply");
    c.lacks("Science and Technology links");
    c.lacks("Proudly powered by WordPress");
    c.load("https://www.zeit.de/mobilitaet/2020-01/zugverkehr-christian-lindner-hochgeschwindigkeitsstrecke-eu-kommission");
    c.has("36 Stunden.");
    c.has("Nationale Egoismen");
    c.has("Deutschland kaum beschleunigt.");
    c.lacks("Durchgehende Tickets fehlen");
    c.has("geprägte Fehlentscheidung.");
    c.has("horrende Preise für miserablen Service bezahlen?");
    c.lacks("Bitte melden Sie sich an, um zu kommentieren.");
    c.load("https://www.franceculture.fr/emissions/le-journal-des-idees/le-journal-des-idees-emission-du-mardi-14-janvier-2020");
    c.has("Performativité");
    c.has("Les individus productifs communiquent");
    c.has("de nos espoirs et de nos désirs.");
    c.lacks("A la tribune je monterai");
    c.lacks("À découvrir");
    c.lacks("Le fil culture");
    c.load("https://wikimediafoundation.org/news/2020/01/15/access-to-wikipedia-restored-in-turkey-after-more-than-two-and-a-half-years/");
    c.has("as further access is restored.");
    c.lacks("Read further in the pursuit of knowledge");
    c.lacks("Here’s what that means.");
    c.lacks("Stay up-to-date on our work.");
    c.lacks("Photo credits");
    c.load("https://www.reuters.com/article/us-awards-sag/parasite-scores-upset-at-sag-awards-boosting-oscar-chances-idUSKBN1ZI0EH");
    c.lacks("4 Min Read");
    c.lacks("Factbox: Key winners");
    c.has("Despite an unknown cast,");
    c.has("Additional reporting by");
    c.load("https://vancouversun.com/technology/microsoft-moves-to-erase-its-carbon-footprint-from-the-atmosphere-in-climate-push/wcm/76e426d9-56de-40ad-9504-18d5101013d2");
    c.has("Microsoft Corp said on Thursday");
    c.has("Postmedia is committed");
    c.lacks("I consent to receiving");
    c.has("It was not immediately clear if");
    c.lacks("turns CO2 into soap");
    c.lacks("Reuters files");
}
