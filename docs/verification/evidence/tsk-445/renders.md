# Renders of the example and the specimens

The four `*-after-*.webp` images were made on 2026-11-04 from the sources at
`c5de5a9743e1bc12cce1c0ea7f60a044106421a3`. They replace the 2026-10-26
captures, which showed an earlier anchor line in the example and filled copy
regions in the structure specimen. The `*-before-*` and
`worked-example-ledger-*` images are unchanged.

| Images | Made from | Width |
|---|---|---|
| `example-after-light.webp`, `example-after-dark.webp` | `present-document.example.json` opened with the candidate binary (`present check`: 0 faults, 0 warnings), then `codeflow present export --mode light` and `--mode dark` | 1280 px, full page |
| `specimens-after-light.webp`, `specimens-after-dark.webp` | the nine `<figure>` blocks of `figure-grammar-specimens.md` with their section headings, styled by the portal starter's `utility-tokens.css`, `figure-roles.css` and `figure.css`, `data-theme` light and dark | 900 px, full page |

Each page was captured with Playwright 1.62.1 (Chromium) and encoded with
`cwebp -q 82`. Every figure in all four images was inspected and draws.
