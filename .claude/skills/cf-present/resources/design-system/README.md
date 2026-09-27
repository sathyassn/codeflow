# CodeFlow design system reference kit

This folder holds the CodeFlow design system as working files. Open either
reference page from this folder and the whole system works with no build step:
three skins, light and dark, the Display panel, the panels, search, the figures
and the present Comment flow. Products do not import these files as a library;
they carry the values and the behaviour this kit demonstrates.

## Files

| File | What it is |
|---|---|
| `tokens.json` | The value source: six skin and mode pairs, one hex value per role |
| `tokens.css` | Custom properties only: type, shape and motion on `:root`, the `data-cf-typeface` and `data-cf-scale` blocks, and six `[data-cf-skin][data-cf-theme]` blocks holding the `tokens.json` values plus `--cf-shadow` and `--cf-fig-na-alpha` |
| `chrome.css` | The `cf-` primitives: header, panels, sheets, grip, rows, pills, popover, dialog, tabs, table of contents, crumbs, provenance, preview card, footer cards, marker, float, composer, rail, toast. Tokens only, no literal colour |
| `figure.css` | Figure frame, legend, caption, table twin and the mark classes of the figure grammar. Colours and faces from `--cf-fig-*` tokens only; sizes and one transition also read `--cf-ui-scale` and `--cf-dur-select`, each with a fallback |
| `chrome.js` | The runtime: prepaint, Display panel, search dialog, panels with collapse, peek and resize, tabs, table of contents, preview card, footer cards, the Comment state machine and the toast. Reads optional page data from `#cf-data` and leaves three seams to the product: search, routing and review delivery. Plain ES2020, no framework |
| `portal.reference.html` | The docs portal shell around one full guide page, six figures across the three altitudes |
| `present.reference.html` | A present session reviewing one figure, Comment mode included |

## How a product consumes it

```
tokens.json ── equal per role ──▶ product token sheet
figure.css  ── byte for byte ──▶ product figure sheet
chrome.css  ── behaviour and look reference ──▶ product chrome (Preact, Starlight, or other)
chrome.js   ── behaviour reference ──▶ product runtime
```

- Token roles are equal per role. A product token sheet may add roles and
  may be generated differently, but every role in `tokens.json` carries the
  same value in the same skin and mode. So do the values `tokens.css` adds:
  `--cf-shadow` and `--cf-fig-na-alpha` in each pair block, and the `:root`
  aliases `--cf-fig-ground`, `--cf-fig-rule`, `--cf-fig-font` and
  `--cf-fig-mono`, which point at the same roles.
- `figure.css` is carried byte for byte. A figure drawn for one product renders
  the same in every product that carries the sheet.
- `chrome.css` and `chrome.js` are the reference for what the chrome looks
  like and how it behaves. They are not a drop-in: a product reimplements the
  same primitives, states, motion and keyboard contract in its own stack and
  checks itself against these pages.

## Naming and persistence contract

| Surface | Names |
|---|---|
| Root attributes | `data-cf-skin` (`graphite`, `slate`, `sage`), `data-cf-theme` (`light`, `dark`), `data-cf-typeface` (`archivo`, `inter`, `plex`), `data-cf-scale` (`compact`, `default`, `large`) |
| State attributes, set by the runtime and never stored themselves | `data-cf-nav` and `data-cf-toc` (`collapsed`, restored from `cf.nav.open` and `cf.toc.open`), `data-cf-preload` (set until the view has mounted and painted), `data-cf-mounted` (on the mounted view), `data-cf-comment` (`armed`) |
| `localStorage` keys | `cf.skin`, `cf.theme` (`light`, `dark`, `system`), `cf.typeface`, `cf.scale`, `cf.nav.width` (220 to 360), `cf.nav.open`, `cf.toc.open`, `cf.nav.groups` (JSON of collapsed groups) |
| Custom properties | `--cf-<role>` for chrome, `--cf-fig-<name>` for figures |
| Chrome classes | `cf-` prefix throughout, for example `cf-header`, `cf-panel`, `cf-row`, `cf-pill`, `cf-marker` |
| Figure classes | `cf-fig` and `cf-fig-*` for the frame, `cf-legend` and `cf-key`, `cf-fig-title` and `cf-fig-details`, `cf-m-<mark>` for every mark of the grammar (`cf-m-layer--remote` for layer-remote) and part classes such as `cf-m-cap` and `cf-m-trans-head`, `cf-f-*` for axes, rules, ticks and hit areas, `cf-t` for table twin text |
| Ids and hooks | `cf-main`, `cf-nav`, `cf-toc`, `cf-data` (page data JSON), `cf-present-document`; page hooks: every `data-cf-*` attribute `chrome.js` queries, among them `data-cf-view`, `data-cf-open`, `data-cf-toggle`, `data-cf-sheet`, `data-cf-peek`, `data-cf-section`, `data-cf-document`, `data-cf-block-id`, `data-cf-anchor` and `data-cf-tool`; the reference pages show each one in place |
| Runtime surface | `window.CF` with `CF.view`, `CF.mount(el)`, `CF.getPref(name)`, `CF.setPref(name, value)` and `CF.toast(msg)`; the `cf:prefs` event fires on every Display change and when the system scheme changes under `system` |

The theme preference `system` is stored, never written to the root: the root
always carries the resolved `light` or `dark`. The prepaint block at the top of
`chrome.js` applies every stored choice except `cf.nav.groups` before first
paint; the runtime applies the groups when the view mounts. A product inlines
the prepaint block in `<head>`.

## Drift checks a consumer can run

| Check | How |
|---|---|
| Roles equal per role | Read `tokens.json` and the added values in `tokens.css`; for each pair and role, the product sheet's `--cf-<role>` value is identical |
| Figure sheet unchanged | `cmp` of `figure.css` against the product copy prints nothing |
| No literal colour in chrome or figure rules | The colour grep below prints nothing; named colours are not caught by the grep |
| No em or en dash in any kit file | The dash sweep below prints nothing |
| Pages stand alone | Every `href` and `src` in the reference pages is an in-page `#` link or names a file in this folder; no remote URL anywhere in the kit |
| Rendered checklist | Open both pages in every skin and mode at 1280 and 375: no console error, no horizontal scroll at 375, narrow figure variants under a 646 px container, reduced motion honoured, and every control working: the Display pills with the choice remembered, search by click, `/` and Ctrl or Cmd K, nav groups, panel collapse, peek by hover, keyboard, `[` and `]`, the resize grip within 220 to 360, the altitude tabs, the table of contents, the preview card, the footer cards, and on the present page the topbar, the section route and the Comment flow from `C` through gestures, composer, markers, rail, verdict and submit, with Escape closing at each step |

```
cmp figure.css ../path/to/product/figure.css
grep -nEi '#[0-9a-f]{3,8}\b|\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(' chrome.css figure.css
perl -CSD -ne 'print "$ARGV\n" if /\x{2013}|\x{2014}/' *
```

## Writing rules

Every string on a page follows these rules:

- Visuals first: a lead sentence above each figure, the acting sentences below.
- Short plain sentences; bullets or a table where they carry facts better
  than prose.
- No em or en dash; use a comma, colon, full stop or hyphen.
- Titles name the subject in words, never a bare identifier.
- Sentence case, except the uppercase mono kicker.
- No slogans, no "not X but Y" turns, no rhetorical triplets.

## Where the doctrine lives

The rules this kit renders are in the same skill folder, one level up in
`resources/`: `utility-presentation-system.md` (the separation of planes, the
composition rule and the altitude grammar) and `figure-grammar.md` (the nine
families, twelve rules, altitude contract, mark vocabulary and figure
declaration). Read them before authoring a page or a figure; read this kit to
see them run.
