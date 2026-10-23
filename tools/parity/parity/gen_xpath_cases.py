"""Generates src/xpaths/cases.rs: upstream's XPath selectors run on HTML snippets, for the Rust tests.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_xpath_cases import main; main()' > src/xpaths/cases.rs

Each snippet is parsed with `parity.html5tree.parse_document`; every `XPath` of the content, comment and
discard lists and `BASIC_CLEAN_XPATH` runs on the context element, and the matches are written as
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


def tokens_snippets(selector: str, attr: str, tokens, values: dict, tag: str = "div") -> list[Snippet]:
    """One snippet per token: `values[token]` (a string or a tuple of them) in `attr`, every one matching its token."""
    assert set(values) == set(tokens), f"{selector} {attr}: token table out of date: {set(values) ^ set(tokens)}"
    out = []
    for token, found in values.items():
        for value in (found,) if isinstance(found, str) else found:
            assert re.search(token, value), f"{selector} {attr}: {value!r} does not match {token!r}"
            out.append(Snippet(page(element(tag, attr, value)), hit=(selector,)))
    return out


def plain(selector: str, attr: str, values, tag: str = "div", hit: bool = True) -> list[Snippet]:
    "Snippets for the values of an exact or substring test; `hit` says whether the selector must match."
    return [Snippet(page(element(tag, attr, value)), **{"hit" if hit else "miss": (selector,)}) for value in values]


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
        "^shar": ("shared", "share"),
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
        "navbox": "navbox",
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
        "share-": "share-this",
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
        "hide-print": "hide-print",
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
        Snippet(page("<svg><cite>a</cite><quote>b</quote></svg>"), miss=(cd(1),)),
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

    # ---- BASIC_CLEAN_XPATH
    out += [
        Snippet(page("<aside>a</aside><p>b</p>"), hit=(clean,)),
        Snippet(page("<footer>a</footer>"), hit=(clean,)),
        Snippet(page("<fencedframe>a</fencedframe>"), hit=(clean,)),
        Snippet(page("<script>var a = 1;</script>"), hit=(clean,)),
        Snippet("<html><head><script>var a;</script><style>p{}</style></head><body><p>x</p></body></html>", hit=(clean,)),
        Snippet(page("<style>p{}</style>"), hit=(clean,)),
        Snippet(page("<svg><title>t</title></svg>"), miss=(clean,)),
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

    # ---- namespaces: a name test sees HTML elements only, `*` sees every element
    out += [
        Snippet(page('<svg id="s"><title class="x">t</title><style>s</style><a id="a" class="comment">l</a></svg><p>b</p>'), miss=(clean,)),
        Snippet(page('<svg class="cookiebot"><title>t</title></svg>'), hit=(clean,)),
        Snippet(page('<svg><aside>a</aside><footer>b</footer><script>c</script><template></template></svg>'), miss=(clean,)),
        Snippet(page('<svg><a class="main">l</a><section class="post-body">s</section><article>a</article></svg>'), miss=(b(0), b(1), b(2), b(4))),
        Snippet(page('<svg><header>h</header><div class="x"></div></svg>'), miss=(pd(0),)),
        Snippet(page('<svg><section class="comments">s</section><details id="comments">d</details></svg>'), miss=(c(1), rc, rcl)),
        Snippet(page('<svg><section class="social">s</section><section class="teaser">t</section><section class="caption">c</section></svg>'),
                miss=(d0, t0, di)),
        Snippet(page('<svg class="infinitescroll"><aside class="infinitescroll">a</aside></svg>'), miss=(p0,)),
        Snippet(page('<math><mi class="message">x</mi></math>'), hit=(d1,)),
        Snippet(page('<svg><svg class="hide-me"><title>t</title></svg></svg>'), hit=(d1,), miss=(clean,)),
        Snippet(page('<svg><a>l</a></svg><a>m</a><svg><title>t</title></svg><title>u</title>'), miss=(clean,)),
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
        "DISCARD_IMAGE_ELEMENTS", "COMMENTS_DISCARD_XPATH",
    )
    found = [(f"{name}[{i}]", xp) for name in names for i, xp in enumerate(getattr(x, name))]
    return found + [("BASIC_CLEAN_XPATH", BASIC_CLEAN_XPATH)]


_ESCAPES = {"\\": "\\\\", '"': '\\"', "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def rust_str(text: str) -> str:
    "A Rust string literal, ASCII only: controls, spaces other than U+0020 and non-ASCII as `\\u{..}`."
    body = "".join(_ESCAPES.get(ch) or (ch if " " <= ch <= "~" else f"\\u{{{ord(ch):X}}}") for ch in text)
    return f'"{body}"'


def main() -> None:
    from parity import html5tree, treedump

    sels = selectors()
    out = [
        "// @generated by tools/parity/parity/gen_xpath_cases.py. Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_xpath_cases import main; main()' > src/xpaths/cases.rs",
        "// Upstream's XPath selectors run on HTML snippets: the matches as positions in `root.iter()` order.",
        "",
        "/// One snippet, the upstream selectors' matches on it.",
        "pub(super) struct Case {",
        "    pub html: &'static str,",
        "    /// `treedump.dump_tree` of the html5lib tree.",
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
        root = html5tree.parse_document(snippet.html)
        assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {snippet.html!r}"
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
