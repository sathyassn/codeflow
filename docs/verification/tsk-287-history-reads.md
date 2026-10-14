# Guarded history reads

Local judging reads refuse a graft file or replace ref before trusting
parents or ancestry. Walks also refuse a shallow boundary they reach.
This sweep records the TSK-287 migration for issue 122.

## Scope and limits

- `workgraph::history` owns parent lists, parent lookup, revision walks,
  ancestry, merge bases, ahead/behind counts and the raw default-line walk.
- CI checks history once at entry, including a task-less range without a
  PR body, before its commit log, merge bases, added lines, conflict markers
  and classification diffs. Push-set queries guard their shared Git entry
  points. Spawned queries require a full clone; bounded libgit2 walks may
  stop at a hidden shallow base.
- A read-only judgment memoizes the overlay check and shallow boundary per
  repository on its thread. Nested scopes reuse that snapshot; the outer
  scope clears it on return. Calls outside a scope check metadata afresh.
  Every parent step still checks its commit against the saved boundary.
- `GIT_SHALLOW_FILE` follows the spawned query's working directory, set to
  the repository root with `current_dir` or Git's `-C`. Missing and empty
  override files, or an empty override value, mean no boundary; other read
  errors refuse.
- The source scan catches direct Rust parent and ancestry calls, including
  new calls in the counted exception files. It does not resolve types,
  inspect subprocess arguments, or prove that callers propagate errors.
- Tests plant both a graft and a replace ref for acceptance binding,
  journey scope, reopen detection, landing, pin containment, epic-line
  coverage and range bases. The installed CLI journey uses the shipped
  pre-push shim, `work start` and local CI; its destination is a local bare
  repository, so it does not verify hosted enforcement.
- This reader does not lock Git metadata against concurrent external edits.
  A new judgment checks overlays again; an active judgment and its walks
  use the snapshot they began with. An object fetch invalidates its
  repository's snapshot; other Git mutations must end the scope.

## Judging sites

The entries name each guarded call in production source. Acceptance and
lifecycle callers also serve `status_verb` and `ci/classification.rs`.
The CLI's push-set helpers guard their spawned history commands at the
shared `git` and `git_input` entry points. CI's entry guard also covers its
spawned readers listed below.

| Site | Guarded read |
|---|---|
| `crates/codeflow-cli/src/cmd/ci/change_class.rs:61` | `check_command` |
| `crates/codeflow-cli/src/cmd/ci.rs:254` | `judgment` |
| `crates/codeflow-cli/src/cmd/ci.rs:354` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:423` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:1646` | `check_command` |
| `crates/codeflow-cli/src/cmd/push_set.rs:1661` | `check_command` |
| `crates/codeflow-core/src/git/conflict.rs:58` | `merge_base` |
| `crates/codeflow-core/src/hooks/git_discard.rs:201` | `descendant_of` |
| `crates/codeflow-core/src/hooks/git_hook.rs:1129` | `merge_base` |
| `crates/codeflow-core/src/hooks/git_hook.rs:1241` | `descendant_of` |
| `crates/codeflow-core/src/integrate.rs:258` | `descendant_of` |
| `crates/codeflow-core/src/integrate.rs:602` | `revwalk` |
| `crates/codeflow-core/src/status.rs:143` | `judgment` |
| `crates/codeflow-core/src/status.rs:426` | `check` |
| `crates/codeflow-core/src/status.rs:429` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:87` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:94` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:109` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:113` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:122` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:125` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:195` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:652` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:959` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:979` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1010` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1062` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1063` | `check` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1084` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1110` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1139` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1205` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1207` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1224` | `parent` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1225` | `parent` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1230` | `merge_commits` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1303` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1312` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1322` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:1702` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2025` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2048` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2591` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2731` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2747` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2799` | `revwalk` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2826` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2872` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:2888` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3007` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3013` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3021` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3027` | `parent_id` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3050` | `judgment` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3057` | `parents` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3216` | `merge_base` |
| `crates/codeflow-core/src/workgraph/acceptance.rs:3305` | `merge_base` |
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
| `crates/codeflow-core/src/workgraph/readiness.rs:600` | `judgment` |
| `crates/codeflow-core/src/workgraph/readiness.rs:606` | `parents` |
| `crates/codeflow-core/src/workgraph/readiness.rs:609` | `parent` |
| `crates/codeflow-core/src/workgraph/readiness.rs:753` | `judgment` |
| `crates/codeflow-core/src/workgraph/readiness.rs:1204` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/readiness.rs:1436` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:393` | `merge_commits` |
| `crates/codeflow-core/src/workgraph/release_line.rs:535` | `refresh` |
| `crates/codeflow-core/src/workgraph/release_line.rs:633` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:647` | `merge_base_many` |
| `crates/codeflow-core/src/workgraph/release_line.rs:757` | `judgment` |
| `crates/codeflow-core/src/workgraph/release_line.rs:769` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:863` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:877` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/release_line.rs:939` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:976` | `revwalk` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1037` | `judgment` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1046` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1056` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1180` | `judgment` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1188` | `check` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1190` | `merge_base` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1207` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1238` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1241` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1246` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1257` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1305` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1325` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1370` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1709` | `judgment` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1713` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1781` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1894` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1910` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1943` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1984` | `parents` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1992` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:1993` | `parent_id` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2047` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2303` | `recorded_first_parent` |
| `crates/codeflow-core/src/workgraph/release_line.rs:2563` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/status_verb.rs:342` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:408` | `ahead_behind` |
| `crates/codeflow-core/src/workgraph/work_start.rs:674` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1328` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1331` | `revwalk` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1341` | `parent_id` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1344` | `parents` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1375` | `check` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1386` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1410` | `judgment` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1429` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1575` | `judgment` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1633` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1926` | `revwalk` |
| `crates/codeflow-core/src/workgraph/work_start.rs:1993` | `merge_base` |
| `crates/codeflow-core/src/workgraph/work_start.rs:2390` | `descendant_of` |
| `crates/codeflow-core/src/workgraph/work_start.rs:2802` | `judgment` |

## Spawned CI reads covered by its entry check

| Site | Query |
|---|---|
| `crates/codeflow-cli/src/cmd/ci.rs:2496` | Commit log and parent fields |
| `crates/codeflow-cli/src/cmd/ci.rs:2567` | Merge base for added lines |
| `crates/codeflow-cli/src/cmd/ci/conflict_markers.rs:90` | Merge base for conflict markers |
| `crates/codeflow-cli/src/cmd/ci/classification.rs:1004` | Three-dot diff for classification |

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
