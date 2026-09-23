# Operating doctrine body verification

This records the integration check for the operating doctrine epic (EPC-017)
before it goes to `main`. The four tasks landed on
`integration/EPC-017-operating-doctrine`, a combined review found three
editorial defects, and those were fixed on the same branch. The head below
passed every check named in TSK-069.

```text
TSK-066 written content policy  (#521) --+
TSK-065 pull request follow-up  (#523) --+--> integration head --> checks --> PR to main
TSK-067 verified links          (#524) --+       e6121491e
TSK-068 blind evaluation cases  (#527) --+
combined editorial fixes        (#529) --+
```

## Landings

| Pull request | Task | Head | Merge |
|---|---|---|---|
| #521 | TSK-066 written content policy, ADR-0067 | `77545df9e` | `90e1944f4` |
| #523 | TSK-065 pull request follow-up | `4df3424ab` | `c7e16d336` |
| #524 | TSK-067 verified links | `c2d4487e4` | `e1b9c3053` |
| #527 | TSK-068 blind evaluation cases | `558da2b2f` | `9af5c6d02` |
| #529 | combined editorial fixes | `d2cbedae1` | `e6121491e` |

Each task had its own full gate and Grok review before it landed; the task
records hold those.

## Checks on the integration head `e6121491e`

```
rust-format: ok
rust-workspace: ok (211 s)
rust-clippy: ok
rustdoc: ok
rust-coverage: ok (241 s)
release-automation: ok
gate-parity: ok
model-eval-kit: ok
skill-triggers: ok
herdr-delivery: ok
docs-validation: ok
cf-present-qualification: ok (215 s)
docs-portal: ok
test gate: passed (13 target(s))
validate: .codeflow/policy.json clean
validate: 70 record(s) clean
validate --docs: doc graph clean
codeflow ci: 1 warning(s) only
ok    managed-drift: no managed-region drift (codeflow blocks match the record)
```

The one `codeflow ci` warning is `git.breaking_watch_paths` on `c0a798b5`,
which adds `policy_characters` to `assets/base/policy.json`. Judgment: not a
breaking change. Version 3.0.0 is unpublished (no `v3` tag), the key reaches
only new scaffolds, a consumer's own `policy.json` is user-owned and left
alone, and a binary that knows the check fills a missing key with `block`.
Copying the new key into a checkout that still runs an older binary would
fail its schema check; that is an upgrade-order note for the release notes,
not a break of a published contract.

## Update on a scaffolded sample

A sample was scaffolded with the installed `codeflow` 3.0.0 at the full tier
and updated with the binary built from `e6121491e`:

```
162 created
20 added, 49 changed, 1 merged, 1 keys-added, 7 skipped, 104 unchanged
```

| Carried file (both `.claude` and `.agents`) | Equal to source |
|---|---|
| `cf-ship/SKILL.md` | yes |
| `cf-ship/references/pr-evidence.md` | yes |
| `cf-editorial-review/references/editorial-smells.md` | yes |
| `cf-method/references/workflow-lifecycle.md` | yes |
| `cf-evaluate-model/resources/cases.json` | yes |
| `cf-evaluate-model/resources/packs.json` | yes |

The sample's `AGENTS.md` carries the written content policy line, and its
new `policy.json` carries `"policy_characters": "block"`.

## Reviews

| Review | Scope | Verdict |
|---|---|---|
| Grok, combined body | `origin/main...9af5c6d02`: cross-file contradictions, mirrors, hashes, the new policy key against installed hooks, epic criteria | approved; lesser notes applied in #529 |
| Fable, editorial round 1 | all prose in the combined diff | changes requested: three blocking findings |
| Fable, editorial round 2 | the fixes in #529 | approved |
| Codex | none | not run: the seat is unavailable until 2026-09-27; recorded as reduced assurance |

Fable's findings N6 and N8 were left by change control: ADR-0067 is accepted
and append-only, and the completed task records keep their text; their
closeouts now point here.

## Not verified

- Hosted CI: GitHub Actions refuses every job for billing, so no hosted
  check ran on any of these pull requests. The local full gate is the
  evidence.
- Native model trials of the new evaluation cases. The cases grade recorded
  answers; registering them proves nothing about live behaviour.
- The upgrade-order case above on a real consumer.
