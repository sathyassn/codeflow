# Stage-colour verification

The governing normal-vision separation metric is Euclidean OKLab distance
×100 >= 15, as implemented by the kit's dataviz validator. The Visual and
docs Coordinator decided this metric for TSK-1122 at 22:18 on 2026-11-02.

| Check | Floor | Evidence still owed |
|---|---|---|
| `.st-line` and `.st-line-f` against the figure surface | sRGB contrast >= 3:1 | Rendered probe and `floors.cjs` across every theme, mode and accent |
| `.st-line` and `.st-line-f` against `.st-main` | OKLab distance ×100 >= 15 | Mandatory pairs in `floors.cjs`, including the worst value |
| Shared line mix | One percentage in `_src/stage-colours.cjs` | `build.cjs` and `floors.cjs` import the same constant |
| Visible floor correction | Before/after capture of the same stage bar | Capture the original page before rebuilding; capture the rebuilt page after a feasible mix is selected |

The Coordinator selected 75% after the probe at `53f298d34`. The shared
constant is set to 75; rebuilt-page verification remains pending. Both
floors must pass; report any failure to the Coordinator before further
design changes. Generated `prototype.html` and `_src/kit/index.html` await
a builder run, including the `.st-line-f` correction.
