# CodeFlow design system reference kit

The approved round 2 design as files that run. Open either reference page from
this folder and the whole system works with no build step: three skins, light
and dark, the Display panel, the panels, search, the figures and the present
Comment flow. Products do not import these files as a library; they carry the
values and the behaviour this kit demonstrates.

## Files

| File | What it is |
|---|---|
| `tokens.json` | The value source: six skin and mode pairs, one hex value per role |
| `tokens.css` | Custom properties only, generated from `tokens.json`: six `[data-cf-skin][data-cf-theme]` blocks plus the typeface, scale, type, shape and motion attributes |
| `chrome.css` | The `cf-` primitives: header, panels, sheets, grip, rows, pills, popover, dialog, tabs, table of contents, crumbs, provenance, preview card, footer cards, marker, float, composer, rail, toast. Tokens only, no literal colour |
| `figure.css` | Figure frame, legend, caption, table twin and the mark classes of the figure grammar. `--cf-fig-*` tokens only |
| `chrome.js` | The runtime: prepaint, Display panel, search dialog, panels with collapse, peek and resize, tabs, table of contents, preview card, footer cards and the Comment state machine. Plain ES2020, no framework |
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
  may be generated differently, but every role named in `tokens.json` carries
  the same value in the same skin and mode.
- `figure.css` is carried byte for byte. A figure drawn for one product renders
  the same in the other.
- `chrome.css` and `chrome.js` are the reference for what the chrome looks
  like and how it behaves. They are not a drop-in: a product reimplements the
  same primitives, states, motion and keyboard contract in its own stack and
  checks itself against these pages.

## Naming and persistence contract

| Surface | Names |
|---|---|
| Root attributes | `data-cf-skin` (`graphite`, `slate`, `sage`), `data-cf-theme` (`light`, `dark`), `data-cf-typeface` (`archivo`, `inter`, `plex`), `data-cf-scale` (`compact`, `default`, `large`) |
| State attributes, never persisted as preferences | `data-cf-nav` and `data-cf-toc` (`collapsed`), `data-cf-preload` (set until first paint), `data-cf-comment` (`armed`) |
| `localStorage` keys | `cf.skin`, `cf.theme` (`light`, `dark`, `system`), `cf.typeface`, `cf.scale`, `cf.nav.width` (220 to 360), `cf.nav.open`, `cf.toc.open`, `cf.nav.groups` (JSON of collapsed groups) |
| Custom properties | `--cf-<role>` for chrome, `--cf-fig-<name>` for figures |
| Chrome classes | `cf-` prefix throughout, for example `cf-header`, `cf-panel`, `cf-row`, `cf-pill`, `cf-marker` |
| Figure classes | `cf-fig` and `cf-fig-*` for the frame, `cf-legend` and `cf-key`, `cf-twin`, `cf-m-<mark>` for every mark of the grammar, `cf-f-*` for axes, rules, ticks and hit areas, `cf-t` for table twin text |
| Ids and hooks | `cf-main`, `cf-nav`, `cf-toc`, `cf-data` (page data JSON), `cf-present-document`; page hooks `data-cf-view`, `data-cf-open`, `data-cf-toggle`, `data-cf-document`, `data-cf-block-id`, `data-cf-anchor` |
| Runtime surface | `window.CF` with `CF.view`; the `cf:prefs` event fires on every Display change |

The theme preference `system` is stored, never written to the root: the root
always carries the resolved `light` or `dark`. The prepaint block at the top of
`chrome.js` applies every stored choice before first paint; a product inlines
it in `<head>`.

## Drift checks a consumer can run

| Check | How |
|---|---|
| Roles equal per role | Read `tokens.json`; for each pair and role, the product sheet's `--cf-<role>` value is identical |
| Figure sheet unchanged | `cmp` of `figure.css` against the product copy prints nothing |
| No literal colour in chrome or figure rules | The colour grep below prints nothing |
| No em or en dash in any kit file | The dash sweep below prints nothing |
| Pages stand alone | Every `href` and `src` in the reference pages names a file in this folder; no remote URL anywhere in the kit |
| Rendered checklist | Open both pages in every skin and mode at 1280 and 375: no console error, no horizontal scroll at 375, narrow figure variants under a 646 px container, reduced motion honoured, every control in the brief's section 9 list working |

```
cmp figure.css ../path/to/product/figure.css
grep -nE '#[0-9a-fA-F]{3,8}\b|rgb\(|hsl\(' chrome.css figure.css
perl -CSD -ne 'print "$ARGV\n" if /\x{2013}|\x{2014}/' *
```

## Writing rules

Every string on a page follows the written content policy: visuals first, a
lead sentence above each figure and the acting sentences below; short plain
sentences; bullets or a table where they carry facts better than prose; no em
or en dash, use a comma, colon, full stop or hyphen; titles name the subject in
words, never a bare identifier; sentence case except the uppercase mono
kicker; no slogans, contrast turns or rhetorical triplets.

## Where the doctrine lives

The rules this kit renders are in the same skill folder, one level up in
`resources/`: `utility-presentation-system.md` (the composition rule, altitude
contract, separation of planes) and `figure-grammar.md` (the nine families,
twelve rules, mark vocabulary and figure declaration). Read them before
authoring a page or a figure; read this kit to see them run.
