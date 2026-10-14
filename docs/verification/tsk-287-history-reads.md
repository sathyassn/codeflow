# Guarded history reads

Local judging reads refuse a graft file or replace ref before trusting
parents or ancestry. Walks also refuse a shallow boundary they reach.
This sweep records the TSK-287 migration for issue 122; independent review
and the primary's essential and full gates follow the builder's draft PR.

## Scope and limits

- `workgraph::history` owns parent lists, parent lookup, revision walks,
  ancestry, merge bases, ahead/behind counts and the raw default-line walk.
- The spawned range readers call `check_command` before `merge-base`,
  `rev-list`, `cherry` and the three-dot diff. Those queries require a full
  clone; bounded libgit2 walks may stop at a hidden shallow base.
- The source scan catches direct Rust parent and ancestry calls, including
  new calls in the counted exception files. It does not resolve types,
  inspect subprocess arguments, or prove that callers propagate errors.
- Tests plant both a graft and a replace ref for acceptance binding,
  journey scope, reopen detection, landing, pin containment, epic-line
  coverage and range bases. The installed CLI journey uses the shipped
  pre-push shim, `work start` and local CI; its destination is a local bare
  repository, so it does not verify hosted enforcement.
- This reader does not lock Git metadata against concurrent external edits.
  A fresh read checks overlays again; a walk uses the boundary it read when
  it began.

## Judging sites

The entries name each guarded call in production source. Acceptance and
lifecycle callers also serve `status_verb` and `ci/classification.rs`.
The CLI's push-set helpers guard their spawned history commands at the
shared `git` and `git_input` entry points.

| Site | Guarded read |
|---|---|
| `crates/codeflow-cli/src/cmd/ci/change_class.rs:61` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:423` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:1646` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:1661` | `check_command` |
| `crates/codeflow-core/src/git/conflict.rs:58` | `merge_base` |
| `crates/codeflow-core/src/hooks/git_discard.rs:201` | `descendant_of` |
| `crates/codeflow-core/src/hooks/git_hook.rs:1129` | `merge_base` |
| `crates/codeflow-core/src/hooks/git_hook.rs:1241` | `descendant_of` |
| `crates/codeflow-core/src/integrate.rs:258` | `descendant_of` |
| `crates/codeflow-core/src/integrate.rs:602` | `revwalk` |
| `crates/codeflow-core/src/status.rs:425` | `check` |
| `crates/codeflow-core/src/status.rs:428` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:93` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:108` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:112` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:121` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:124` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:194` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:651` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:958` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:978` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1009` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1061` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1082` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1108` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1137` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1204` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1221` | `parent` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1222` | `parent` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1227` | `merge_commits` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1308` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1318` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1698` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2021` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2044` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2587` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2742` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2794` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2821` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2882` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3006` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3014` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3020` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3049` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3208` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3297` | `merge_base` |
| `crates/codeflow-core/src/workgraph/amendment.rs:295` | `merge_base` |
| `crates/codeflow-core/src/workgraph/landing.rs:201` | `check` |
| `crates/codeflow-core/src/workgraph/landing.rs:375` | `revwalk` |
| `crates/codeflow-core/src/workgraph/landing.rs:646` | `revwalk` |
| `crates/codeflow-core/src/workgraph/landing.rs:654` | `parents` |
| `crates/codeflow-core/src/workgraph/landing.rs:692` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/landing.rs:710` | `revwalk` |
| `crates/codeflow-core/src/workgraph/landing.rs:847` | `revwalk` |
| `crates/codeflow-core/src/workgraph/landing.rs:860` | `parents` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:616` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2156` | `parents` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2162` | `parent` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2166` | `parents` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2211` | `revwalk` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2224` | `parents` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2308` | `merge_base` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2379` | `merge_base` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2412` | `merge_base` |
| `crates/codeflow-core/src/workgraph/lifecycle.rs:2614` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/line_adoption.rs:150` | `revwalk` |
| `crates/codeflow-core/src/workgraph/line_adoption.rs:192` | `parents` |
| `crates/codeflow-core/src/workgraph/line_adoption.rs:215` | `merge_base` |
| `crates/codeflow-core/src/workgraph/line_adoption.rs:232` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:425` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/readiness.rs:472` | `check_command` |
| `crates/codeflow-core/src/workgraph/readiness.rs:487` | `check_command` |
| `crates/codeflow-core/src/workgraph/readiness.rs:505` | `revwalk` |
| `crates/codeflow-core/src/workgraph/readiness.rs:510` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:519` | `revwalk` |
| `crates/codeflow-core/src/workgraph/readiness.rs:525` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:549` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:555` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:557` | `parent` |
| `crates/codeflow-core/src/workgraph/readiness.rs:605` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:608` | `parent` |
| `crates/codeflow-core/src/workgraph/readiness.rs:1202` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/readiness.rs:1434` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:393` | `merge_commits` |
| `crates/codeflow-core/src/workgraph/release_line.rs:632` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:646` | `merge_base_many` |
| `crates/codeflow-core/src/workgraph/release_line.rs:767` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:861` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:875` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/release_line.rs:937` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:974` | `revwalk` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1043` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1053` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1184` | `check` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1186` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1203` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1234` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1237` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1242` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1253` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1301` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1321` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1366` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1708` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1776` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1889` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1905` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1938` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1979` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1987` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1988` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2042` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2298` | `recorded_first_parent` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2558` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/status_verb.rs:342` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:408` | `ahead_behind` |
| `crates/codeflow-core/src/workgraph/work_start.rs:674` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1328` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1331` | `revwalk` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1341` | `parent_id` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1344` | `parents` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1375` | `check` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1386` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1428` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1631` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1924` | `revwalk` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1991` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:2388` | `descendant_of` |

## Direct reads retained for display

| Site | Reason |
|---|---|
| `crates/codeflow-core/src/ceremony/history.rs:77` | Ceremony metrics; never authorizes work or landing |
| `crates/codeflow-core/src/ceremony/history.rs:89` | Ceremony metrics; never authorizes work or landing |
| `crates/codeflow-core/src/ceremony/history.rs:189` | Ceremony metrics; never authorizes work or landing |
| `crates/codeflow-core/src/ceremony/history.rs:190` | Ceremony metrics; never authorizes work or landing |
| `crates/codeflow-core/src/ceremony/history.rs:191` | Ceremony metrics; never authorizes work or landing |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:272` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:287` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:291` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:461` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:466` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:479` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:504` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:588` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:601` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/estimate/outcomes/history.rs:609` | Historical estimates; never authorizes a transition |
| `crates/codeflow-core/src/hooks/session_summary.rs:168` | Session context only; never gate evidence |
| `crates/codeflow-core/src/hooks/session_summary.rs:179` | Session context only; never gate evidence |
| `crates/codeflow-core/src/root_checkout.rs:1305` | Ahead/behind counts in the checkout inventory |

`contained.rs` has a counted non-Git exception: its `parent` method walks
filesystem paths. Test fixtures construct history directly.
