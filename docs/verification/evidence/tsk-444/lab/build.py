import json, re, html, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "rendered.json")))


def tag_entities(svg_html, mapping):
    """Add a class to the <g data-cf-entity="id"> wrappers named in mapping {id: cls}."""
    out = svg_html
    for ent, cls in mapping.items():
        out = re.sub(r'(<g data-state="[^"]+" id="[^"]+" data-cf-entity="%s")' % re.escape(ent), r'\1 class="%s"' % cls, out)
    return out


def legend_split(fig_html, state, entries):
    """Replace the legend entry for `state` with several entries (cls, label)."""
    m = re.search(r'<li data-state="%s"[^>]*>(.*?)</li>' % re.escape(state), fig_html)
    if not m:
        return fig_html
    whole = m.group(0)
    inner = m.group(1)
    key = re.match(r'(<svg class="cf-key".*?</svg>)', inner).group(1)
    new = "".join('<li data-state="%s" class="%s">%s%s</li>' % (state, cls, key, html.escape(label)) for cls, label in entries)
    return fig_html.replace(whole, new)


def legend_class(fig_html, state, cls):
    return re.sub(r'<li data-state="%s"' % re.escape(state), '<li data-state="%s" class="%s"' % (state, cls), fig_html)


def renumber(fig_html, n, title=None):
    out = re.sub(r'<span class="cf-fig-number">Figure \d+</span>', '<span class="cf-fig-number">Figure %d</span>' % n, fig_html)
    return out


def unique_ids(fig_html, suffix):
    return fig_html.replace('lab-', 'lab-%s-' % suffix)


def structure(opt):
    f = R["02-structure"]
    if opt == "c0":
        return f
    if opt == "c1":
        f = tag_entities(f, {"m1": "v-region", "r1": "v-region"})
        f = legend_class(f, "owner", "v-region")
        return f
    if opt == "c2":
        f = tag_entities(f, {"m1": "v-region", "r1": "v-region", "m2": "v-cat1", "r2": "v-cat1", "m3": "v-cat2", "r3": "v-cat2"})
        f = legend_class(f, "owner", "v-region")
        f = legend_split(f, "copy", [("v-cat1", "Live copy, never edited"), ("v-cat2", "Baseline copy, never edited")])
        return f
    if opt == "c3":
        f = tag_entities(f, {"m1": "v-region", "r1": "v-region", "m2": "v-mid", "r2": "v-mid", "m3": "v-faint", "r3": "v-faint"})
        f = legend_class(f, "owner", "v-region")
        f = legend_split(f, "copy", [("v-mid", "Live copy, never edited"), ("v-faint", "Baseline copy, never edited")])
        return f


def layering(opt):
    f = R["03-layering"]
    if opt == "c0":
        return f
    if opt == "c1":
        f = tag_entities(f, {"m4": "v-region", "l4": "v-region"})
        f = legend_class(f, "layer-remote", "v-region")
        return f
    if opt == "c2":
        f = tag_entities(f, {"m1": "v-cat1", "m2": "v-cat1", "l1": "v-cat1", "l2": "v-cat1", "m3": "v-cat2", "l3": "v-cat2", "m4": "v-region", "l4": "v-region"})
        f = legend_split(f, "layer", [("v-cat1", "Plane runs in the working tree, editable"), ("v-cat2", "Plane runs on the forge, required")])
        f = legend_class(f, "layer-remote", "v-region")
        return f
    if opt == "c3":
        f = tag_entities(f, {"m3": "v-mid", "l3": "v-mid", "m4": "v-region", "l4": "v-region"})
        f = legend_split(f, "layer", [("", "Plane covers this moment, local and editable"), ("v-mid", "Plane covers this moment, on the forge")])
        f = legend_class(f, "layer-remote", "v-region")
        return f


def sequence(opt):
    f = R["04-sequence"]
    if opt != "c2":
        return f
    for x, cls in (("120", "v-p0"), ("400", "v-p1"), ("640", "v-p2")):
        f = f.replace('<line class="cf-f-axis" x1="%s"' % x, '<line class="cf-f-axis %s" x1="%s"' % (cls, x))
        f = re.sub(r'<text class="cf-t cf-t--strong" x="%s" y="22" text-anchor="middle">' % x, '<text class="cf-t cf-t--strong %s" x="%s" y="22" text-anchor="middle">' % (cls, x), f)
    return f


def coverage(opt):
    f = R["06-coverage"]
    if opt == "c0":
        return f
    f = f.replace('<figure class="cf-fig"', '<figure class="cf-fig v-cov"')
    return f


def flow(opt):
    return R["01-flow"]


def fig_ids(figs):
    """Give each figure copy unique DOM ids and a running number."""
    out = []
    for i, (h, suffix) in enumerate(figs, 1):
        out.append(renumber(unique_ids(h, suffix), i))
    return out


OPT_CSS = open(os.path.join(HERE, "options.css")).read()

SAMPLE_HEAD_A = """
<p class="pg-kicker">Review document · TSK-444 · Revision 1</p>
<h1 class="pg-title">Presentation and portal design direction</h1>
<p class="pg-lede">Three decisions, each shown as rendered pages across the six skins: how a page opens, one type scale, and how figures use colour.</p>
<p class="pg-meta"><span>Task TSK-444</span><span>Epic EPC-022</span><span>2026-10-23</span></p>
"""

SAMPLE_HEAD_B = """
<p class="pg-kicker">Review document · TSK-444 · Revision 1</p>
<h1 class="pg-title">Presentation and portal design direction</h1>
<p class="pg-lede">Three decisions, each shown as rendered pages across the six skins: how a page opens, one type scale, and how figures use colour.</p>
<ul class="pg-anchor">
  <li><b>What</b><span>The design direction for cf-present documents and docs-portal pages, in three parts.</span></li>
  <li><b>Why</b><span>Figures render nearly grey, option titles outrank the section around them, and the summary never reaches the page.</span></li>
  <li><b>Stands</b><span>Options rendered; no token or style has changed yet.</span></li>
  <li><b>Decide</b><span>One candidate per part, or a direction of your own.</span></li>
</ul>
"""


def sample_section_a(fig_html):
    return """
<section class="pg-section">
  <h2 class="pg-h2">Where a skill lives</h2>
  <p class="pg-lead pg-lead--lede">A skill is edited in one place and copied to four; the figure shows which region is the owner.</p>
  %s
  <h3 class="pg-h3">What the parity test compares</h3>
  <p class="pg-p">The test reads every copy and compares its bytes with the source. A drifted copy fails the build, so the live copies are never edited by hand.</p>
  <div class="pg-cmp">
    <article><h4>Keep four copies</h4><p>The scaffold writes them; the test keeps them equal.</p></article>
    <article><h4>Symlink the copies</h4><p>Fewer bytes, but a harness that resolves links differently reads a different file.</p></article>
  </div>
  <div class="pg-dec"><h4>Which copy model holds</h4><p>Choose after the harness check.</p><ul class="pg-opts"><li>Keep four copies</li><li>Symlink the copies</li></ul></div>
</section>
""" % fig_html


def sample_section_b(fig_html):
    return """
<section class="pg-section">
  <div class="pg-plate">
    <p class="pg-kicker">Section 1</p>
    <h2 class="pg-h2">Where a skill lives</h2>
    <p class="pg-lead">A skill is edited in one place and copied to four; the figure shows which region is the owner.</p>
  </div>
  %s
  <h3 class="pg-h3">What the parity test compares</h3>
  <p class="pg-p">The test reads every copy and compares its bytes with the source. A drifted copy fails the build, so the live copies are never edited by hand.</p>
  <div class="pg-cmp">
    <article><h4>Keep four copies</h4><p>The scaffold writes them; the test keeps them equal.</p></article>
    <article><h4>Symlink the copies</h4><p>Fewer bytes, but a harness that resolves links differently reads a different file.</p></article>
  </div>
  <div class="pg-dec"><h4>Which copy model holds</h4><p>Choose after the harness check.</p><ul class="pg-opts"><li>Keep four copies</li><li>Symlink the copies</li></ul></div>
</section>
""" % fig_html


def sample_today(fig_html):
    return """
<div class="pg-topbar"><span class="k">Review document</span><strong>Presentation and portal design direction</strong><span class="r">Revision 1</span></div>
%s
<h2 class="pg-h2" style="margin-top:36px">Where a skill lives</h2>
<p class="pg-p">A skill is edited in one place and copied to four; the figure shows which region is the owner.</p>
<h3 class="pg-h3">What the parity test compares</h3>
<p class="pg-p">The test reads every copy and compares its bytes with the source.</p>
<div class="pg-cmp">
  <article><h4>Keep four copies</h4><p>The scaffold writes them; the test keeps them equal.</p></article>
  <article><h4>Symlink the copies</h4><p>Fewer bytes, but a harness that resolves links differently reads a different file.</p></article>
</div>
<div class="pg-dec"><h4>Which copy model holds</h4><p>Choose after the harness check.</p><ul class="pg-opts"><li>Keep four copies</li><li>Symlink the copies</li></ul></div>
""" % fig_html


def ladder(cls, sizes, bad=()):
    rows = [
        ("l-display", "Title", sizes[0]),
        ("l-lede", "Summary, the lede under the title", sizes[1]),
        ("l-h2", "Section heading", sizes[2]),
        ("l-h3", "Subsection heading", sizes[3]),
        ("l-title", "Block title: a comparison column, the decision, a figure line", sizes[4]),
        ("l-body", "Body text at the reading measure", sizes[5]),
        ("l-caption", "Caption and legend", sizes[6]),
    ]
    out = ['<div class="ladder %s">' % cls]
    for c, label, px in rows:
        out.append('<div><span class="%s%s">%s</span><span class="px">%s px</span></div>' % (c, " l-bad" if c in bad else "", label, px))
    out.append("</div>")
    return "".join(out)


def build_lab():
    f_c0 = fig_ids([(structure("c0"), "s0"), (layering("c0"), "l0"), (sequence("c0"), "q0"), (coverage("c0"), "v0")])
    f_c1 = fig_ids([(structure("c1"), "s1"), (layering("c1"), "l1"), (sequence("c1"), "q1"), (coverage("c1"), "v1")])
    f_c2 = fig_ids([(structure("c2"), "s2"), (layering("c2"), "l2"), (sequence("c2"), "q2"), (coverage("c2"), "v2")])
    f_c3 = fig_ids([(structure("c3"), "s3"), (layering("c3"), "l3"), (sequence("c3"), "q3"), (coverage("c3"), "v3")])
    page_fig_a = renumber(unique_ids(structure("c2"), "pa"), 1)
    page_fig_b = renumber(unique_ids(structure("c2"), "pb"), 1)
    page_fig_t = renumber(unique_ids(structure("c0"), "pt"), 1)

    def opt(title, tag, inner):
        return '<div class="lab-opt"><p class="lab-opt-t">%s<span class="tag">%s</span></p>%s</div>' % (title, tag, inner)

    parts = []
    parts.append('<section data-lab="anatomy"><h2 class="lab-h">1 · Page anatomy</h2>')
    parts.append('<div class="lab-grid">')
    parts.append(opt("Today", "present 3.1", '<div class="pg sc-today">%s</div>' % sample_today(page_fig_t)))
    parts.append(opt("A1 · Head and headings", "title, summary, meta; heading with its lede", '<div class="pg sc-wide opt-cat">%s%s</div>' % (SAMPLE_HEAD_A, sample_section_a(page_fig_a))))
    parts.append(opt("A2 · Head with anchor and section plates", "title, summary, anchor lines; numbered section plate", '<div class="pg sc-wide opt-cat">%s%s</div>' % (SAMPLE_HEAD_B, sample_section_b(page_fig_b))))
    parts.append('</div></section>')

    parts.append('<section data-lab="scale"><h2 class="lab-h">2 · Type scale</h2>')
    parts.append('<div class="lab-grid-3">')
    parts.append(opt("Today", "present 3.1 at 1280 px", ladder("sc-today", ["57.6", "16", "28.2", "17.9", "18.4", "16", "13"], bad=("l-title",))))
    parts.append(opt("S1 · Kit scale", "tokens.css as shipped; the portal uses it", ladder("sc-kit", ["30", "20", "22", "16.5", "14", "15", "13"])))
    parts.append(opt("S2 · Wider steps", "one ratio of about 1.33 per step", ladder("sc-wide", ["34", "19", "24", "18", "15", "15", "13"])))
    parts.append('</div></section>')

    for name, title, tag, figs, cls in (
        ("c0", "Today", "accent only on done, agent, merge, act, return, used", f_c0, ""),
        ("c1", "C1 · Accent governs", "tokens byte-identical; one governing region per figure in accent-soft with an accent edge; the covered cell in accent", f_c1, ""),
        ("c2", "C2 · Accent and two companions", "adds two category hues per skin and mode, each with a tint; the governing region stays accent", f_c2, "opt-cat"),
        ("c3", "C3 · Tonal ramp", "tokens byte-identical; two derived tints of the accent grade the regions below the governing one", f_c3, ""),
    ):
        parts.append('<section data-lab="%s"><h2 class="lab-h">3 · Figure colour</h2>%s</section>' % (name, opt(title, tag, '<div class="lab-grid %s">%s</div>' % (cls, "".join('<div>%s</div>' % f for f in figs)))))

    sw = json.load(open(os.path.join(HERE, "..", "cat-tokens.json")))
    swatch_rows = []
    for pair, row in sw.items():
        cells = "".join('<div class="sw"><i style="background:%s;border-color:%s"></i><span>%s<br><span class="n">%s</span></span></div>' % (row[k + "-soft"], row[k], k, row[k] + " / " + row[k + "-soft"]) for k in ("fig-cat1", "fig-cat2"))
        swatch_rows.append('<p class="lab-cap">%s</p><div class="swatches">%s</div>' % (pair, cells))
    parts.append('<section data-lab="swatches">%s</section>' % opt("C2 values proposed for tokens.json", "ink and tint per skin and mode, computed in OKLCH from the accent", "".join(swatch_rows)))

    doc = """<!doctype html>
<html lang="en" data-cf-skin="graphite" data-cf-theme="light" data-cf-typeface="inter" data-cf-scale="default">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>TSK-444 design options lab</title>
<link rel="stylesheet" href="tokens.css"><link rel="stylesheet" href="chrome.css"><link rel="stylesheet" href="figure.css"><link rel="stylesheet" href="options.css">
<script>(function(){var q=new URLSearchParams(location.search);var r=document.documentElement;if(q.get('skin'))r.setAttribute('data-cf-skin',q.get('skin'));if(q.get('mode'))r.setAttribute('data-cf-theme',q.get('mode'));if(q.get('scale'))r.setAttribute('data-cf-scale',q.get('scale'));var only=q.get('only');if(only){document.addEventListener('DOMContentLoaded',function(){document.querySelectorAll('section[data-lab]').forEach(function(s){if(s.dataset.lab!==only)s.remove();});});}})();</script>
</head><body><main class="lab-sheet cf-prose">%s</main></body></html>""" % "".join(parts)
    open(os.path.join(HERE, "lab.html"), "w").write(doc)
    print("lab.html", len(doc))


if __name__ == "__main__":
    build_lab()
