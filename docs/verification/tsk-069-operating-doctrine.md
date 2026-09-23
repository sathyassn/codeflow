# Operating doctrine body verification

This records the integration check for the operating doctrine epic (EPC-017)
before it goes to `main`. The four tasks landed on
`integration/EPC-017-operating-doctrine`, a combined review found three
editorial defects, and those were fixed on the same branch. The head below
passed every check named in TSK-069. A Codex review then found seven more
defects. Its second round accepted six of the fixes on
`fix/epc017-codex-review`, found one still open in the live commit-msg hook,
and found one new defect in the binary-file check. Its third round accepted
the binary-file fix and found two more gaps in the live hook. Fixes for those
are on the same branch and await a Codex recheck. The full gate on that branch's final
head, with the tested commit, is recorded in the body of the pull request
that lands it, and the pull request to `main` repeats it for the head that
ships.

```text
TSK-066 written content policy  (#521) --+
TSK-065 pull request follow-up  (#523) --+--> integration head --> checks --> PR to main
TSK-067 verified links          (#524) --+       e6121491e
TSK-068 blind evaluation cases  (#527) --+
combined editorial fixes        (#529) --+
Codex review fixes              (fix/epc017-codex-review) --> final head
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
break of a published contract, since version 3.0.0 is unpublished (no `v3`
tag). It does need an upgrade order. Corrected on 2026-09-23 after the Codex
review below: `codeflow update` adds the key to an existing consumer's
`policy.json`, because a user-owned JSON policy file gains new default keys
additively (`scaffold/update.rs`, `scaffold-manifest.toml`). The key does not
reach only new scaffolds, as this section first said. The hooks run whichever
`codeflow` is on `PATH`, and a binary built before the key rejects it, so
updating the scaffold before upgrading that binary blocks every commit. The
upgrade order is now in `docs/releasing.md`, `docs/adoption.md` and the 3.0.0
changelog entry.

### Policy key compatibility

Run on 2026-09-23 with the installed `codeflow` 3.0.0 (built 2026-09-13,
before the key) and the binary built from this branch, on two minimal
scaffolds: one made by the installed binary (no key) and one by the new build
(key present). The message was a plain conventional subject, then the same
subject with an em dash.

| Policy file | Hook binary | Plain message | With an em dash |
|---|---|---|---|
| Without the key | installed | exit 0 | exit 0, rule not checked |
| Without the key | new build | exit 0 | exit 1, `git.policy_characters` |
| With the key | new build | exit 0 | exit 1, `git.policy_characters` |
| With the key | installed | exit 1, unknown key | exit 1, unknown key |

The last row's message is `policy error: unknown key git.policy_characters`.
Running the new build's `codeflow update` on the keyless scaffold reported
`keys-added .codeflow/policy.json` with `added key git.policy_characters`,
and the installed binary's hook then failed the plain message with exit 1.

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
`policy.json` carries `"policy_characters": "block"`: the `1 keys-added`
above is `update` adding that key to the existing file.

## Reviews

| Review | Scope | Verdict |
|---|---|---|
| Grok, combined body | `origin/main...9af5c6d02`: cross-file contradictions, mirrors, hashes, the new policy key against installed hooks, epic criteria | approved; lesser notes applied in #529 |
| Fable, editorial round 1 | all prose in the combined diff | changes requested: three blocking findings |
| Fable, editorial round 2 | the fixes in #529 | approved |
| Codex | none | not run: the seat is unavailable until 2026-09-27; recorded as reduced assurance |
| Codex gpt-6-astra, high, 2026-09-23 | adversarial review of `origin/main...962ec0b6f` | changes requested: one P1 and six P2 findings; fixes submitted on `fix/epc017-codex-review` |
| Codex gpt-6-astra, high, 2026-09-23, round 2 | recheck of `962ec0b6f..01c9570de` | changes requested: six findings resolved; finding 3 open in the live hook; new finding N1 |
| Codex gpt-6-astra, high, 2026-09-23, round 3 | recheck of `01c9570de..f76e82368` | changes requested: N1 resolved; finding 3 still partly open, with new findings N2 (scissors) and N3 (comment prefix). Fixed below, awaiting recheck |

### Codex round two and three fixes

**Finding 3, live hook.** Git runs the commit-msg hook on the message file
before its own cleanup. Checked with Git 2.53.0 in a disposable repository:
the file handed to the hook still holds `#` lines that `--cleanup=strip`
later removes, and Git exports `GIT_EDITOR=:` to the hook when no editor
runs (`-m`, `-F`, `--amend --no-edit`, `git merge --no-edit`).

Round two tried to predict Git's cleanup, and round three found two places
where the prediction dropped text Git keeps. The hook now drops only text
Git is certain to remove and scans everything else:

| Text in the message file | Hook drops it when |
|---|---|
| A line starting with the exact comment prefix | cleanup is `strip`, or the default mode with an editor |
| Git's cut line followed by its two instruction lines, and all below | always; Git writes this only where it truncates |
| Anything else, including a bare cut line | never |

With the default `#` prefix, Git's cleanup keeps comment lines here:

| Situation | Git keeps `#` lines |
|---|---|
| No editor, default cleanup | yes |
| Editor, default cleanup | no |
| `commit.cleanup` whitespace, verbatim or scissors | yes |
| `commit.cleanup=strip` | no |
| `core.commentChar=;` with an editor | yes; the `;` lines go |

- **N2, scissors.** Git truncates at the cut line only for a verbose or
  scissors cleanup (Git 2.53.0 `sequencer.c` `cleanup_message`), and a direct
  `git merge` cleans up with verbose off even under `commit.verbose`
  (`builtin/merge.c` line 980). An editor session alone is no evidence. The
  hook cuts only at the three lines `wt_status_append_cut_line` writes
  (`wt-status.c` lines 1117 to 1123).
- **N3, comment prefix.** `core.commentChar` and `core.commentString` are
  read with `git config -z`, byte for byte, the last setting winning as in
  Git. A value of `auto` means Git picks the prefix inside its own process,
  so the hook treats no line as a comment and scans the whole file.

The code is `GitCleanup` in `crates/codeflow-core/src/hooks/git_hook.rs` and
`pending_cleanup` in `crates/codeflow-cli/src/cmd/git_hook.rs`. Six
installed-hook regressions in `crates/codeflow-cli/tests/hooks_cli.rs` drive
real `git commit` and `git merge`, including a passing edited-template
control with a verbose diff preview.

What the hook cannot know, and how it errs:

- A command-line `--cleanup`, `-v` or `--no-verbose` is invisible to a hook.
  The hook reads only the environment and config.
- Where Git's behaviour cannot be observed, the hook scans more, not less:
  `-m --cleanup=strip`, `-v -m` or `commit.verbose` on a commit whose cut line
  Git removes, `commentChar=auto`, and a verbose preview under a non-English
  locale, where Git translates the instruction lines.
- It can scan less than Git keeps in two cases: an editor session with a
  command-line `--cleanup` of verbatim, whitespace or scissors, and a message
  that itself carries Git's full cut signature without a verbose or scissors
  cleanup.
- The stored-message check in `codeflow ci` scans every byte Git recorded.
  It stays required and is the authority; the hook gives early feedback.

**N1, quoted names.** The binary check now reads each patch's new-side blob
id from its `index` line (`--full-index`) instead of looking up a path, and a
quoted header name is decoded in full, so reports name the real file. An
unreadable blob id is an error, not a pass. A fixture in
`crates/codeflow-cli/tests/ci_cli.rs` adds a binary named
`docs/release"preview.png` holding em dash bytes, which passes, and a text
file `docs/release"notes.md` with an em dash, which fails.

Fable's findings N6 and N8 were left by change control: ADR-0067 is accepted
and append-only, and the completed task records keep their text; their
closeouts now point here.

## Not verified

- Hosted CI: GitHub Actions refuses every job for billing, so no hosted
  check ran on any of these pull requests. The local full gate is the
  evidence.
- Native model trials of the new evaluation cases. The cases grade recorded
  answers; registering them proves nothing about live behaviour.
- The upgrade-order case on a real consumer repository; the compatibility
  table above uses disposable minimal scaffolds.
