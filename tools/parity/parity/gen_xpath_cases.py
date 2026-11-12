"""Generates src/xpaths/cases.rs: upstream's XPath selectors run on HTML snippets, for the Rust tests.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_xpath_cases import main; main()' > src/xpaths/cases.rs

Each snippet is parsed with `lxml.html.fromstring(html, parser=HTML_PARSER)`; every `XPath` of the content, comment,
discard and metadata lists and `BASIC_CLEAN_XPATH` runs on the context element, and the matches are written as
positions in `root.iter()` order. A snippet carries the `treedump.dump_tree` of its tree, so the Rust
test can tell a parser difference from a selector difference.

The token tables hold one value per upstream token. A value must match its token (checked here), and an
upstream sync that adds or renames a token fails the coverage check until its value is added.
"""

import html
import re
import sys
from dataclasses import dataclass, field


@dataclass
class Snippet:
    html: str
    hit: tuple[str, ...] = field(default=())  # selectors that must match something
    miss: tuple[str, ...] = field(default=())  # selectors that must match nothing


def page(body: str) -> str:
    return f"<html><body>{body}</body></html>"


def element(tag: str, attr: str, value: str, text: str = "x") -> str:
    return f'<{tag} {attr}="{html.escape(value, quote=True)}">{text}</{tag}>'


# Tokens whose every sensible value also matches another token of the same attribute in upstream's own
# lists, so no case can show the token alone.
SHADOWED_BY_UPSTREAM = {"sidebar", "navbar", "embedded"}


def tokens_snippets(selector: str, attr: str, tokens, values: dict, tag: str = "div") -> list[Snippet]:
    """One snippet per token: `values[token]` (a string or a tuple of them) in `attr`, every one matching its token."""
    assert set(values) == set(tokens), f"{selector} {attr}: token table out of date: {set(values) ^ set(tokens)}"
    out = []
    for token, found in values.items():
        found = (found,) if isinstance(found, str) else found
        # a selector that drops the token must fail a case: some value matches this token only
        alone = [v for v in found if not any(re.search(other, v) for other in tokens if other != token)]
        assert alone or token in SHADOWED_BY_UPSTREAM, f"{selector} {attr}: no value matches only {token!r}"
        for value in found:
            assert re.search(token, value), f"{selector} {attr}: {value!r} does not match {token!r}"
            out.append(Snippet(page(element(tag, attr, value)), hit=(selector,)))
    return out


def plain(selector: str, attr: str, values, tag: str = "div", hit: bool = True) -> list[Snippet]:
    "Snippets for the values of an exact or substring test; `hit` says whether the selector must match."
    return [Snippet(page(element(tag, attr, value)), **{"hit" if hit else "miss": (selector,)}) for value in values]


LINK = '<a href="/c">c</a>'


def under(selector: str, tag: str, attr: str, values, hit: bool = True) -> list[Snippet]:
    "Snippets with a link inside an element `tag` carrying `attr`=value, for the `//holder//a[@href]` selectors."
    return [Snippet(page(element(tag, attr, value, LINK)), **{"hit" if hit else "miss": (selector,)}) for value in values]


def by_tag(selector: str, attr: str, value: str, hits, misses, text: str = "x") -> list[Snippet]:
    "One snippet per tag: `selector` must match an element of a tag in `hits` and none of a tag in `misses`."
    return ([Snippet(page(element(t, attr, value, text)), hit=(selector,)) for t in hits]
            + [Snippet(page(element(t, attr, value, text)), miss=(selector,)) for t in misses])


def snippets() -> list[Snippet]:
    from trafilatura import xpaths as x

    out: list[Snippet] = []
    b = lambda i: f"BODY_XPATH[{i}]"  # noqa: E731
    o = lambda i: f"OVERALL_DISCARD_XPATH[{i}]"  # noqa: E731
    c = lambda i: f"COMMENTS_XPATH[{i}]"  # noqa: E731
    cd = lambda i: f"COMMENTS_DISCARD_XPATH[{i}]"  # noqa: E731
    rc, rcl = "REMOVE_COMMENTS_XPATH[0]", "REMOVE_COMMENTS_AND_LISTS_XPATH[0]"
    prune = lambda i: f"RAW_TREE_PRUNE_XPATH[{i}]"  # noqa: E731
    pd = lambda i: f"PRECISION_DISCARD_XPATH[{i}]"  # noqa: E731
    clean = "BASIC_CLEAN_XPATH"

    # ---- BODY_XPATH
    out += tokens_snippets(b(0), "id", x._ARTICLE_CONTENT_ID_TOKENS, {
        "(?:entry|article|art)-content": ("entry-content", "art-content"),
        "article__content": "article__content",
        "article(?:-|__)?body": ("article-body", "article__body", "articlebody"),
        "articleBody": "articleBody",
        "body-text": "body-text",
    })
    out += tokens_snippets(b(0), "class", x._ARTICLE_CONTENT_CLASS_TOKENS, {
        "post[-_]text": ("post-text", "post_text"),
        "post-body": "post-body",
        "post-?entry": ("postentry", "post-entry"),
        "post[-_]?content": ("postcontent", "post_content"),
        "postContent": "postContent",
        "post_inner_wrapper": "post_inner_wrapper",
        "article-?text": ("article-text", "articletext"),
        "articleText": "articleText",
        "(?:entry|page|text|article|art)-content": ("page-content", "text-content"),
        "article__content": "article__content",
        "article(?:-|__)?body": "article-body",
        "articleBody": "articleBody",
        "ArticleContent": "ArticleContent",
        "body-text": "body-text",
        "article__container": "article__container",
    })
    out += plain(b(0), "class", ["post", "entry"])
    out += plain(b(0), "class", ["post entry", "Post", "posts"], hit=False)
    out += plain(b(0), "itemprop", ["articleBody"])
    out += plain(b(0), "itemprop", ["articlebody", "articleBody2"], hit=False)
    out += plain(b(0), "id", ["articleContent"])
    out += plain(b(0), "id", ["articlecontent", "articleContent2"], hit=False)
    out += plain(b(0), "class", ["xpost-body-y", "post-bodyx", "article-body"], hit=True)
    out += plain(b(0), "class", ["Post-Body"], hit=False)
    out += plain(b(0), "class", ["post body", "entrycontent", "Entry-Content"], hit=False)
    # tag filter: article, div, main, section only
    out += [Snippet(page(element(t, "class", "post-body")), hit=(b(0),)) for t in ("article", "main", "section")]
    out += [Snippet(page(element(t, "class", "post-body")), miss=(b(0),)) for t in ("span", "p", "aside", "ul")]
    # [1] is per parent: the outer div and the nested one match, the second sibling does not
    out.append(Snippet(page('<div class="post-body"><div class="post-body">a</div></div><div class="post-body">b</div>'), hit=(b(0),)))
    out.append(Snippet(page('<section><div class="post-body">a</div><div class="post-body">b</div></section><section><div class="post-body">c</div></section>'), hit=(b(0),)))

    # BODY_XPATH[1]: (.//article)[1]
    out.append(Snippet(page("<article id='a'><article id='b'>x</article></article><article id='c'>y</article>"), hit=(b(1),)))
    out.append(Snippet(page("<div><article>x</article></div><article>y</article>"), hit=(b(1),)))
    out.append(Snippet(page("<div>no article</div><articles>x</articles>"), miss=(b(1),)))

    out += tokens_snippets(b(2), "id", x._STORY_ID_TOKENS, {"^primary": "primary-nav", "story-body": "my-story-body"})
    out += tokens_snippets(b(2), "class", x._STORY_CLASS_TOKENS, {
        "^article ": "article lead",
        "post-bodycopy": "post-bodycopy",
        "story-?content": ("storycontent", "story-content"),
        "(?:theme|blog|section|single)-content": ("theme-content", "blog-content", "section-content", "single-content"),
        "single-post": "single-post",
        "main-column": "main-column",
        "wpb_text_column": "wpb_text_column",
        "story-body": "story-body",
        "field-body": "field-body",
    })
    out += plain(b(2), "id", ["x-primary", "Primary", "story-bod"], hit=False)
    out += plain(b(2), "class", ["article", "x article y", "myarticle lead"], hit=False)
    out += plain(b(2), "role", ["article"])
    out += plain(b(2), "role", ["Article"], hit=False)
    out += plain(b(2), "id", ["article", "story"])
    out += plain(b(2), "id", ["Article", "story2"], hit=False)
    out += plain(b(2), "class", ["postarea", "art-postcontent", "text", "cell", "story"])
    out += plain(b(2), "class", ["Text", "text cell", "stories"], hit=False)
    out += plain(b(2), "class", ["fulltext", "FullText", "my-FULLTEXT-x"])
    out += plain(b(2), "class", ["full text", "fulltex"], hit=False)
    out += [Snippet(page(element(t, "class", "story-body")), miss=(b(2),)) for t in ("span", "p")]
    out.append(Snippet(page('<div class="story-body"><div class="story-body">a</div></div><div class="story-body">b</div>'), hit=(b(2),)))

    out += tokens_snippets(b(3), "id", x._MAIN_CONTENT_ID_TOKENS, {
        "content-main": "content-main", "content-body": "content-body", "contentBody": "contentBody",
    })
    out += tokens_snippets(b(3), "class", x._MAIN_CONTENT_CLASS_TOKENS, {
        "content[-_]main": ("content-main", "content_main"),
        "content(?:-|__)body": ("content-body", "content__body"),
    })
    out += plain(b(3), "id", ["content"])
    out += plain(b(3), "class", ["content"])
    out += plain(b(3), "id", ["Content", "content2"], hit=False)
    out += plain(b(3), "class", ["Content", "content wide"], hit=False)
    # translate maps C and M only: an all-caps value stays all-caps in the unmapped letters
    out += plain(b(3), "id", ["main-content", "Main-Content", "x-Main-content"], hit=True)
    out += plain(b(3), "id", ["MAIN-CONTENT", "Main_Content", "x-MAIN-content", "mAIN-CONTENT"], hit=False)
    out += plain(b(3), "class", ["main-content", "Main-Content", "xmain-contenty"])
    out += plain(b(3), "class", ["MAIN-CONTENT", "mainContent"], hit=False)
    out += plain(b(3), "class", ["page-content", "Page-Content", "my-Page-content"])
    out += plain(b(3), "class", ["PAGE-CONTENT", "pagecontent", "my-pAGE-content"], hit=False)
    out += plain(b(3), "id", ["page-content"], hit=False)
    out += [Snippet(page(element(t, "class", "content-body")), miss=(b(3),)) for t in ("span", "p")]
    out.append(Snippet(page('<div id="content"><div class="content">a</div></div><div id="content">b</div>'), hit=(b(3),)))

    out += plain(b(4), "class", ["main", "main-wrap", "mainx"])
    out += plain(b(4), "id", ["main", "main-wrap"])
    out += plain(b(4), "role", ["main", "mainx"])
    out += plain(b(4), "class", ["Main", "x main", "domain"], hit=False)
    out += plain(b(4), "id", ["Main", "x-main"], hit=False)
    out += plain(b(4), "role", ["Main"], hit=False)
    out += [Snippet(page(element(t, "class", "main")), hit=(b(4),)) for t in ("article", "section")]
    out += [Snippet(page(element(t, "class", "main")), miss=(b(4),)) for t in ("span", "p")]
    out.append(Snippet(page("<div><main>a</main></div><main>b</main>"), hit=(b(4),)))
    out.append(Snippet(page("<div class='main'>a</div><main>b</main>"), hit=(b(4),)))
    out.append(Snippet(page("<main><main>a</main></main>"), hit=(b(4),)))
    out.append(Snippet(page("<mains>a</mains>"), miss=(b(4),)))

    # ---- COMMENTS_XPATH. @id|@class reads the attribute that comes first in the source
    for tag in ("div", "section", "list"):
        out.append(Snippet(page(f'<{tag} id="comment-list">x</{tag}>'), hit=(c(0),)))
    out += plain(c(0), "id", ["commentlist", "comment-list", "my-comment-list-x"], hit=True)
    out += plain(c(0), "class", ["commentlist", "comment-list"], hit=True)
    out += plain(c(0), "id", ["comment--list", "Commentlist", "comment list"], hit=False)
    out += plain(c(0), "class", ["comment-page", "comments-content", "post-comments", "my-post-comments-x"], hit=True)
    out += plain(c(0), "id", ["comment-page", "comments-content", "post-comments"], hit=False)
    out += plain(c(0), "class", ["comment page", "Post-comments", "post_comments"], hit=False)
    out += [
        Snippet(page('<div id="comment-list" class="x">a</div>'), hit=(c(0),)),
        Snippet(page('<div class="x" id="comment-list">a</div>'), miss=(c(0),)),
        Snippet(page('<div class="comment-list" id="x">a</div>'), hit=(c(0),)),
        Snippet(page('<div id="x" class="comment-list">a</div>'), miss=(c(0),)),
        Snippet(page('<span id="comment-list">a</span><p class="comment-page">b</p><ul class="comment-page">c</ul>'), miss=(c(0),)),
    ]
    out += plain(c(1), "id", ["comments", "comment-x", "comment-"], hit=True)
    out += plain(c(1), "class", ["comments", "comment-x", "comment-"], hit=True)
    out += plain(c(1), "id", ["comment", "commentary", "x-comments", "Comments", "comment x", "comment_x"], hit=False)
    out += plain(c(1), "class", ["comment", "commentary", "x comments"], hit=False)
    out += plain(c(1), "class", ["Comments", "Comments-x", "article-comments", "my-article-comments-x"], hit=True)
    out += plain(c(1), "id", ["Comments", "article-comments"], hit=False)
    out += plain(c(1), "class", ["article_comments", "article-comment", "xComments"], hit=False)
    out += [
        Snippet(page('<div id="comments" class="x">a</div>'), hit=(c(1),)),
        Snippet(page('<div class="x" id="comments">a</div>'), miss=(c(1),)),
        Snippet(page('<div class="comments" id="x">a</div>'), hit=(c(1),)),
        Snippet(page('<div id="x" class="comments">a</div>'), miss=(c(1),)),
        Snippet(page('<span id="comments">a</span><p class="comments">b</p>'), miss=(c(1),)),
        Snippet(page('<section class="comments">a</section><list id="comments">b</list>'), hit=(c(1),)),
    ]
    out += plain(c(2), "id", ["comol", "disqus_thread", "dsq-comments", "comol-x", "dsq-comments-y"], hit=True)
    out += plain(c(2), "id", ["x-comol", "Comol", "disqus-thread", "dsq_comments"], hit=False)
    out += plain(c(2), "class", ["comol", "disqus_thread"], hit=False)
    out += plain(c(2), "id", ["comol"], tag="section")
    out += plain(c(2), "id", ["comol"], tag="list")
    out += plain(c(2), "id", ["comol"], tag="span", hit=False)
    out += plain(c(3), "id", ["social", "social-share", "socialx"], hit=True)
    out += plain(c(3), "id", ["Social", "my-social", "soc"], hit=False)
    out += plain(c(3), "class", ["comment", "x-comment-y", "comments", "commentary"], hit=True)
    out += plain(c(3), "class", ["Comment", "comnt"], hit=False)
    out += plain(c(3), "class", ["comment"], tag="section")
    out += plain(c(3), "class", ["comment"], tag="list", hit=False)
    out += plain(c(3), "class", ["comment"], tag="span", hit=False)
    out += plain(c(3), "id", ["social"], tag="p", hit=False)

    # ---- REMOVE_COMMENTS_XPATH and the list variant (lookahead, anchors, ol/ul)
    # (?!ary) rejects "commentary" only; "commentar" and "commenta\n" are not "commentary"
    out += plain(rc, "id", ["comment", "Comment", "comments", "comment-1", "commentX", "commentar", "commenta", "commenta\n",
                            "commentarz", "comment\n", "comments\n", "comol", "comol-x", "disqus_thread", "dsq-comments"], hit=True)
    out += plain(rc, "id", ["commentary", "commentary\n", "commentaryx", "xcomment", "Xcomment", "COMMENT", "disqus-thread",
                            "x-disqus_thread", "Comol", "xcomol"], hit=False)
    out += plain(rc, "class", ["comment", "Comment-list", "commentar", "commenta\n", "comment\n", "article-comments", "post-comments", "x-post-comments-y"], hit=True)
    out += plain(rc, "class", ["commentary", "commentary\n", "my-comment", "COMMENT", "article_comments", "comol", "disqus_thread"], hit=False)
    out += [
        Snippet(page(f'<{t} id="comments">x</{t}>'), hit=(rc, rcl)) for t in ("div", "list", "section", "details")
    ] + [
        Snippet(page(f'<{t} id="comments">x</{t}>'), hit=(rcl,), miss=(rc,)) for t in ("ol", "ul")
    ] + [
        Snippet(page(f'<{t} id="comments">x</{t}>'), miss=(rc, rcl)) for t in ("span", "p", "article")
    ]
    out += [
        Snippet(page('<ol id="comment-list"><li>a</li></ol><ul class="comments"><li>b</li></ul>'), hit=(rcl,), miss=(rc,)),
        Snippet(page('<ol class="post-comments"><li>a</li></ol><ul class="Comments"><li>b</li></ul>'), hit=(rcl,)),
        # disqus ids and the ol/ul filter: a list needs a comment id or class
        Snippet(page('<ol id="disqus_thread">a</ol><ul id="dsq-comments">b</ul>'), miss=(rc, rcl)),
        Snippet(page('<div id="disqus_thread">a</div><ol id="comol">b</ol>'), hit=(rc, rcl)),
        Snippet(page('<ul id="commentary">a</ul><ol class="commentary">b</ol>'), miss=(rc, rcl)),
        Snippet(page('<ol id="disqus_thread" class="comments">a</ol>'), hit=(rcl,)),
        Snippet(page('<div id="comments"><ol class="comments"><li>a</li></ol></div>'), hit=(rc, rcl)),
    ]

    # ---- RAW_TREE_PRUNE_XPATH
    p0 = prune(0)
    for attr in ("id", "class"):
        out += plain(p0, attr, ["mvp-post-add-box", "mvp-post-add-wrap", "infinitescroll", "infinite-scroll", "x-infinite-scroll-y"], hit=True)
        out += plain(p0, attr, ["mvp-post-add-other", "mvp-post-add", "infinite--scroll", "Infinitescroll", "infinite scroll"], hit=False)
    out += [Snippet(page(element(t, "class", "infinitescroll")), hit=(p0,)) for t in ("section", "aside")]
    out += [Snippet(page(element(t, "class", "infinitescroll")), miss=(p0,)) for t in ("span", "p", "article")]
    # a container holding the h1 is the real page, unless it is a mvp-post-add box
    out += [
        Snippet(page('<div class="infinite-scroll"><h1>t</h1></div>'), miss=(p0,)),
        Snippet(page('<div class="infinite-scroll"><section><h1>t</h1></section></div>'), miss=(p0,)),
        Snippet(page('<div class="infinite-scroll"><p>t</p></div><h1>t</h1>'), hit=(p0,)),
        Snippet(page('<div class="mvp-post-add-box"><h1>t</h1></div>'), hit=(p0,)),
        Snippet(page('<div id="mvp-post-add-wrap"><h1>t</h1></div>'), hit=(p0,)),
        Snippet(page('<div id="infinitescroll"><h1>t</h1></div>'), miss=(p0,)),
        Snippet(page('<div class="infinitescroll mvp-post-add-box"><h1>t</h1></div>'), hit=(p0,)),
        Snippet(page('<div class="x" id="infinitescroll"><h1>t</h1></div>'), miss=(p0,)),
        Snippet(page('<div id="infinitescroll"><div class="mvp-post-add-box"><h1>t</h1></div></div>'), hit=(p0,)),
    ]
    p1 = prune(1)
    out += plain(p1, "class", ["elementor-share-buttons", "x elementor-share-buttons y", "elementor-share-buttons-2"], hit=True)
    out += plain(p1, "class", ["Elementor-share-buttons", "elementor-share", "elementor-widget"], hit=False)
    out += plain(p1, "id", ["elementor-share-buttons"], hit=False)
    out += [Snippet(page(element(t, "class", "elementor-share-buttons")), hit=(p1,)) for t in ("section", "aside")]
    out += [Snippet(page(element(t, "class", "elementor-share-buttons")), miss=(p1,)) for t in ("span", "p")]

    # ---- OVERALL_DISCARD_XPATH[0]
    d0 = o(0)
    consent = {"cookie": ("cookie", "my-cookie-x")}
    out += tokens_snippets(d0, "id", x._CONSENT_ID_CLASS_TOKENS, consent)
    out += [
        Snippet(page('<div id="cookie" class="x">a</div>'), hit=(d0,)),
        Snippet(page('<div class="x" id="cookie">a</div>'), miss=(d0,)),
        Snippet(page('<div class="cookie" id="x">a</div>'), hit=(d0,)),
        Snippet(page('<div id="x" class="cookie">a</div>'), miss=(d0,)),
        Snippet(page('<div class="Cookie">a</div>'), miss=(d0,)),
        Snippet(page('<div data-lp-replacement-content>a</div>'), hit=(d0,)),
        Snippet(page('<div data-lp-replacement-content="">a</div>'), hit=(d0,)),
        Snippet(page('<div data-lp-replacement-content="x">a</div>'), hit=(d0,)),
        Snippet(page('<div data-lp-replacement>a</div><div data-x="data-lp-replacement-content">b</div>'), miss=(d0,)),
        Snippet(page('<div data-component="MostPopularStories">a</div>'), hit=(d0,)),
        Snippet(page('<div data-component="x-MostPopularStories-y">a</div>'), hit=(d0,)),
        Snippet(page('<div data-component="mostpopularstories">a</div>'), miss=(d0,)),
    ]
    # role: translate maps N to n only, so an all-caps role does not contain 'nav'
    out += plain(d0, "role", ["navigation", "Navigation", "nav", "Nav", "x-Nav-y", "subnav"], hit=True)
    out += plain(d0, "role", ["NAV", "NAVIGATION", "na", "main"], hit=False)
    both = {
        "^shar": ("shared", "sharx"),
        "social": "social-links",
        "viral": "viral-box",
        "newsletter": "newsletter-box",
        "syndication": "syndication-x",
        "tags": "post-tags",
        "sidebar": "sidebar-left",
        "banner": "top-banner",
        "bread-?crumb": ("breadcrumb", "bread-crumb"),
        "button": "button-row",
        "author": "author-box",
    }
    id_only = {
        "^(?:jp-|dpsp-content)": ("jp-post", "dpsp-content-x"),
        "bmdh": "bmdh1",
        "footer": "site-footer",
        "Footer": "SiteFooter",
        "share": "reshare",
        "Share": "ReShare",
        "nav": "main-nav",
        "Nav": "MainNav",
        "menu": "menu-x",
        "related": "related-posts",
        "message-container": "message-container",
        "premium": "premium-box",
        "^ad-": "ad-top",
    }
    class_only = {
        "^(?:nav|post-nav|ZendeskForm)": ("nav-top", "post-nav-x", "ZendeskForm"),
        "subnav": "subnav",
        "avigation": "Navigation",
        "navbar": "navbar",
        "navbox": "xnavbox",
        "menu": "menu-x",
        "bar": "top-bar",
        " ad ": "a ad b",
        "-ad-": "top-ad-slot",
        "(?:^| )ad-": ("ad-slot", "a ad-slot"),
        "dfp[Aa]d": ("dfpAd", "dfpad"),
        "outbrain": "outbrain",
        "taboola": "taboola",
        "criteo": "criteo",
        "paid-?content": ("paidcontent", "paid-content"),
        "widget": "widget",
        "footer": "footer",
        "Footer": "Footer",
        "byline": "byline",
        "Byline": "Byline",
        "share-": "x-share-y",
        "sociable": "sociable",
        "embedded": "embedded",
        "embed": "embed",
        "tag-list": "tag-list",
        "consent": "consent",
        "modal-content": "modal-content",
        "permission": "permission",
        "elated": "related-posts",
        "next-": "next-post",
        "-stories": "top-stories",
        "most-popular": "most-popular",
        "more-on": "more-on-x",
        "meta": "post-meta",
        "rating": "rating",
        "attachment": "attachment",
        "timestamp": "timestamp",
        "user-info": "user-info",
        "user-profile": "user-profile",
        "-icon": "fa-icon",
        "article-infos": "article-infos",
        "message-container": "message-container",
        "slide": "slide",
        "viewport": "viewport",
        "overlay": "overlay",
        "options": "options",
        "expand": "expand",
        "obfuscated": "obfuscated",
        "blurred": "blurred",
        "mol-factbox": "mol-factbox",
        "yin": "yin",
        "zlylin": "zlylin",
        "nfoline": "nfoline",
    }
    out += tokens_snippets(d0, "id", x._OVERALL_DISCARD_BOTH_TOKENS + x._OVERALL_DISCARD_ID_TOKENS, {**both, **id_only})
    out += tokens_snippets(d0, "class", x._OVERALL_DISCARD_BOTH_TOKENS + x._OVERALL_DISCARD_CLASS_TOKENS, {**both, **class_only})
    # the id and class lists differ: a class-only token in an id (and the reverse) does not match
    out += plain(d0, "id", ["widget", "taboola", "top-bar", "byline", "sociable-x"], hit=False)
    out += plain(d0, "class", ["bmdh1", "premium-box", "jp-post", "site-xyz", "xad-top", "my-ad", "ad"], hit=False)
    out += plain(d0, "class", ["Social", "SHARE", "Shared", "xshare", "Cookie", "ad", "ad ", " ad", "lad", "adx", "headline", "x-ad", "plain"], hit=False)
    out += plain(d0, "id", ["Social", "xad-", "ad", "plain"], hit=False)
    out += [
        # anchors: ^shar needs the start, (?:^| )ad- a start or a space
        Snippet(page('<div class="x shared">a</div>'), miss=(d0,)),
        Snippet(page('<div class="xad-y">a</div>'), miss=(d0,)),
        Snippet(page('<div id="x-ad-y">a</div>'), miss=(d0,)),
        # the token in the first and in the second attribute
        Snippet(page('<div id="x" class="social">a</div>'), hit=(d0,)),
        Snippet(page('<div class="x" id="social">a</div>'), hit=(d0,)),
        Snippet(page('<div id="social" class="x">a</div>'), hit=(d0,)),
        Snippet(page('<div class="social" id="x">a</div>'), hit=(d0,)),
    ]
    for t in ("item", "list", "p", "section", "span"):
        out.append(Snippet(page(element(t, "class", "social")), hit=(d0,)))
    for t in ("a", "article", "li", "ul", "aside", "header", "h2"):
        out.append(Snippet(page(element(t, "class", "social")), miss=(d0,)))

    # ---- OVERALL_DISCARD_XPATH[1]: every element
    d1 = o(1)
    out += plain(d1, "class", ["comments-title"])
    out += plain(d1, "class", ["reply-x", "reply-"], hit=True)
    out += plain(d1, "id", ["reply-x", "reply-"], hit=True)
    out += plain(d1, "id", ["Reply-x", "xreply-", "reply"], hit=False)
    out += [
        Snippet(page('<div class="reply-x" id="y">a</div>'), hit=(d1,)),
        Snippet(page('<div id="y" class="reply-x">a</div>'), miss=(d1,)),
        Snippet(page('<div id="reply-x" class="y">a</div>'), hit=(d1,)),
        Snippet(page('<div class="y" id="reply-x">a</div>'), miss=(d1,)),
        # @id|@style reads the id when there is no style, and the first of the two otherwise
        Snippet(page('<div class="b" id="hidden">a</div>'), hit=(d1,)),
        Snippet(page('<div id="a" class="hidden">a</div>'), miss=(d1,)),
        Snippet(page('<div id="hidden" style="x">a</div>'), hit=(d1,)),
        Snippet(page('<div style="x" id="hidden">a</div>'), miss=(d1,)),
        Snippet(page('<div style="hidden">a</div>'), hit=(d1,)),
        Snippet(page('<div style="color: hidden">a</div>'), hit=(d1,)),
        Snippet(page('<div id="a" style="hidden">a</div>'), miss=(d1,)),
        Snippet(page('<div id="x-hidden-y">a</div>'), hit=(d1,)),
        Snippet(page('<div id="Hidden">a</div>'), miss=(d1,)),
    ]
    out += plain(d1, "style", ["display:none", "color:red;display:none", "display: none"], hit=True)
    out += plain(d1, "style", ["DISPLAY:NONE", "display:  none", "display :none", "display:non"], hit=False)
    out += plain(d1, "id", ["reader-comments", "akismet", "x-akismet-y", "my-reader-comments"], hit=True)
    out += plain(d1, "id", ["Reader-comments", "reader_comments", "Akismet"], hit=False)
    class_d1 = {
        "^hide-": "hide-me",
        "comments-title": "x-comments-title-y",
        "nocomments": "nocomments",
        "-reply-": "a-reply-b",
        "message": "my-message",
        "akismet": "akismet-box",
        "suggest-links": "suggest-links",
        "-hide-": "a-hide-b",
        "hide-print": "xhide-print",
        " hidden": "a hidden",
        " hide": ("a hide", "a hide-x"),
        "noprint": "noprint",
        "notloaded": "notloaded",
    }
    out += tokens_snippets(d1, "class", tuple(class_d1), class_d1, tag="span")
    out += plain(d1, "class", ["hidden", "xhide-me", "hide", "Hide-me", "no-print", "reply", "-reply"], hit=False)
    out += plain(d1, "class", ["x hide-me", "x hide"], hit=True)
    out += plain(d1, "class", ["hidden x", "hidden"], hit=False)
    out += plain(d1, "aria-hidden", ["true"], tag="span")
    out += plain(d1, "aria-hidden", ["false", "True", "true "], hit=False)
    out += plain(d1, "data-x", ["hidden", "display:none", "message"], hit=False)
    # an element of any name, and an SVG element too
    for t in ("a", "h2", "li", "article", "td"):
        out.append(Snippet(page(f'<{t} aria-hidden="true">a</{t}>') if t != "td" else page('<p aria-hidden="true">a</p>'), hit=(d1,)))
    out.append(Snippet(page('<svg class="message"><title class="hide-me">t</title></svg>'), hit=(d1,)))

    # ---- TEASER_DISCARD_XPATH
    t0 = "TEASER_DISCARD_XPATH[0]"
    out += plain(t0, "id", ["teaser", "Teaser", "x-teaser-y", "my-Teaser"], hit=True)
    out += plain(t0, "class", ["teaser", "Teaser", "post-Teaser", "teasers"], hit=True)
    out += plain(t0, "id", ["TEASER", "teas", "te aser"], hit=False)
    out += plain(t0, "class", ["TEASER", "Teas er", "tease"], hit=False)
    out += [Snippet(page(element(t, "class", "Teaser")), hit=(t0,)) for t in ("item", "list", "p", "section", "span")]
    out += [Snippet(page(element(t, "class", "Teaser")), miss=(t0,)) for t in ("a", "article", "li", "h2")]

    # ---- PRECISION_DISCARD_XPATH
    out += [
        Snippet(page("<header>a</header>"), hit=(pd(0),)),
        Snippet(page("<div><header><header>a</header></header></div>"), hit=(pd(0),)),
        Snippet(page("<headers>a</headers><div class='header'>b</div>"), miss=(pd(0),)),
    ]
    p1 = pd(1)
    bottom_hit = ["bottom", "bottom-bar", "article-bottom", "x bottom", "bottom x", "bottom\n", "a-bottom\n", "x\nbottom",
                  "x bottom-bar", "a\tbottom", "bottom\N{NO-BREAK SPACE}x", "x\N{EM SPACE}bottom\u0085", "a\N{LINE SEPARATOR}bottom", "a bottom\N{IDEOGRAPHIC SPACE}b", "bottom\r"]
    # `(^|\s)bottom` and `bottom(\s|$)` each anchor one side only
    bottom_hit += ["bottoms", "bottom_", "a_bottom", "bottom\N{ZERO WIDTH SPACE}", "x\N{ZERO WIDTH SPACE}bottom"]
    bottom_miss = ["border-bottom-0", "Padding-bottom-lg-30", "Bottom", "xbottomx", "bot tom",
                   "a\N{ZERO WIDTH SPACE}bottom\N{ZERO WIDTH SPACE}b"]
    out += plain(p1, "class", bottom_hit, hit=True)
    out += plain(p1, "class", bottom_miss, hit=False)
    out += plain(p1, "id", ["bottom", "page-bottom", "a bottom"], hit=True)
    out += plain(p1, "id", ["border-bottom-0", "xbottomx"], hit=False)
    link_hit = ["link", "a link b", "link x", "x link", "link\n", "x\nlink", "x\N{NO-BREAK SPACE}link\N{EM SPACE}y", "a\tlink"]
    link_miss = ["permalink", "headline-link", "link-x", "Link", "links", "xlink", "link_", "a-link", "link\N{ZERO WIDTH SPACE}", "a\N{ZERO WIDTH SPACE}link"]
    out += plain(p1, "class", link_hit, hit=True)
    out += plain(p1, "class", link_miss, hit=False)
    out += plain(p1, "id", ["link", "a link"], hit=True)
    out += plain(p1, "id", ["permalink", "link-x"], hit=False)
    out += plain(p1, "style", ["border:1px solid", "x;border-top:0", "border", "xborder"], hit=True)
    out += plain(p1, "style", ["Border:1px", "bord er"], hit=False)
    out += [
        Snippet(page('<div id="a" class="link">x</div>'), miss=(p1,)),
        Snippet(page('<div class="link" id="a">x</div>'), hit=(p1,)),
        Snippet(page('<div id="a" class="bottom">x</div>'), miss=(p1,)),
        Snippet(page('<div class="bottom" id="a">x</div>'), hit=(p1,)),
        Snippet(page('<div id="a" class="x" style="border:0">x</div>'), hit=(p1,)),
        Snippet(page('<div id="a" class="x">x</div><div>y</div>'), miss=(p1,)),
    ]
    out += [Snippet(page(element(t, "class", "link")), hit=(p1,)) for t in ("item", "list", "p", "section", "span")]
    out += [Snippet(page(element(t, "class", "link")), miss=(p1,)) for t in ("a", "article", "li", "ul", "h2")]
    out += [Snippet(page(element(t, "style", "border:0")), miss=(p1,)) for t in ("a", "article")]

    # ---- DISCARD_IMAGE_ELEMENTS
    di = "DISCARD_IMAGE_ELEMENTS[0]"
    out += plain(di, "id", ["caption", "image-caption", "figcaption", "captions"], hit=True)
    out += plain(di, "class", ["caption", "wp-caption", "x caption y", "figcaption"], hit=True)
    out += plain(di, "id", ["Caption", "capt ion"], hit=False)
    out += plain(di, "class", ["Caption", "capt-ion", "cap"], hit=False)
    out += [Snippet(page(element(t, "class", "caption")), hit=(di,)) for t in ("item", "list", "p", "section", "span")]
    out += [Snippet(page(element(t, "class", "caption")), miss=(di,)) for t in ("a", "figcaption", "figure", "li", "article")]
    out.append(Snippet(page('<div id="x" class="caption">a</div>'), hit=(di,)))

    # ---- COMMENTS_DISCARD_XPATH
    out += plain(cd(0), "id", ["respond", "respond-x", "respondent"], hit=True)
    out += plain(cd(0), "id", ["Respond", "x-respond", "correspond"], hit=False)
    out += plain(cd(0), "class", ["respond"], hit=False)
    out += plain(cd(0), "id", ["respond"], tag="section")
    out += plain(cd(0), "id", ["respond"], tag="span", hit=False)
    out += plain(cd(0), "id", ["respond"], tag="p", hit=False)
    out += [
        Snippet(page("<cite>a</cite>"), hit=(cd(1),)),
        Snippet(page("<quote>a</quote>"), hit=(cd(1),)),
        # a union comes back in document order: the quote first, though .//cite is the first operand
        Snippet(page("<quote>a</quote><p>b</p><cite>c</cite>"), hit=(cd(1),)),
        Snippet(page("<cite>a<quote>b</quote></cite><quote>c<cite>d</cite></quote>"), hit=(cd(1),)),
        Snippet(page("<div><blockquote>a</blockquote><cites>b</cites><q>c</q></div>"), miss=(cd(1),)),
        Snippet(page("<svg><cite>a</cite><quote>b</quote></svg><cite>c</cite>"), hit=(cd(1),)),
        Snippet(page("<svg><cite>a</cite><quote>b</quote></svg>"), hit=(cd(1),)),
    ]
    out += plain(cd(2), "class", ["comments-title"], hit=True)
    out += plain(cd(2), "style", ["display:none", "x;display:none;y"], hit=True)
    out += plain(cd(2), "style", ["display: none", "DISPLAY:none"], hit=False)
    out += plain(cd(2), "class", ["comments-title", "nocomments", "a-reply-b", "message", "signin", "x-signin-y", "my-message"], hit=True)
    out += plain(cd(2), "class", ["Message", "sign-in", "comments_title", "reply", "xx"], hit=False)
    out += plain(cd(2), "id", ["reply-x", "akismet", "x-akismet-y", "reply-"], hit=True)
    out += plain(cd(2), "class", ["reply-x", "akismet", "x-akismet-y"], hit=True)
    out += plain(cd(2), "id", ["Reply-x", "xreply-", "Akismet", "message", "signin"], hit=False)
    out += [
        Snippet(page('<div class="reply-x" id="y">a</div>'), hit=(cd(2),)),
        Snippet(page('<div id="y" class="reply-x">a</div>'), miss=(cd(2),)),
        Snippet(page('<div id="reply-x" class="y">a</div>'), hit=(cd(2),)),
        Snippet(page('<div class="y" id="reply-x">a</div>'), miss=(cd(2),)),
        Snippet(page('<span class="signin">a</span><a class="message">b</a><li class="nocomments">c</li>'), hit=(cd(2),)),
        Snippet(page('<svg class="signin"><title class="message">t</title></svg>'), hit=(cd(2),)),
    ]

    # ---- METADATA: `//` selectors start at the document root, so the root element can match
    a0, a1, a2 = (f"AUTHOR_XPATHS[{i}]" for i in range(3))
    out += plain(a0, "rel", ["author"])
    out += plain(a0, "rel", ["Author", "author x", "authors"], hit=False)
    out += plain(a0, "id", ["author"])
    out += plain(a0, "id", ["Author", "author-x"], hit=False)
    out += plain(a0, "class", ["author"])
    out += plain(a0, "class", ["authors", "my author"], hit=False)
    out += plain(a0, "itemprop", ["author name"])
    out += plain(a0, "itemprop", ["name author", "author", "Author name", "author  name"], hit=False)
    out += plain(a0, "data-testid", ["AuthorCard", "AuthorURL"])
    out += plain(a0, "data-testid", ["authorcard", "AuthorCard2", "AuthorUrl"], hit=False)
    out += plain(a0, "class", ["author-name", "authorname", "AuthorName", "authorName", "x-author-name-y", "my-AuthorName"])
    out += plain(a0, "class", ["author--name", "authorNam", "author name", "Authorname", "author_name"], hit=False)
    out += [
        # `rel='me'` is a child element named rel whose string value is 'me', not an attribute
        Snippet(page("<a><rel>me</rel></a>"), hit=(a0,)),
        Snippet(page("<div><rel>me</rel><rel>no</rel></div>"), hit=(a0,)),
        Snippet(page("<div><rel>no</rel><rel>me</rel></div>"), hit=(a0,)),
        Snippet(page("<span><rel>m<b>e</b></rel></span>"), hit=(a0,)),
        Snippet(page("<p><rel>me</rel>x</p>"), hit=(a0,)),
        Snippet(page("<a><rel>Me</rel></a>"), miss=(a0,)),
        Snippet(page("<a><rel> me</rel></a>"), miss=(a0,)),
        Snippet(page("<a><rel>me </rel></a>"), miss=(a0,)),
        Snippet(page("<a><rel>mee</rel></a>"), miss=(a0,)),
        Snippet(page("<a><rel></rel></a>"), miss=(a0,)),
        Snippet(page("<a><b><rel>me</rel></b></a>"), miss=(a0,)),
        Snippet(page("<h3><rel>me</rel></h3>"), miss=(a0,)),
        Snippet(page('<a rel="me">x</a>'), miss=(a0,)),
        Snippet(page('<a me="rel">x</a><a rel="x"><me>rel</me></a>'), miss=(a0,)),
        Snippet(page("<div><svg><rel>me</rel></svg></div>"), miss=(a0,)),
        Snippet(page("<author>a</author>"), hit=(a0,)),
        Snippet(page("<div><author>a</author></div><p>x</p><author>b</author>"), hit=(a0,)),
        Snippet(page("<authors>a</authors><auth>b</auth><div class='Author'>c</div>"), miss=(a0,)),
        Snippet(page("<svg><author>a</author></svg>"), hit=(a0,)),
        # a union is in document order, whichever operand finds the node
        Snippet(page('<author>a<div class="author">b</div><author>c</author></author><p id="author">d</p>'), hit=(a0, a1, a2)),
        Snippet(page('<div class="author"><author>x</author></div><author>y</author>'), hit=(a0, a1, a2)),
        Snippet(page('<div class="author"><span class="author-name">a</span></div><p id="author">b</p>'), hit=(a0, a1, a2)),
    ]
    out += by_tag(a0, "class", "author", ("a", "address", "div", "link", "p", "span", "strong"), ("h3", "h4", "li", "article", "section", "ul", "svg"))
    out += by_tag(a0, "rel", "author", ("a", "address", "div", "p", "span", "strong"), ("h3", "li", "article", "svg", "header"))
    # the root element is not one of a0's tags
    out += [
        Snippet('<html class="author"><body><p>x</p></body></html>', hit=(a2,), miss=(a0, a1)),
        Snippet('<html id="author"><body><p>x</p></body></html>', hit=(a2,), miss=(a0, a1)),
        Snippet('<html itemprop="author name"><body><p>x</p></body></html>', hit=(a2,), miss=(a0,)),
        Snippet('<html rel="author"><body><p>x</p></body></html>', miss=(a0, a1, a2)),
        Snippet('<html><body class="author"><p>x</p></body></html>', hit=(a2,), miss=(a0, a1)),
        Snippet('<html><body itemprop="name author"><p>x</p></body></html>', hit=(a2,), miss=(a0, a1)),
        Snippet('<html><body><div itemprop="author name">a</div><div itemprop="name author">b</div></body></html>', hit=(a0, a1, a2)),
    ]

    out += plain(a1, "class", ["byline", "username", "byl", "BBL"])
    out += plain(a1, "class", ["Byline", "bylines", "user name", "bbl", "Username", "byl x", "BBL1"], hit=False)
    out += plain(a1, "itemprop", ["author", "x-author-y", "author name", "name author", "authors"])
    out += plain(a1, "itemprop", ["Author", "auth", "writer"], hit=False)
    out += plain(a1, "id", ["author", "zuozhe", "bianji", "xiaobian", "x-author-y", "my-bianji", "authors"])
    out += plain(a1, "id", ["Author", "xiao bian", "Zuozhe", "xiao-bian", "auth"], hit=False)
    out += plain(a1, "class", ["author", "channel-name", "zuozhe", "bianji", "xiaobian", "submitted-by", "posted-by", "journalist-name", "my-author", "x posted-by y"])
    out += plain(a1, "class", ["channelname", "Channel-name", "submitted by", "posted_by", "journalist name", "Author", "submitted_by", "xiao bian"], hit=False)
    out += by_tag(a1, "class", "channel-name", ("a", "div", "h3", "h4", "p", "span"), ("h1", "h2", "address", "strong", "li", "link", "article", "svg"))
    out += by_tag(a1, "id", "author", ("a", "div", "h3", "h4", "p", "span"), ("h1", "li", "article"))
    out += by_tag(a1, "itemprop", "author", ("a", "div", "h3", "h4", "p", "span"), ("h2", "li", "strong"))
    out += by_tag(a1, "class", "byline", ("a", "div", "h3", "h4", "p", "span"), ("h1", "li", "address"))

    out += plain(a2, "data-component", ["Byline", "x-Byline-y", "Bylines"])
    out += plain(a2, "data-component", ["byline", "BYLINE", "By line"], hit=False)
    out += plain(a2, "itemprop", ["author", "x-author", "author name", "authors"])
    out += plain(a2, "itemprop", ["Author", "auth", "writer"], hit=False)
    out += plain(a2, "id", ["author", "Author", "x-Author-y", "authors"])
    out += plain(a2, "id", ["AUTHOR", "auth or", "uthor", "writer", "byline"], hit=False)
    out += plain(a2, "class", ["author", "Author", "screenname", "writer", "byline", "Byline", "my-writer-x", "x-screenname"])
    out += plain(a2, "class", ["AUTHOR", "screen name", "Writer", "BYLINE", "screenName", "writ er", "by line", "auth"], hit=False)
    out += plain(a2, "data-x", ["author", "Byline", "writer"], hit=False)
    out += by_tag(a2, "class", "author", ("li", "h1", "article", "ul", "svg", "header", "strong"), ())
    out.append(Snippet(page('<svg><title class="writer">t</title></svg>'), hit=(a2,)))

    ad0 = "AUTHOR_DISCARD_XPATHS[0]"
    out += plain(ad0, "class", ["comments", "title", "date"])
    out += plain(ad0, "class", ["Title", "titles", "x title", "dates", "Date", "date x"], hit=False)
    out += plain(ad0, "id", ["comments", "comments-x", "commentsx", "comment-list", "commentlist", "x-commentlist-y", "ProductReviews", "x-ProductReviews-y"])
    out += plain(ad0, "id", ["Comments", "xcomments", "x-comments", "productreviews", "comment_list", "comment list", "comment--list", "Comment"], hit=False)
    out += plain(ad0, "class", ["comments", "Comments", "comments-x", "Commentsx", "commentlist", "x commentlist y", "comments-list", "x comments-list y",
                                "sidebar", "my-sidebar-x", "is-hidden", "quote", "blockquote", "embedly-instagram", "article-share", "article-support",
                                "x article-share", "print", "print-only", "category", "my-category", "meta-date", "meta-reviewer"])
    out += plain(ad0, "class", ["Comment", "Comment-x", "xcomments", "x comments", "comment list", "Sidebar", "is hidden", "Quote",
                                "embedly instagram", "article-other", "article_share", "articleshare", "Print", "categories", "Category", "meta date"], hit=False)
    out += plain(ad0, "data-component", ["Figure", "x-Figure-y", "Figures"])
    out += plain(ad0, "data-component", ["figure", "FIGURE", "Fig ure"], hit=False)
    out += by_tag(ad0, "class", "comments", ("a", "div", "section", "span"), ("p", "li", "article", "h3", "svg"))
    out += [
        # `.//`: the context node is never a match, its descendants are
        Snippet(page('<div id="out" class="comments"><span data-context class="comments" id="ctx"><span class="comments" id="in">a</span></span></div>'), hit=(ad0,)),
        Snippet(page('<div id="out" class="comments"><span data-context id="ctx"><p class="comments">a</p></span></div>'), miss=(ad0,)),
        Snippet(page('<div class="comments"><div class="title"><span id="comments">a</span></div></div><a class="date">b</a>'), hit=(ad0,)),
    ]
    ad1 = "AUTHOR_DISCARD_XPATHS[1]"
    out += [
        Snippet(page("<time>a</time>"), hit=(ad1,)),
        Snippet(page("<figure>a</figure>"), hit=(ad1,)),
        Snippet(page("<figure><time>a</time></figure><p>b</p><time>c</time>"), hit=(ad1,)),
        Snippet(page("<time><figure>a</figure></time>"), hit=(ad1,)),
        Snippet(page("<div><figure><figure>a</figure></figure></div>"), hit=(ad1,)),
        Snippet(page("<times>a</times><figcaption>b</figcaption><div class='time'>c</div>"), miss=(ad1,)),
        Snippet(page("<svg><time>a</time><figure>b</figure></svg>"), hit=(ad1,)),
        Snippet(page("<svg><time>a</time></svg><figure>b</figure>"), hit=(ad1,)),
    ]

    # ---- CATEGORIES_XPATHS and TAGS_XPATHS: `//holder//a[@href]`; a link under several holders is one match
    c0, c1, c2, c3, c4, c5 = (f"CATEGORIES_XPATHS[{i}]" for i in range(6))
    t0, t1, t2, t3 = (f"TAGS_XPATHS[{i}]" for i in range(4))
    out += under(c0, "div", "class", ["post-info", "postinfo", "post-meta", "postmeta", "meta", "entry-meta", "entry-info", "entry-utility",
                                      "meta-x", "postinfo x", "post-meta-data", "metadata", "entry-utility x"])
    out += under(c0, "div", "class", ["x meta", "Meta", "entry_meta", "post--info", "info", "Post-info", "postin fo", "my-entry-meta", "entry meta"], hit=False)
    out += under(c0, "div", "id", ["postpath", "postpath-x", "postpathx"])
    out += under(c0, "div", "id", ["Postpath", "x-postpath", "post-path"], hit=False)
    out += under(c0, "span", "class", ["meta", "postinfo"], hit=False)
    out += under(c0, "section", "id", ["postpath"], hit=False)
    out += [
        Snippet(page('<div class="meta"><p><span><a href="/c">c</a></span></p></div>'), hit=(c0,)),
        Snippet(page('<div class="meta"><a href="">c</a></div>'), hit=(c0,)),
        Snippet(page('<div class="meta"><a>c</a></div>'), miss=(c0,)),
        Snippet(page('<div class="meta"><a name="x">c</a></div>'), miss=(c0,)),
        Snippet(page('<div class="meta"><a HREF="/c">c</a></div>'), hit=(c0,)),
        Snippet(page('<div class="meta"><span href="/c">c</span></div>'), miss=(c0,)),
        Snippet(page('<div class="meta"></div><a href="/c">c</a>'), miss=(c0,)),
        Snippet(page('<a href="/c">c</a><div class="meta">x</div>'), miss=(c0,)),
        Snippet(page('<div class="meta"><svg><a href="/c">c</a></svg></div>'), hit=(c0,)),
        Snippet(page('<div class="meta"><a href="/1">a</a><div class="postmeta"><a href="/2">b</a><a href="/3">c</a></div><a href="/4">d</a></div><a href="/5">e</a>'), hit=(c0,)),
        Snippet(page('<div id="postpath"><div class="x"><a href="/1">a</a></div></div><div class="meta"><a href="/2">b</a></div>'), hit=(c0,)),
        Snippet(page('<div class="meta" id="postpath"><a href="/1">a</a></div>'), hit=(c0,)),
    ]
    out += under(c1, "p", "class", ["postmeta", "postmeta-x", "entry-categories", "entry-categories-x", "postinfo", "postmetas"])
    out += under(c1, "p", "class", ["Postmeta", "x postmeta", "postinfo x", "postinfos", "entry-category", "Postinfo", "post-meta", "entry_categories"], hit=False)
    out += under(c1, "p", "id", ["filedunder"])
    out += under(c1, "p", "id", ["filedunder2", "Filedunder", "filed-under"], hit=False)
    out += under(c1, "div", "class", ["postmeta", "postinfo"], hit=False)
    out += under(c1, "span", "id", ["filedunder"], hit=False)
    out += under(c2, "footer", "class", ["entry-meta", "entry-meta-x", "entry-footer", "entry-footer-y"])
    out += under(c2, "footer", "class", ["x entry-meta", "Entry-meta", "entry_meta", "entry footer", "meta"], hit=False)
    out += under(c2, "footer", "id", ["entry-meta", "entry-footer"], hit=False)
    out += under(c2, "div", "class", ["entry-meta"], hit=False)
    out += under(c2, "header", "class", ["entry-footer"], hit=False)
    for tag in ("li", "span"):
        out += under(c3, tag, "class", ["post-category", "postcategory", "entry-category", "cat-links", "x cat-links y", "cat-links-2", "mycat-links"])
        out += under(c3, tag, "class", ["post-categories", "Post-category", "post-category x", "cat-link", "cat_links", "entry-categories", "cat links"], hit=False)
    out += under(c3, "li", "id", ["post-category", "cat-links"], hit=False)
    out += [Snippet(page(element(t, "class", "post-category", LINK)), miss=(c3,)) for t in ("p", "div", "ul", "h3", "section")]
    out += under(c4, "header", "class", ["entry-header"])
    out += under(c4, "header", "class", ["entry-header x", "Entry-header", "entry-headers", "x entry-header", "header"], hit=False)
    out += under(c4, "header", "id", ["entry-header"], hit=False)
    out += under(c4, "div", "class", ["entry-header"], hit=False)
    out += under(c5, "div", "class", ["row", "tags"])
    out += under(c5, "div", "class", ["rows", "row x", "Tags", "tag", "x tags", "Row"], hit=False)
    out += under(c5, "div", "id", ["row", "tags"], hit=False)
    out += under(c5, "span", "class", ["row", "tags"], hit=False)
    out += under(t0, "div", "class", ["tags"])
    out += under(t0, "div", "class", ["tags x", "row", "Tags", "x tags", "tag"], hit=False)
    out += under(t0, "span", "class", ["tags"], hit=False)
    out += under(t1, "p", "class", ["entry-tags", "entry-tags-x"])
    out += under(t1, "p", "class", ["x entry-tags", "entry-tag", "Entry-tags", "entry_tags"], hit=False)
    out += under(t1, "div", "class", ["entry-tags"], hit=False)
    out += under(t2, "div", "class", ["row", "jp-relatedposts", "entry-utility", "tag", "tags x", "tag-list", "postmeta", "postmeta-x", "meta", "metadata"])
    out += under(t2, "div", "class", ["rows", "x tag", "Tag", "post-meta", "entry-meta", "jp-relatedposts x", "entry-utility x", "row x", "relatedposts", "x meta"], hit=False)
    out += under(t2, "div", "id", ["row", "tag", "meta"], hit=False)
    out += under(t2, "span", "class", ["tag", "row"], hit=False)
    for tag in ("div", "span", "li", "section", "p", "ul"):
        out += under(t3, tag, "class", ["entry-meta", "topics", "related-topics-x", "tags-links", "x tags-links y"])
    out += under(t3, "div", "class", ["entry-meta x", "Topics", "tag-links", "tags_links", "entry meta", "topic", "x entry-meta", "tags-link"], hit=False)
    out += under(t3, "div", "id", ["entry-meta", "topics", "tags-links"], hit=False)
    out += [
        # the root element can be a holder
        Snippet('<html class="entry-meta"><body><a href="/c">c</a></body></html>', hit=(t3,)),
        Snippet('<html class="topics"><body><p>x</p></body></html>', miss=(t3,)),
        Snippet('<html><body class="tags-links"><a href="/c">c</a></body></html>', hit=(t3,)),
        Snippet(page('<div class="tags"><svg><a href="/c">c</a></svg></div>'), hit=(t0,), miss=(t3,)),
        Snippet(page('<div class="tags"><a href="/1">a</a></div><div class="tags"><p><a href="/2">b</a></p><a>c</a></div>'), hit=(c5, t0)),
        Snippet(page('<div class="row"><div class="tags"><a href="/1">a</a></div><a href="/2">b</a></div>'), hit=(c5, t0, t2)),
        Snippet(page('<div class="tags"><a href="/1">a</a></div><a href="/2">b</a>'), hit=(c5, t0, t2), miss=(t1,)),
        Snippet(page('<div class="entry-meta"><a href="/1">a</a></div><footer class="entry-meta"><a href="/2">b</a></footer>'), hit=(t3, c2), miss=(t2,)),
        Snippet(page('<p class="topics"><a href="/1">a</a></p><span class="entry-meta"><b><a href="/2">b</a></b></span>'), hit=(t3,)),
    ]

    ti0, ti1, ti2 = (f"TITLE_XPATHS[{i}]" for i in range(3))
    out += plain(ti0, "class", ["post-title", "entry-title", "article-title", "post__title", "headline", "x-headline-y", "my-post-title", "xpost-title",
                                "post-titles"], tag="h1")
    out += plain(ti0, "class", ["posttitle", "entry_title", "Headline", "post_title", "article--title", "title", "post-_title", "Post-title", "post--title",
                                "post__ title", "head line", "HEADLINE"], tag="h1", hit=False)
    out += plain(ti0, "id", ["headline", "x-headline", "headlines"], tag="h2")
    out += plain(ti0, "id", ["Headline", "head-line", "title"], tag="h2", hit=False)
    out += plain(ti0, "itemprop", ["headline", "x headline", "headlines"], tag="h1")
    out += plain(ti0, "itemprop", ["Headline", "head line", "name"], tag="h1", hit=False)
    out += by_tag(ti0, "class", "post-title", ("h1", "h2"), ("h3", "h4", "div", "p", "span", "header", "article", "svg"))
    out += by_tag(ti0, "itemprop", "headline", ("h1", "h2"), ("h3", "div", "span"))
    out += by_tag(ti0, "id", "headline", ("h1", "h2"), ("h3", "div", "p"))
    out += plain(ti1, "class", ["entry-title", "post-title"], tag="h1")
    out += plain(ti1, "class", ["entry-title x", "Entry-title", "post-titles", "x post-title", "article-title", "entry_title", "title", "post title"], tag="h1", hit=False)
    out += plain(ti1, "id", ["entry-title", "post-title"], tag="h1", hit=False)
    out += by_tag(ti1, "class", "entry-title", ("h1", "div", "span", "p", "li", "a", "article", "h4", "svg"), ())
    out += by_tag(ti1, "class", "post-title", ("h2", "section", "header"), ())
    out += [
        Snippet(page('<div class="entry-title"><span class="post-title">a</span></div><p class="entry-title">b</p>'), hit=(ti1,)),
        Snippet('<html class="entry-title"><body><p>x</p></body></html>', hit=(ti1,)),
        Snippet('<html class="post-title"><body><p>x</p></body></html>', hit=(ti1,), miss=(ti0, ti2)),
        Snippet('<html itemprop="headline"><body><p>x</p></body></html>', miss=(ti0, ti1, ti2)),
        Snippet('<html class="title"><body><p>x</p></body></html>', miss=(ti0, ti1, ti2)),
        Snippet(page('<h1 class="post-title">a</h1><h2 class="entry-title">b</h2><h3 class="title">c</h3>'), hit=(ti0, ti1, ti2)),
        Snippet(page('<div class="entry-title">a</div><div class="x">b</div>'), hit=(ti1,), miss=(ti0, ti2)),
    ]
    out += plain(ti2, "class", ["title", "my-title-x", "subtitle", "titles"], tag="h1")
    out += plain(ti2, "class", ["Title-x", "Titles", "ti-tle"], tag="h1", hit=False)
    out += plain(ti2, "id", ["title", "page-title", "x-title-y", "subtitle"], tag="h2")
    out += plain(ti2, "id", ["Title", "ti tle", "headline"], tag="h2", hit=False)
    out += by_tag(ti2, "class", "title", ("h1", "h2", "h3"), ("h4", "div", "p", "span", "header", "svg"))
    out += by_tag(ti2, "id", "title", ("h1", "h2", "h3"), ("h4", "div", "span"))
    out += [
        Snippet(page('<h1 id="a" class="title">a</h1><h2 class="b" id="title">b</h2>'), hit=(ti2,)),
    ]

    # ---- BASIC_CLEAN_XPATH
    out += [
        Snippet(page("<aside>a</aside><p>b</p>"), hit=(clean,)),
        Snippet(page("<footer>a</footer>"), hit=(clean,)),
        Snippet(page("<fencedframe>a</fencedframe>"), hit=(clean,)),
        Snippet(page("<script>var a = 1;</script>"), hit=(clean,)),
        Snippet("<html><head><script>var a;</script><style>p{}</style></head><body><p>x</p></body></html>", hit=(clean,)),
        Snippet(page("<style>p{}</style>"), hit=(clean,)),
        Snippet(page("<svg><title>t</title></svg>"), hit=(clean,)),
        Snippet(page("<template></template>"), hit=(clean,)),
        Snippet(page("<div><aside><aside>a</aside></aside></div>"), hit=(clean,)),
        Snippet(page("<asides>a</asides><footers>b</footers><scripts>c</scripts><styles>d</styles>"), miss=(clean,)),
        Snippet(page("<p>no boilerplate</p><div>x</div>"), miss=(clean,)),
        Snippet(page("<div>x</div><div class>y</div><div class=''>z</div>"), miss=(clean,)),
    ]
    out += plain(clean, "class", ["footer", "site-footer", "Xfooterx"], hit=True)
    out += plain(clean, "id", ["footer", "site-footer"], hit=True)
    out += plain(clean, "class", ["Footer", "FOOTER", "foot er"], hit=False)
    out += plain(clean, "id", ["Footer"], hit=False)
    out += plain(clean, "class", ["footer"], tag="span", hit=False)
    out += plain(clean, "class", ["footer"], tag="p", hit=False)
    out += plain(clean, "class", ["footer"], tag="section", hit=False)
    out += [
        # contains(@class|@id, 'footer') reads the first attribute in source order
        Snippet(page('<div class="footer" id="x">a</div>'), hit=(clean,)),
        Snippet(page('<div id="x" class="footer">a</div>'), miss=(clean,)),
        Snippet(page('<div id="footer" class="x">a</div>'), hit=(clean,)),
        Snippet(page('<div class="x" id="footer">a</div>'), miss=(clean,)),
    ]
    consent_values = [
        "cookie-banner", "cookiebar", "cookie_consent", "cookielaw", "cookie-notice", "cookie_description",
        "notice-cookie", "notice__cookie", "noticecookie", "consent-banner", "consentmanager", "consent_sdk", "borlabs",
        "cookiebot", "cmplz", "onetrust", "moove-gdpr", "moovegdpr", "x cookie-banner y", "my-onetrust-x",
    ]
    consent_ignorecase = [
        "COOKIE-BANNER", "CookieBot", "ONETRUST", "OneTrust", "Cmplz", "BORLABS", "Cookie_Policy", "NOTICE-COOKIE", "MOOVE-GDPR",
        "cook\N{LATIN SMALL LETTER DOTLESS I}e-banner", "cook\N{LATIN CAPITAL LETTER I WITH DOT ABOVE}e-banner", "COOK\N{LATIN CAPITAL LETTER I WITH DOT ABOVE}EBOT", "cook\N{LATIN SMALL LETTER DOTLESS I}ebot", "onetrust".replace("i", "\N{LATIN SMALL LETTER DOTLESS I}"), "cookie-pol\N{LATIN SMALL LETTER DOTLESS I}cy", "cookie-pol\N{LATIN CAPITAL LETTER I WITH DOT ABOVE}cy",
        "borlab\N{LATIN SMALL LETTER LONG S}", "coo\N{KELVIN SIGN}ie-banner", "con\N{LATIN SMALL LETTER LONG S}ent-banner",
    ]
    consent_miss = [
        "cookie", "cookie-", "cookies", "cookie-bann", "cookie--policy", "\N{KELVIN SIGN}ookie-banner", "cookie-settings", "notice-and-cookie", "notice---cookie", "consent",
        "consent-form", "moove--gdpr", "cookie_ban ner", "one trust", "cmpl", "borlab",
    ]
    for attr in ("class", "id"):
        out += plain(clean, attr, consent_values + consent_ignorecase, hit=True)
        out += plain(clean, attr, consent_miss, hit=False)
    out += [
        # the two clauses test @class and @id separately, so the order of the attributes does not matter
        Snippet(page('<div class="x" id="cookiebot">a</div>'), hit=(clean,)),
        Snippet(page('<span id="x" class="cookiebot">a</span>'), hit=(clean,)),
        Snippet(page('<p data-x="cookiebot" title="cookiebot">a</p>'), miss=(clean,)),
        Snippet(page('<a class="cookie-banner"></a>'), hit=(clean,)),
    ]

    # ---- no foreign content: libxml2 builds svg and math as HTML elements, so a name test sees them too
    out += [
        Snippet(page('<svg id="s"><title class="x">t</title><style>s</style><a id="a" class="comment">l</a></svg><p>b</p>'), hit=(clean,)),
        Snippet(page('<svg class="cookiebot"><title>t</title></svg>'), hit=(clean,)),
        Snippet(page('<svg><aside>a</aside><footer>b</footer><script>c</script><template></template></svg>'), hit=(clean,)),
        Snippet(page('<svg><a class="main">l</a><section class="post-body">s</section><article>a</article></svg>'), hit=(b(0), b(1)), miss=(b(2), b(4))),
        Snippet(page('<svg><header>h</header><div class="x"></div></svg>'), hit=(pd(0),)),
        Snippet(page('<svg><section class="comments">s</section><details id="comments">d</details></svg>'), hit=(c(1), rc, rcl)),
        Snippet(page('<svg><section class="social">s</section><section class="teaser">t</section><section class="caption">c</section></svg>'),
                hit=(d0, di), miss=(t0,)),
        Snippet(page('<svg class="infinitescroll"><aside class="infinitescroll">a</aside></svg>'), hit=(p0,)),
        Snippet(page('<math><mi class="message">x</mi></math>'), hit=(d1,)),
        Snippet(page('<svg><svg class="hide-me"><title>t</title></svg></svg>'), hit=(d1, clean)),
        Snippet(page('<svg><a>l</a></svg><a>m</a><svg><title>t</title></svg><title>u</title>'), hit=(clean,)),
    ]

    # ---- the context node
    out += [
        # context is a nested element: the matches are its descendants, never itself
        Snippet(page('<div class="post-body" id="out"><div data-context class="post-body" id="ctx"><div class="post-body" id="in">a</div></div></div>'),
                hit=(b(0),)),
        Snippet(page('<article id="out"><div data-context="1"><article id="in">a</article></div></article><article id="after">b</article>'),
                hit=(b(1),)),
        Snippet(page('<div class="comment" id="out"><section data-context class="comments" id="ctx"><div id="comments">a</div><ul id="comments">b</ul></section></div>'),
                hit=(rc, rcl)),
        Snippet(page('<div id="out" class="social"><div data-context id="social"><p class="share-this">a</p><span class="cookie">b</span></div></div>'),
                hit=(d0,)),
        Snippet(page('<aside id="out"><div data-context><aside id="in">a</aside><footer id="f">b</footer></div></aside>'), hit=(clean,)),
        Snippet(page('<div data-context="x"></div><div class="post-body"></div>'), miss=(b(0),)),
        Snippet(page('<div id="first" class="post-body"><p data-context>a</p></div>'), miss=(b(0),)),
    ]
    # whitespace-only and plain structure: nothing to select
    out += [
        Snippet(page("")),
        Snippet(page("<p>text</p>")),
        Snippet(page('<div id="a" class="b"><p>x</p></div>'), miss=(b(0), c(1), d0, d1)),
        Snippet(page('<div>\n  <span>x</span>\n</div>')),
    ]
    return out


def selectors() -> list[tuple[str, object]]:
    "(name, XPath) for every selector, in the order the cases list them."
    from trafilatura import xpaths as x
    from trafilatura.settings import BASIC_CLEAN_XPATH

    names = (
        "BODY_XPATH", "COMMENTS_XPATH", "REMOVE_COMMENTS_XPATH", "REMOVE_COMMENTS_AND_LISTS_XPATH",
        "RAW_TREE_PRUNE_XPATH", "OVERALL_DISCARD_XPATH", "TEASER_DISCARD_XPATH", "PRECISION_DISCARD_XPATH",
        "DISCARD_IMAGE_ELEMENTS", "COMMENTS_DISCARD_XPATH", "AUTHOR_XPATHS", "AUTHOR_DISCARD_XPATHS",
        "CATEGORIES_XPATHS", "TAGS_XPATHS", "TITLE_XPATHS",
    )
    found = [(f"{name}[{i}]", xp) for name in names for i, xp in enumerate(getattr(x, name))]
    return found + [("BASIC_CLEAN_XPATH", BASIC_CLEAN_XPATH)]


_ESCAPES = {"\\": "\\\\", '"': '\\"', "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def rust_str(text: str) -> str:
    "A Rust string literal, ASCII only: controls, spaces other than U+0020 and non-ASCII as `\\u{..}`."
    body = "".join(_ESCAPES.get(ch) or (ch if " " <= ch <= "~" else f"\\u{{{ord(ch):X}}}") for ch in text)
    return f'"{body}"'


def main() -> None:
    from lxml.html import fromstring

    from parity import treedump
    from trafilatura.utils import HTML_PARSER

    sels = selectors()
    out = [
        "// @generated by tools/parity/parity/gen_xpath_cases.py. Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_xpath_cases import main; main()' > src/xpaths/cases.rs",
        "// Upstream's XPath selectors run on HTML snippets: the matches as positions in `root.iter()` order.",
        "",
        "/// One snippet, the upstream selectors' matches on it.",
        "pub(super) struct Case {",
        "    pub html: &'static str,",
        "    /// `treedump.dump_tree` of the lxml tree.",
        "    pub dump: &'static str,",
        "    /// Position in `root.iter()` order of the context node: 0 (the root) or the element carrying",
        "    /// `data-context`.",
        "    pub context: usize,",
        "    /// Selector name and matched positions in `root.iter()` order; only selectors with matches.",
        "    pub hits: &'static [(&'static str, &'static [usize])],",
        "}",
        "",
        "/// Every upstream selector name, `BODY_XPATH[0]` to `BASIC_CLEAN_XPATH`.",
        "#[rustfmt::skip]",
        "pub(super) const SELECTOR_NAMES: &[&str] = &[",
        *[f"    {rust_str(name)}," for name, _ in sels],
        "];",
        "",
        "#[rustfmt::skip]",
        "pub(super) const CASES: &[Case] = &[",
    ]
    # The same markup written for two selectors is one case that carries both expectations.
    unique: dict[str, Snippet] = {}
    for snippet in snippets():
        known = unique.setdefault(snippet.html, snippet)
        if known is not snippet:
            known.hit += snippet.hit
            known.miss += snippet.miss
    problems: list[str] = []
    for snippet in unique.values():
        root = fromstring(snippet.html, parser=HTML_PARSER)
        nodes = list(root.iter())
        position = {node: i for i, node in enumerate(nodes)}
        contexts = [node for node in nodes if "data-context" in node.attrib]
        assert len(contexts) <= 1, snippet.html
        context = contexts[0] if contexts else root
        hits = {name: [position[node] for node in xp(context)] for name, xp in sels}
        problems += [f"{name} must match {snippet.html!r}" for name in snippet.hit if not hits[name]]
        problems += [f"{name} must not match {snippet.html!r}: {hits[name]}" for name in snippet.miss if hits[name]]
        cells = ", ".join(f"({rust_str(name)}, &{found})" for name, found in hits.items() if found)
        out += [
            "    Case {",
            f"        html: {rust_str(snippet.html)},",
            f"        dump: {rust_str(treedump.dump_tree(root))},",
            f"        context: {position[context]},",
            f"        hits: &[{cells}],",
            "    },",
        ]
    assert not problems, chr(10).join(problems)
    out += ["];", ""]
    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    main()
