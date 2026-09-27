# Release notes and release checks: review record

TSK-106 fixes how the 3.0.0 notes render, gives each pending entry one
identity, adds local release checks and a typed repair for a broken base,
and moves CodeFlow's own release jobs out of the CI file adopters receive.
This record holds the evidence that tests alone do not carry: the two reads
of the rendered notes, the entry list against the breaking commits, the
mutation run on the typed repair and the architecture fitness check.

## Notes review (AC-1)

The notes were rendered with `scripts/release.py release-notes --tag v3.0.0`
from the branch head `7e08a1d12`. They run to 1,496 lines and carry the
`### Added`, `### Changed` and `### Fixed` headings once each, in that order,
with no staging text and no internal marker.

**Read as a new user** (what the release does):

- Found and fixed: moving two fixes out of the legacy group put a
  `### Fixed` heading just before the group. The group opens with three
  entries that have no heading of their own (the estimation method, Grok
  Build as a host and the Herdr overlay), and they then rendered under Fixed.
  The repair commit now places a `### Changed` heading before the group, so
  they render under Changed as they did before. The working-tree notes test
  pins this.
- Kept as is: each section lists this line's entries first and the legacy
  group's entries after them. The legacy bytes are pinned by digest, so
  their order is not changed here.

**Read as a user upgrading from 2.1.0** (what to do, in order): the notes
open with six numbered steps. Each step points at detail that exists:

| Step | Where the detail is |
|---|---|
| 1. Repairs before updating | the "Breaking migrations" block under Changed |
| 2. Install the 3.0.0 binary first | "Pinned, checksum-verified CI binary" |
| 3. Raise only `scaffold_version` | the same entry |
| 4. `codeflow update`, effort, `ids seed` | "Effort default on upgrade", "Shared id registry" |
| 5. The `Task:` line | "Pull request classification and light planning paths" |
| 6. Portal ownership | the table under "Portal ownership migration" in `docs/releasing.md` |

Step 6 names a repository file, not a link, so a reader of the published
notes has to open the repository. This is left as is: the table is long and
belongs in the runbook. The release checklist still requires both reads on
the final assembled source before the tag; this read covers the line as it
stands.

## Entries against the breaking commits (AC-2)

Every commit since `v2.1.0` marked breaking has its migration in an entry:

| Commit | Change | Entry that carries the migration |
|---|---|---|
| `93136b408` | Task line in the PR template | "Pull request classification and light planning paths" |
| `00739affd` | typed worker routes, ensemble v4 | "Effort default on upgrade" (no step needed, update installs it) |
| `83903695d` | high primary defaults | "Effort default on upgrade" |
| `ca5688592`, `a18cb760b` | portal runtime ownership | "Breaking: explicit portal runtime ownership" and the runbook table |
| `63586080e` | renderer compression runtime | "Breaking: presentation build reproducibility" |
| `ad234a2f1` | portal dependency updates | "Breaking: portal dependency security updates" |
| `7b4f10a6c`, `b926e62ef`, `19ea5a453`, `83329db97` | records, dangerous commands, test modes, coverage scopes | the "Breaking migrations" block |

Three other commits (`5a7c1312f`, `99d579d25`, `764fd1422`) name
`BREAKING CHANGE` only in a bullet about commit policy; they are not
breaking.

- **Pasted prompt entry:** settled once, as this line's entry "Delegate turns
  accept a pasted prompt". Its text matches the shipped instruction
  `Carry out the pasted instructions.` in cf-delegate and cf-herdr. The older
  EPC-016 text under the same label is not carried.
- **EPC-018:** its entries stay out, as the task's non-goal states (its own
  4.0.0 release).

## Typed repair mutation run

Each check was removed in turn, and the typed repair, entry edit, entry
identity and pull request tests (55) were run against the mutant from a
snapshot of the script. Rows M7 to M15 were added after review round 1.

| Mutant | Result | Killed by |
|---|---|---|
| M0 none (control) | 55 pass | |
| M1 configuration path check removed | killed | `test_a_repair_cannot_change_the_release_configuration` |
| M2 head configuration comparison removed | killed | `test_the_repair_is_judged_with_the_base_configuration` |
| M3 allowed path check removed | killed | `test_a_broken_base_blocks_ordinary_work` |
| M4 stamp comparison removed | killed | `test_a_repair_that_changes_more_than_a_stamp_is_refused` |
| M5 manifest hash check removed | killed | the stale hash and manifest hash tests |
| M6 repair judged with the head's configuration | killed | `test_the_repair_is_judged_with_the_base_configuration` |
| M7 baseline stamp check removed | killed | the stamp off the release and baseline left behind tests |
| M8 baseline consistency never checked | killed | four baseline tests |
| M9 baselines checked only when the manifest changes | killed | `test_a_baseline_changed_without_the_manifest_is_refused` |
| M10 lazy continuation dropped from an entry | killed | the continuation and extent tests |
| M11 a repair may edit an entry | killed | `test_a_repair_cannot_rewrite_an_existing_entry` |
| M12 `none` turns a same-impact rewrite into wording | killed | seven edit tests |
| M13 rewrapping counted as an edit | killed | the two rewrap tests |
| M14 base judged by the pull request's configuration | killed | `test_the_repair_is_judged_with_the_base_configuration` |
| M15 an unreadable base configuration refused | killed | the configuration fallback test |

M9 first survived: every baseline test also changed the manifest. The
baseline-only test was added, and the rerun killed it.

## Review round 1

Codex requested four changes; the fixes are in the checker and its tests.

- **Continuation text (F3).** An entry is now the whole bullet as Markdown
  renders it, lazy continuation lines included, so changing or deleting one
  is an edit.
- **Wording (F4).** A `none` declaration no longer turns a same-impact
  rewrite into wording. The checker cannot prove that changed words keep
  their meaning, so only rewrapping (every word, the label and the impact
  kept) is not an edit; any other change is assessed at the entry's impact.
  A repair may not edit an existing entry at all.
- **Baselines (F2).** When a repair touches a managed baseline or the
  manifest, each baseline must carry the one managed stamp of the release
  version and the manifest must record its exact hash.
- **Base configuration.** Replaying the line's landing on `main` showed that
  `main`'s configuration predates the line's schema, so judging every base by
  its own configuration refused the line. A base whose configuration this
  checker cannot read is now judged with the pull request's, and the output
  says so; a readable one is still used.
- **Preflight on a broken base.** Kept as designed: on a base that is
  already invalid the preflight warns, and the pull request job judges the
  change. The runbook says so rather than claiming it catches every new break.

## Landing replay (F1)

Each step was run the way its GitHub job runs it: the job's
`scripts/release.py` and `.release/config.json` come from the pull request's
merge commit (or the pushed commit), with live host state read through `gh`
for `sathyassn/codeflow-archive`. Merge commits were built as unreferenced
objects; nothing was pushed. Line tip `fe536d92d`, `main` `2c9c77f5c`. The
script is `replay-tsk106.sh` in the session scratchpad.

| Step | Checker | Result |
|---|---|---|
| Line tip, release state | the tip's | fails: legacy marker is not the bounded group |
| PR 1, the repair commit alone | the tip's (the PR changes no script) | fails: same base failure |
| PR 1, the whole task | this task's | fails: not a repair |
| Line after PR 1, release state | the tip's | ok, 3.0.0 |
| PR 2, the task on the repaired line | this task's | ok, minor, one entry added |
| Line after PR 2, release state | this task's | ok, 3.0.0 |
| The line onto `main` | this task's | ok, `main`'s configuration named as unreadable |
| `main` after, release state | this task's | ok, 3.0.0 |

No first pull request can pass its own release job on this line. The job
runs the checker in the PR's merge tree; the tip's checker validates the
base before anything else and has no repair path, so it fails on the broken
base whatever the PR changes (a configuration change cannot help, because
the base's changelog marker carries the old digest). A PR that brings the
new checker is judged by it, and it refuses a repair that also changes the
checker. Landing the first step therefore needs an operator decision. Every
later step passes. Hosted jobs on this repository currently do not start
(the account's billing blocks them), so the local replay is the evidence for
every step.

## Architecture fitness check

- **One path-set table.** `crates/codeflow-core/src/workgraph/path_sets.toml`
  is read by `codeflow ci` and by `release.py`. Tests check that every
  release contract member is adopter-facing and watched in
  `.release/config.json`, and that each reader classifies behaviour paths
  by the table.
- **One calculator.** Pre-push and `codeflow integrate` run
  `scripts/release.py`; the Rust code adds no second release calculation.
- **Release jobs stay project-owned.** The managed CI file carries no release
  job after `init` and after `update`, at every tier, and CodeFlow's jobs
  live in `codeflow-release.yml`.
- **One Release impact fixture set.** Both parsers pass the 26 cases in
  `scripts/fixtures/release_impact_cases.json`.

## Not verified

- The new `codeflow-release.yml` workflow has not run on the host yet.
- The replay reads live host state, but the hosted jobs themselves have not
  run: they do not start on this repository at present.
- The notes review on the final assembled source before the tag belongs to
  the release and is not part of this record.
