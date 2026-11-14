"""Build the TSK-444 cf-present option document from the lab pieces.

The present html sanitizer refuses `cf-` classes, `cf-` ids and `data-cf-*`
attributes inside an html block, and the scoped stylesheet refuses at-rules,
so every figure copy is renamed to an `fg-` prefix, loses its narrow
composition and carries a flat copy of the figure sheet plus the option rules.
"""
import json, os, re, html
import build as B

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "tsk-444-design-direction.json")


def flat_css(css):
    """Drop @-rules (with their blocks) and comments; keep flat rules only."""
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    out, depth, i = [], 0, 0
    while i < len(css):
        if css.startswith("@", i):
            j = css.find("{", i)
            k, d = j + 1, 1
            while k < len(css) and d:
                d += {"{": 1, "}": -1}.get(css[k], 0)
                k += 1
            i = k
            continue
        out.append(css[i])
        i += 1
    return "".join(out)


FIGURE_CSS = open(os.path.join(HERE, "figure.css")).read()
OPTIONS_CSS = open(os.path.join(HERE, "options.css")).read()


def rename_classes(text):
    """Rename cf- classes inside class attributes only; content text keeps its words."""
    def one(m):
        v = m.group(1).replace("cf-fig-svg--narrow", "fg-narrow")
        return 'class="%s"' % re.sub(r"(?<![-\w])cf-(?=[a-z])", "fg-", v)
    return re.sub(r'class="([^"]*)"', one, text)


def rename(text):
    text = text.replace("cf-fig-svg--narrow", "fg-narrow")
    text = re.sub(r"(?<![-\w])cf-(?=[a-z])", "fg-", text)
    return text


def present_css():
    css = flat_css(FIGURE_CSS) + "\n" + flat_css(OPTIONS_CSS)
    css = rename(css)
    # custom properties must keep their --cf- names
    css = css.replace("--fg-", "--cf-")
    css = css.replace(".fg-fig-svg--wide { display: none; max-width: 720px; }", ".fg-fig-svg--wide { display: block; max-width: 720px; }")
    css = css.replace(".fg-narrow { display: block; max-width: 400px; }", ".fg-narrow { display: none; }")
    css = css.replace(".fg-legend li[data-fg-wide] { display: none; }", "")
    css += "\n.lab-grid { grid-template-columns: 1fr; gap: 20px; } .lab-grid-3 { grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; } .lab-opt { padding: 12px 12px 4px; } .ladder > div { grid-template-columns: minmax(0, 1fr) 6ch; gap: 8px; }"
    return css


def present_figure(fig_html):
    """A rendered figure, made admissible to an html block."""
    h = re.sub(r'<svg class="cf-fig-svg cf-fig-svg--narrow".*?</svg>', "", fig_html, flags=re.S)
    h = re.sub(r'<g data-state="([^"]+)" id="lab-(?:[a-z0-9]+)-([a-z0-9-]+)-wide-([a-z0-9-]+)" data-cf-entity="[^"]+" data-cf-entity-label="([^"]*)"',
               lambda m: '<g data-state="%s" data-cf-group="%s-%s" data-cf-label="%s"' % (m.group(1), m.group(2), m.group(3), m.group(4) or m.group(1)), h)
    h = re.sub(r' data-cf-(?!group|label)[a-z-]+="[^"]*"', "", h)
    h = re.sub(r' data-cf-[a-z-]+(?=[ >])', "", h)
    h = re.sub(r' aria-labelledby="[^"]*"', "", h)
    h = re.sub(r' id="lab-[^"]*"', "", h)
    h = h.replace('role="img"', "")
    h = rename_classes(h)
    return h


def block_html(bid, title, caption, inner, cls=""):
    return {"type": "html", "id": bid, "title": title, "caption": caption,
            "html": "<style>%s</style><div class=\"sheet %s\">%s</div>" % (present_css(), cls, inner)}


def figure_block(bid, fixture):
    decl = json.load(open(os.path.join(HERE, fixture + ".json")))
    return {"type": "figure", "id": bid, "declaration": decl}


R = B.R
SAMPLE_CSS = ".sheet .pg { max-width: 860px; margin: 0; }"


def build():
    pf = lambda h, suffix: present_figure(B.renumber(B.unique_ids(h, suffix), 1))
    colour_set = lambda opt, sfx: "".join("<div>%s</div>" % present_figure(B.renumber(B.unique_ids(h, sfx + str(i)), i)) for i, h in enumerate([B.structure(opt), B.layering(opt), B.sequence(opt), B.coverage(opt)], 1))

    blocks = []
    blocks.append({"type": "narrative", "id": "anchor", "markdown": (
        "**What this is.** The design direction for cf-present documents and docs-portal pages, in three parts: how a page opens, one type scale, and how figures use colour. Each part shows rendered candidates; switch skin and appearance in Settings to see them in all six pairs.\n\n"
        "**Why now.** Figures render nearly grey because the grammar reserves accent for six marks; comparison and decision titles render larger than the subsection around them; the document summary never reaches the page.\n\n"
        "**Where it stands.** Candidates only. No token, style or test has changed; the tokens stay byte-identical until you choose.\n\n"
        "**What you decide.** One candidate per part below, or a direction of your own in the notes. The recommended option is marked on each question.")})

    # Part 1: page anatomy
    blocks.append({"type": "narrative", "id": "anatomy-lead", "markdown": "## Part 1: how a page opens\n\nThe page head and every section lead with context. Today the title lives only in the top bar and the summary never shows. Both candidates put the title, the summary and a meta line at the top of the document column and give every section heading a lead; they differ in how a section announces itself."})
    blocks.append(block_html("anatomy-today", "Today: present 3.1", "The title sits in the top bar only; the first block is the first thing on the page.",
                             "<style>%s</style><div class=\"pg sc-today\">%s</div>" % (SAMPLE_CSS, B.sample_today(pf(B.structure("c0"), "pt")))))
    blocks.append(block_html("anatomy-a1", "A1: head and headings", "Kicker, title, summary and meta at the top; a section is a heading with its lede paragraph; no new block.",
                             "<style>%s</style><div class=\"pg sc-wide opt-cat\">%s%s</div>" % (SAMPLE_CSS, B.SAMPLE_HEAD_A, B.sample_section_a(pf(B.structure("c2"), "pa")))))
    blocks.append(block_html("anatomy-a2", "A2: head with anchor lines and section plates", "The head adds four anchor lines; a section opens with a numbered plate: kicker, title, lead and a hairline.",
                             "<style>%s</style><div class=\"pg sc-wide opt-cat\">%s%s</div>" % (SAMPLE_CSS, B.SAMPLE_HEAD_B, B.sample_section_b(pf(B.structure("c2"), "pb")))))
    blocks.append({"type": "decision", "id": "decide-anatomy", "title": "Which page anatomy", "markdown": "A1 renders the existing title, summary and provenance fields and styles the paragraph after a heading as its lead; no schema change. A2 needs anchor lines and a section block in the schema, so authors write them and the route lists them.", "options": [
        {"value": "a1", "label": "A1: head and headings", "recommended": True},
        {"value": "a2", "label": "A2: head with anchor lines and section plates"}]})

    # Part 2: type scale
    blocks.append({"type": "narrative", "id": "scale-lead", "markdown": "## Part 2: one type scale\n\nThe document title must be larger than a section, a section larger than a subsection, and a subsection larger than any block title. Today a comparison column title and the decision title render at 18.4 px against a 17.9 px subsection. Both candidates fix the order; they differ in how far apart the steps sit."})
    blocks.append(block_html("scale-ladders", "Three scales, measured in px at the default size", "S1 is the kit scale the portal already uses; S2 widens every step to about 1.33 so the order survives the compact size and reads at a glance.",
                             '<div class="lab-grid-3">%s%s%s</div>' % (
                                 '<div class="lab-opt"><p class="lab-opt-t">Today<span class="tag">present 3.1 at 1280 px</span></p>%s</div>' % B.ladder("sc-today", ["57.6", "16", "28.2", "17.9", "18.4", "16", "13"], bad=("l-title",)),
                                 '<div class="lab-opt"><p class="lab-opt-t">S1: kit scale<span class="tag">tokens.css as shipped</span></p>%s</div>' % B.ladder("sc-kit", ["30", "20", "22", "16.5", "14", "15", "13"]),
                                 '<div class="lab-opt"><p class="lab-opt-t">S2: wider steps<span class="tag">about 1.33 per step</span></p>%s</div>' % B.ladder("sc-wide", ["34", "19", "24", "18", "15", "15", "13"]))))
    blocks.append({"type": "table", "id": "scale-table", "title": "The three scales side by side", "caption": "Sizes in px at the default size; the compact size multiplies by 0.94 and the large by 1.12, and the floors (body 15, caption 13, micro 12.5) never drop.", "columns": ["Role", "Today (present)", "S1 kit scale", "S2 wider steps"], "rows": [
        ["Document title", "not on the page (57.6 if it were)", "30", "34"],
        ["Summary (lede)", "not on the page", "20", "19"],
        ["Section heading (##)", "23 to 32, by viewport", "22", "24"],
        ["Subsection heading (###)", "17.9", "16.5", "18"],
        ["Block title (comparison column, decision, figure line)", "18.4", "14", "15"],
        ["Body", "16", "15", "15"],
        ["Caption, legend", "13", "13", "13"]]})
    blocks.append({"type": "decision", "id": "decide-scale", "title": "Which type scale", "markdown": "S1 keeps tokens.css byte-identical and the portal unchanged; present adopts it. S2 changes the shared type values in tokens.css for both surfaces; the portal gets the same wider steps.", "options": [
        {"value": "s2", "label": "S2: wider steps (34 / 24 / 18 / 15)", "recommended": True},
        {"value": "s1", "label": "S1: kit scale as shipped (30 / 22 / 16.5 / 14)"}]})

    # Part 3: figure colour
    blocks.append({"type": "narrative", "id": "colour-lead", "markdown": "## Part 3: how figures use colour\n\nThe governing path of every figure is drawn in accent; everything else is grey for context. Today only six marks carry accent, so a structure figure, a layering figure with no act marker and every coverage figure render grey. The four figures below are the same in every candidate; only their colour rules change. The first four are drawn by the runtime from their declarations as the grammar draws them today."})
    blocks.append(figure_block("today-structure", "02-structure"))
    blocks.append(figure_block("today-layering", "03-layering"))
    blocks.append(figure_block("today-sequence", "04-sequence"))
    blocks.append(figure_block("today-coverage", "06-coverage"))
    blocks.append(block_html("colour-c1", "C1: accent governs", "Tokens stay byte-identical; one governing region per figure takes the accent-soft field with an accent edge, and the covered cell takes the accent.",
                             '<div class="lab-grid">%s</div>' % colour_set("c1", "c1")))
    blocks.append(block_html("colour-c2", "C2: accent and two companions", "Two category hues per skin and mode, each with a tint, for peers that must be told apart; the governing region stays accent and every category mark keeps its dash, width or cap.",
                             '<div class="lab-grid opt-cat">%s</div>' % colour_set("c2", "c2"), cls="opt-cat"))
    blocks.append(block_html("colour-c3", "C3: tonal ramp", "Tokens stay byte-identical; two derived tints of the accent grade the regions below the governing one, so hierarchy reads in one hue.",
                             '<div class="lab-grid">%s</div>' % colour_set("c3", "c3")))
    sw = json.load(open(os.path.join(HERE, "..", "cat-tokens.json")))
    rows = []
    import sys
    sys.path.insert(0, os.path.join(HERE, ".."))
    from color import contrast
    tokens = json.load(open(os.path.join(HERE, "tokens.json")))
    for pair, row in sw.items():
        t = tokens[pair]
        for k in ("fig-cat1", "fig-cat2"):
            ink, soft = row[k], row[k + "-soft"]
            rows.append([pair, k, ink, soft, "%.2f" % contrast(ink, t["surface"]), "%.2f" % contrast(ink, t["canvas"]), "%.2f" % contrast(t["fig-line"], soft), "%.2f" % contrast(ink, soft)])
    blocks.append({"type": "table", "id": "c2-contrast", "title": "C2 values proposed for tokens.json and their contrast", "caption": "Each ink clears 4.5 to 1 on the figure ground and the canvas; the figure ink clears 4.5 to 1 on every tint; the floors the design system sets for accent and text.", "columns": ["Skin and mode", "Role", "Ink", "Tint", "Ink on ground", "Ink on canvas", "Figure ink on tint", "Ink on its tint"], "rows": rows})
    blocks.append({"type": "decision", "id": "decide-colour", "title": "Which figure colour", "markdown": "C1 and C3 add no hue and keep tokens.json byte-identical; they add a governing-region mark and, for C3, two tints derived in figure.css. C2 adds four roles per skin and mode (two inks, two tints) to tokens.json, tokens.css, figure.css and the portal token sheet, with the drift test extended; category hue is used only for peers the subject names, never for the governing path, and never alone.", "options": [
        {"value": "c2", "label": "C2: accent and two companions", "recommended": True},
        {"value": "c1", "label": "C1: accent governs, no new hue"},
        {"value": "c3", "label": "C3: tonal ramp, no new hue"}]})

    # Part 4: the section block question
    blocks.append({"type": "narrative", "id": "section-lead", "markdown": "## Part 4: a dedicated section block\n\nThe task asks whether a `section` block (title, lead) is worth adding beyond the `## ` heading pattern. Evidence from the candidates: A1 gets heading, lead and a route entry from the Markdown a narrative block already carries, styled by one rule (the paragraph after a heading is its lead). A2's plate needs a block so the number and the lead are data, which costs a schema change, validation, docs and a migration for every existing document."})
    blocks.append({"type": "decision", "id": "decide-section", "title": "Add a section block", "markdown": "The recommendation follows the anatomy choice: no block with A1, a block with A2.", "options": [
        {"value": "no", "label": "No: headings in narrative blocks, lead styled by rule", "recommended": True},
        {"value": "yes", "label": "Yes: a section block with title and lead"}]})
    blocks.append({"type": "feedback_prompt", "id": "ask", "prompt": "Answer the four questions, or mark on any candidate what should change. Your answers and notes become the DESIGN_INTENT sign-off; nothing is styled or tokened before that."})

    doc = {"schema_version": 2, "title": "Presentation and portal design direction",
           "summary": "Three design decisions shown as rendered candidates in every skin and mode: page anatomy, one type scale, figure colour; choose one per part.",
           "language": "en", "provenance": {"task_id": "TSK-444"}, "blocks": blocks}
    json.dump(doc, open(OUT, "w"), indent=1)
    print(OUT, os.path.getsize(OUT))


if __name__ == "__main__":
    build()
