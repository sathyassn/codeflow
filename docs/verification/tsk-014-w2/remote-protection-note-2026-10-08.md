# Note on the remote protection sentences, 2026-10-08

Several captured pages of this study say that this repository cannot have
remote branch protection and does not pursue it. Those sentences describe the
repository as it was when the study was captured: private, on GitHub Free,
where `codeflow remote protect` returned HTTP 403. They are not the current
rule. The repository is now public, `main` is protected, and its ruleset
requires the checks on a branch that is up to date with `main`; see
`docs/decisions/ADR-0081-main-requires-branches-to-be-up-to-date-before-m.md`.

The sentences are:

- `content-inventory.md:175`, the "this repo's real perimeter" row
- `answer-key.md:174-175`
- `baselines/d1/chat.md:45`
- `baselines/d1/markdown.md:52`
- `baselines/d1/baseline.html:76`
- `shared/repo-entry.js:64`

They stay byte-identical because `SHA256SUMS` pins every one of those files
and `tools/verify.mjs` fails on any drift, so this note sits beside them
instead of inside them. `tools/verify.mjs` inventories only the files it
names and the `baselines`, `cases`, `checks`, `renders`, `shared` and `tools`
folders, so this file does not change that inventory.
