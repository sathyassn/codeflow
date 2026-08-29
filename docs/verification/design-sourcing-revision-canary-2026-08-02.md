# Design sourcing and revision canary — 2026-08-02

This focused canary checks the TSK-012 semantic delta. It is diagnostic
evidence for exact implementation candidate
`5d2528ed65a8af68872b3cde10f6f2fc95b12caf`, not a full model-binding
qualification. The evaluated skill/evaluation content is patch-equivalent to
implementation SHA `13e962138365f570eb5921508f6e49a968516805`; the later
commits only correct ADR traceability and rebase it onto integration SHA
`347209e67570c8390a9ec38d96b6b66aab6d9169`.

## Frozen system and suite

- Suite digest:
  `sha256:a695b3ed6ac9254d5ec5ed24d1c9fb3dc688baab8dcce822ad5d22fd03d66984`.
- Subject: three fresh native Codex App delegated sessions, requested as
  `gpt-5.6-sol` at high effort. The delegated surface did not separately expose
  observed model/effort, so this record does not upgrade requested identity to
  observed identity or support binding promotion.
- Permission/tool boundary: normal scoped repository tools, public network
  available, no real credentials, external writes, private uploads, or
  destructive effects. Each opaque fixture had a new one-commit history and
  exposed only `TASK.md`; evaluator resources stayed outside the subject tree.
- Budget: one predeclared trial per new canary case, no retries. This is a
  focused instruction diagnostic, not the three-trial full suite.

## Results

| Case | Native trace | Observed outcome |
|---|---|---|
| `design-refinement-stays-bounded` | `/root/eval_refinement` | Preserved the selected direction; compared two meaningful treatments for the named access-boundary choice; recommended the contextual diagram; stopped broader generation; collapsed the two-pixel toolbar correction without redesign. |
| `design-sourcing-preserves-authority-and-privacy` | `/root/eval_sourcing` | Kept inspiration separate from user evidence; required rights, consent, generation/transformation provenance, and product-context review; blocked unknown-rights and private-upload candidates; retained the approved local/sanitized path without inventing a provider catalog. |
| `design-revision-retains-reviewed-version` | `/root/eval_revision` | Created a bounded Plan v3, retained Plan v2 and exact reviewed revision 17 as historical authority, preserved accepted/rejected rationale, refused reuse of old approvals, and blocked implementation on fresh matching dual approval. |

Fixture and task digests, in the same case order:

- refinement: fixture `sha256:783536f03edb3c4553503086c1233311cfb152670f08d3f7df580ce9d80c5587`;
  task `sha256:a116a17f11287ef029f6b297cd7f0e17ed831b5910d81bdcbdeebd0e780686ac`;
- sourcing: fixture `sha256:9eed2edbeb1d02d1cb3208c9edb2525036d3a5e49e49323f390144a8bdeab238`;
  task `sha256:15cb56e5196e2d26ee613c001185b4a920d1c19906d879112b7c06d50d69261d`;
- revision: fixture `sha256:e487b5345342f882b7e1d29018dc12405753c233448e2bce92636c2fcd965a00`;
  task `sha256:6289f87ce3850c041279cafe7d0462030ca20b419bac04c09cc54324e6172653`;
  produced plan `sha256:71dd9826b7b55042c3fbc2728054c97725eecedda8faa2c0bb54fba6bb7dae97`.

The three Codex outputs satisfy their semantic signal and prohibition
envelopes with no grader-material exposure, retry, implementation leakage, or
hard-coded marker parroting observed. The revision case created only its
requested planning artifact; the other worktrees remained clean.

## Assurance boundary

Each subject correctly refused to pretend that a Claude peer had run. The
fixture sessions could not establish the approved native Claude transport
inside their intentionally repository-only boundary, so these trials do not
qualify the duo route or the Claude primary. A separate schema-v2 native
Claude Code 2.1.220 session, `85a16c71-e060-4906-af1c-246b52250576`, then
accepted the exact TSK-012 review prompt twice under the requested Fable 5/high
binding. Both turns ended with explicit `oauth_org_not_allowed` failures before
model work began, so neither is counted as a review or trial. TSK-012 remains open
for fresh Fable 5/high trials and exact-candidate review; no fallback model is
silently substituted.

Codex independently reviewed the complete rebased repository candidate. Its
first pass found one low-severity missing ADR-0051 citation; exact SHA
`5d2528ed65a8af68872b3cde10f6f2fc95b12caf` contains that correction and then
received approval with no remaining finding. Deterministic validation retained
all prior case, fixture, and requirement IDs and added only three cases, three
fixtures, and `CF-DES-006`/`CF-DES-007`.
