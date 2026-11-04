# Stage-colour verification

The governing normal-vision separation metric is Euclidean OKLab distance
×100 >= 15, as implemented by the kit's dataviz validator. The Visual and
docs Coordinator decided this metric for TSK-1122 at 22:18 on 2026-11-02.

| Check | Floor | Evidence still owed |
|---|---|---|
| `.st-line` and `.st-line-f` outline against the figure surface | sRGB contrast >= 3:1 | `floors.cjs` across every theme, mode and accent |
| `.st-line` and `.st-line-f` against `.st-main` | OKLab distance ×100 >= 15 | Mandatory pairs in `floors.cjs`, including the worst value |
| Shared line mix | One percentage in `_src/stage-colours.cjs` | `build.cjs` emits it; `floors.cjs` refuses a stale generated mix, then measures effective styles |
| Visible floor correction | Before/after capture of the same stage bar | `capture-changes.cjs` on the approved page and on the rebuilt page |

The visual and docs Coordinator, since retired, selected this floor of 15
and first chose a 75% mix. On the rebuilt page 75% fails separation (11.00
at worst), and no 5% step passes both floors on the fill alone. The coord/catalog
Coordinator then chose a 65% fill (15.54 at worst) with a 1 px accent outline
(5.14:1 at worst), and the 3:1 check reads the outline. These colours came
after the operator's approval, and the operator has not seen them.

`floors.cjs` reads every stage element's computed style in Chromium across
the twelve theme, mode and accent combinations. HTML `.st-line` uses its
background; SVG `.st-line` and `.st-line-f` use their fills. Each compares
against a `.st-main` resolved with the same tag and ancestors, and against
the same surface token used by the probe. The output records both colours
and the worst separation. Distinct computed pairs remain separate checks.
The SVG main fill follows the effective kit cascade; it is not assumed to
be accent. Latest run: 0 under the floor across 168 token pairs and 192
rendered stage pairs.
