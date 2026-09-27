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

Each check in the typed repair was removed in turn, and the typed repair
tests (12) were run against the mutant from a scratch copy of the script.

| Mutant | Result | Killed by |
|---|---|---|
| none (control) | 12 pass | |
| configuration path check removed | killed | `test_a_repair_cannot_change_the_release_configuration` |
| head configuration comparison removed | killed | `test_the_repair_is_judged_with_the_base_configuration` |
| allowed path check removed | killed | `test_a_broken_base_blocks_ordinary_work` |
| stamp comparison removed | killed | `test_a_repair_that_changes_more_than_a_stamp_is_refused` |
| manifest hash check removed | killed | `test_manifest_hashes_must_be_the_managed_baselines` |
| repair judged with the head's configuration | killed | `test_the_repair_is_judged_with_the_base_configuration` |

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
- The host-backed `check-pr` and `check-state` runs, which read published
  releases, were not run locally.
- The notes review on the final assembled source before the tag belongs to
  the release and is not part of this record.
