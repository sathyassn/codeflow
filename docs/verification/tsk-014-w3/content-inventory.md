# Content inventory

Every fact on this board, with the repository source it comes from. Nothing here is
invented, and `tools/verify.mjs` binds each row to its source on every run.

> This file names the subjects and their sources. It does not contain the expected
> answers — those are in `answer-key.md`, which an observer must not read. Reading *this*
> file does not disqualify an observer from G1–G8, but it does give away the case content,
> so a blind run should not open it either.

## P1 — the EPC-005 plan graph

| What | Source | How it is bound |
|---|---|---|
| Ten task records: id, title, status, `depends_on`, `specs`, `integration_target` | `project-management/tasks/TSK-005.md` … `TSK-014.md` | frontmatter re-parsed and compared field by field, including titles |
| Waves, routes, room, the unbroken run | derived in `shared/plan-model.js` | recomputed independently in `tools/verify.mjs` from the same frontmatter |
| Short labels on task nodes | authored | every word must appear in that task's real frontmatter title |

The embedded dataset is byte-identical across both candidates and the baseline.

## P2 — the TSK-007 qualification evidence

| What | Source | How it is bound |
|---|---|---|
| Eight claims × five lanes, with per-cell state | `docs/verification/tsk-007-presentation/README.md` | every material cell carries a quotation checked verbatim |
| The exact candidate `826715510440df53fc999e68d77429d7b49a6d55` | same | must appear in the record and in the registry |
| Provenance chains and how far each reaches | same | quoted; the two claims with no recorded chain derive their reach from their own cells |
| Five absence causes and their sentences | same | each cause quoted verbatim; every absence classified exactly once; an unclassified absence fails the run |
| Per-claim stop causes | same | required for every claim short of acceptance, forbidden for every claim that reaches it |

## P3 — the qualification harness review

| What | Source | How it is bound |
|---|---|---|
| Eight rounds: id, sha, subject, date | `git log -1` over each commit | abbreviated sha, subject and date compared exactly |
| Quoted diff lines per region | `git show <sha> -- :(literal,top)<path>` | every line matched character for character, including its leading `+`, `-` or space |
| Seven findings and their per-round states | `project-management/tasks/TSK-014.md` | closed state vocabulary; every quotation verbatim |
| The re-opening that came from off the round axis | same | must name an origin with a label, detail and quotation, and the origin's identifying tokens must appear in both |
| Gate context sentences | same | verbatim |

## D1 — the six layers

| What | Source | How it is bound |
|---|---|---|
| The organisation table: layer, what it asks, where it lives, how often it changes | `AGENTS.md` lines 21–28 | the table is re-parsed with a regex and compared cell by cell; each layer's cadence must equal its row index |
| Seven write-authority classes and their rules | `AGENTS.md` | each carries a verbatim quotation from the sentence that states it |
| Declared homes | this checkout | every `file` or `dir` home must exist and be of the declared kind; every `command` or `local-state` home must **not** exist as a path and must quote why |
| The traceability spine | `AGENTS.md` lines 30–31 | quoted verbatim; every spine node's label must appear in the quoted sentence |

The split-authority artefact is `AGENTS.md` itself: project-owned outside the codeflow
markers, tool-maintained between them.

## D2 — SPC-004 as a record page

| What | Source | How it is bound |
|---|---|---|
| Subject record frontmatter | `project-management/specs/SPC-004.md` | title and every declared frontmatter field compared |
| Four decisions and one epic/two tasks | `docs/decisions/ADR-0049…0052`, `project-management/…` | same |
| The record's own shape: six `##` headings and eleven numbered behaviours | `project-management/specs/SPC-004.md` | headings re-parsed and matched both ways; the behaviour count re-counted |
| Five claims, each with its section, its decision links and its evidence state | the records above plus `docs/verification/tsk-007-presentation/README.md` | every claim, link, scope and evidence quotation verbatim |
| The partial-supersession sentence | `docs/decisions/ADR-0052-…md` | verbatim; the decisions it leaves in force must really be `accepted` with `superseded_by: null` |
| That the board's own gates are not run | `observer-state.md` in this directory | the one study-owned source, named explicitly so a repository claim can never be satisfied by a file this study wrote |

## Carrier facts

| What | Source | How it is bound |
|---|---|---|
| Inline CSS in the sandboxed HTML block may not contain `@` | `crates/codeflow-present/src/safe_html.rs` | verbatim |
| The sandbox CSP forbids script | `crates/codeflow-present/src/service.rs` | verbatim |
| The review stylesheet constrains only the frame's inline size | `crates/codeflow-present/web/src/styles.css` | verbatim |
| The escape block is a bounded v1 concession | `project-management/specs/SPC-004.md` | verbatim |
| The block catalogue itself | `assets/base/present/schemas/document-v1.schema.json` | every block type a candidate names must exist in `$defs` |
| Raw HTML in a portal source is escaped | `docs-portal/scripts/lib.mjs` | verbatim |
| No MDX integration is installed | `docs-portal/package.json` | structural: `@astrojs/mdx` must be absent from both dependency sets |
| The portal's composition lives in a real stylesheet | `docs-portal/astro.config.mjs` | verbatim |

## What is authored rather than sourced

Honest list of everything on the board that is this study's own words:

- the page titles, ledes and legends;
- the short labels on P1 task nodes (constrained to be word-subsets of real titles);
- the five absence-cause **labels** in P2 (`no environment to run it in`, and so on) — the
  classification is authored, the sentence justifying each one is the record's;
- the D1 band names (`you write it`, `you write it, in one flow`, `it refuses your edit`)
  and the plain-English gloss of each authority rule;
- the mapping from each D2 claim to the section of SPC-004 it sits in;
- the accessible `title` and `desc` on every figure.

Everything else is a repository fact, and every one of those is checked.
