# Release-rule cutoffs and the seven legacy landings, 2026-09-27

## Scope

This is the verification record for TSK-145 AC-10 (SPC-013 R-120,
planning resolution 22). The raw 3.0.0 rehearsal replay refused seven
criteria changes because each landed on its line in a task pull request
that also changed code. R-120 is newer than those landings, so it applies
from a per-line cutoff recorded on the default target. This record lists
the seven, what each changed, and which review approved each pull request.

## Evidence

- Merges and diffs: `git diff <merge>^1 <merge> -- project-management/tasks/<id>.md`,
  comparing the `## Acceptance Criteria` sections with `- [ ]` and `- [x]`
  ticks normalized.
- Pull requests: `gh pr view <n> --repo sathyassn/codeflow-archive`.
  None of the seven has a GitHub review object or comment, and the
  operator account `sathyassn` merged all seven.
- Review reports: the session review files named in each PR body.
  They live in the session scratchpad and are not versioned, so their
  content is recheckable there but not authenticated. "Codex" and "Grok"
  are the reviewer labels those files and PR bodies use.

## The seven landings

| Record | Line | Landing merge (PR) | What changed in the criteria | Review that approved the PR |
|---|---|---|---|---|
| TSK-134 | EPC-020 | `787e53448` (#585) | AC-3 moved from "shall refuse a `CARGO_TARGET_DIR` outside the worktree" to "shall warn about" it and still run, in commit `7c4bf8608` | The primary (the orchestrator) sanctioned the amendment during review; the criterion records it with its reason (see below). Codex rounds 1 (`531c0e2d6`) and 2 (`b977fc994`) both ended `changes_requested` on other findings, and the acceptance block's `verdict: approved` is the primary's. |
| TSK-101 | EPC-020 | `972c15b61` (#590) | AC-14 (the pinned, checksum-verified installer) was replaced by "Moved to TSK-107", in commit `7dd43bed5` | Codex round 3 `approved` at `c04a70f22`, a confirm-only check of one finding. Round 1 saw the move and did not count it as a finding, but ended `changes_requested`. Commits after `c04a70f22` were checked by the primary only. |
| TSK-103 | EPC-020 | `cf206a232` (#598) | AC-9's own-branch context changed from "`work start` and pre-commit" to "`work start` and CI's own-branch preflight", in commit `a99af1edc` | None. Codex rounds 1 and 2 ended `changes_requested`, and the primary read the final diff instead of a third round. |
| TSK-104 | EPC-020 | `8ad5b6fce` (#587) | AC-2 dropped pre-commit from where the classification runs ("`work start` and pre-commit" became "`work start`"), in commit `f7ed15bc0` | None. Codex rounds 1 and 2 ended `changes_requested`. Round 1 accepted removing the pre-commit preflight as behaviour, but not the criterion text. |
| TSK-112 | EPC-020 | `383ee8c48` (#592) | AC-3 was tightened: an unresolved target now blocks a mutation from any session branch. Result lines were appended to AC-1 to AC-4. | None. Codex rounds 1 to 5 all ended `changes_requested`. Round 1 finding T112-4 asked for the tightening, and round 2 marked it resolved. |
| TSK-115 | EPC-020 | `dd8a6e596` (#601) | No criterion wording changed. Evidence text was appended under AC-1 to AC-4, which the parser reads as criterion text. | Codex round 2 `approved` at `1cd1f519b`, excluding AC-3, whose evidence came after the approval. |
| TSK-093 | EPC-016 | `f5f2b6158` (#613) | No criterion statement changed against its planning version (`b2aae5400`). Ticks, appended evidence and pending lines, the target and a sequencing paragraph changed. | Grok round 1 `approved` at `fc54d9191`, the merged head. It did not examine the criteria. |

Three of the seven pull requests reached an approving review verdict
(#590, #601, #613). The other four landed on the primary's decision after
the last review verdict was `changes_requested`.

## TSK-134 AC-3

The primary, the orchestrator acting during review, sanctioned moving
AC-3 from "refuse" to "warn". The criterion records the amendment and its
reason at `project-management/tasks/TSK-134.md` lines 47 to 49: "amended by
the primary during review: the gate lock already keeps two gates apart and
a shared target directory is a legitimate adopter setup". The round 1
brief told the reviewer the change was coming, and round 1 judged the
warning behaviour on its merits. No separate task is needed.

## The TSK-101 waiver

TSK-101's first completion waives AC-14 against `7dd43bed5` as its
planning amendment. That commit is not a planning amendment and not a
Closeout correction: it is a commit on `task/TSK-101-id-registry` made
before the task landed (parent `ae318a7a8`, the task still `todo` on both
sides), and besides `TSK-101.md` it changes `CHANGELOG.md`,
`docs/architecture.md` and two ADRs. The binding check therefore refuses
the waiver (`work.acceptance_binding`), and the cutoff does not exempt it:
the exemption covers only the brought criteria-landing finding.

The EPC-020 line has since repaired it through the ordinary route, before
its cutoff `2921df9f5`: planning PR #639 (`dc62ef03f`) restates AC-14 and
records the old block as superseded, and PR #640 (`f6cd1c3c3`) completes
TSK-101 again with its AC-14 waiver naming `dc62ef03f`. A release branch
that imports EPC-020 only at or after `dc62ef03f` never brings the old
waiver. A release branch that imported EPC-020 between #590 and #639, as
the rehearsal did at `7f8d3bc47`, `0583c69ba`, `0b2eee788` and
`cd0ac7e74`, brought the old completion too. A later import of the repair
supersedes it (SPC-013 R-120): a completion brought later from the task's
own line, which binds where it was introduced there, becomes the
completion in force. The old waiver is never accepted; it stops being the
completion in force. An invalid later completion, or one that reaches the
release through another line, leaves the old finding in place.

## The reconstructed candidate

The candidate of TSK-145 AC-6 was rebuilt on a disposable authority
(labelled a reconstruction, not the historical commits):

- The authority's `main` is the real `main` (`2c9c77f5c`) plus the
  cutoffs below. Each line is advertised at its cutoff tip; EPC-020 also
  carries a planning merge that gives TSK-010 `role: release-integration`.
- The rehearsal (`cbc0b2ec8`) was cut from the EPC-020 line at
  `405caf064`. Here that point is an import merge from `main`, and every
  later first-parent commit of the rehearsal is replayed with its own tree
  and second parent, the cutoff table kept.
- Left out are the direct record edits the EPC-014 line has since landed
  itself: `1dc0033a2` (the TSK-049 completion), `08aef53c0` (landed there
  as `3f9c7b0dd`) and the EPC-014 part of `082d36488` (landed as
  `ccfb1d8f0`).
- Each line is then imported at its cutoff tip, the records baseline names
  those tips, and TSK-010 completes at the head.

| Run | Exit | Legacy notices | Blocking findings |
|---|---|---|---|
| Pre-push (`git push` from the release checkout)[^tree] | 1, the ref not published | the seven | 2, the same as CI |
| CI, release range from the authority's `main` | 1 | the seven | 2 |
| Final pull request into `main` (`--into main`) | 1 | the seven | 2 |

[^tree]: Pre-push judged everything the release branch adds to the
    authority's `main`. Its tree checks (`validate --docs` and the quick
    targets) were kept out by a changed tracked file, because the replayed
    tree is not a buildable release. Its release preflight did not run: the
    replayed tree's `release.py` has no `preflight` subcommand.

The release-line judge reports no blocking finding. The TSK-101 waiver's
refusal is gone because the EPC-020 import at its cutoff brings the
re-completion of PR #640. The two remaining findings come from the records
rule, and TSK-140 owns both:

| Rule | Finding | Owner |
|---|---|---|
| `work.records` | SPC-002 becomes `approved` in a range that also changes code. EPC-020 approved it in planning PR #655 (`e4ff8eb4e`), but the rule judges spec approval across the whole range, not where it was introduced. | TSK-140 AC-11 |
| `work.records` | TSK-069 is complete without an acceptance block. Its only change against `main` is the `uid` backfill EPC-020 landed in #654 (`a579cf17a`); naming the EPC-020 cutoff in the records baseline does not clear it. | TSK-140 AC-12 |

The rule raises both on any pull request into this `main` that brings
EPC-020's current records with code, so they would block the real 3.0.0
pull request. Importing the current line tips (EPC-016 at `385218852`,
EPC-020 at `99ad91a78`) with the same cutoffs gives the same seven
notices and the same two findings: none of the criteria changes this
candidate brings landed with code after a cutoff, and the cutoff values
need no change.

The first reconstructions also showed three judge faults, fixed with this
record:

- A new release branch's push was judged from one line's tip, which
  re-checked 1,221 commits including history `main` already has.
- A completion that the release branch first held in one form and later
  brought, block and all, from its own line was still bound at the release
  head.
- An earlier brought completion that a valid re-completion from the task's
  own line had replaced still refused (the TSK-101 waiver above).

## Cutoffs to record

The cutoffs are each line's tip when this record was written (advertised
by `origin`, 2026-09-27). They take effect only once they are on the
default target, `main`, in `.codeflow/project.toml`:

```toml
[release_rule_baseline]
"integration/EPC-014-public-docs" = "d618075e229d49f706cf1c0e9ab5b10a3c9c69e3"
"integration/EPC-015-engineering-bar" = "db55fc01c30f75d0eb1bd5f3df1de9dd8f9d1bff"
"integration/EPC-016-visual-guide" = "51ee9374b506d9a359150ac15663c26c914b57cf"
"integration/EPC-018-autonomy-roster" = "ccd56fa85160ac58d66828933e96c000357c4baa"
"integration/EPC-020-delivery-system" = "2921df9f52a5e787f896146233a85201720591d3"
```

With these, a release range lists each of the seven as information, for
example "legacy criteria change, landed before the release rule: TSK-134
on integration/EPC-020-delivery-system, landing 787e53448, cutoff
2921df9f5, policy main at <tip>". Every other check still applies to
them, including import qualification, full tree entries, completion
binding and the release-integration owner.

## Not verified

- The reviewer identity behind each label, and whether the scratchpad
  review files were edited after they were written.
- The source of the "ruled 2026-09-27" notes in TSK-103 and TSK-104
  beyond the records themselves.
- Whether commits landed after each last review changed behaviour beyond
  what their PR bodies say.
