# Autonomy and catalog body verification, 2026-09-27

## Scope

This record is TSK-078's verification of the combined EPC-018 body on
`integration/EPC-018-autonomy-roster` at `5a4d54a53`: TSK-086's
merge-forward (pull request 575), its completion record (579) and the
`AGENTS.md` region record fix (580). It covers the gates, the update journey, the native
identity canary, the model-name search and the budget ratchets. Every command
ran against a debug binary built from this head (`codeflow 3.0.0`, SHA-256
prefix `cfae2ce320266f07`).

The tree at `5a4d54a53` differs from `d65c72b78`, where TSK-086 ran the
workspace gates, only in two task records and one manifest hash
(`project-management/tasks/TSK-083.md`, `TSK-086.md`,
`.codeflow/manifest.json`).

## Gates

| Check | Result | Evidence |
|---|---|---|
| `cargo test --workspace` | Pass | 2356 passed, 0 failed at `d65c72b78` (code identical to this head) |
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` | Pass | exit 0 at `d65c72b78` |
| `codeflow validate`, `validate --docs` | Pass | 84 records clean; doc graph clean |
| `codeflow ci --base origin/main --head HEAD` | Pass | 73 commits and 12 merges; two `breaking_watch_paths` warnings (`242a55a7` adds the `models` command to `main.rs`; `36234984` adds manifest entries), both compatible additions |
| `doctor --check model-bindings` | Pass with warnings | exit 0; one "designated version with no full-suite record" warning per designated version and harness, which TSK-139 closes |
| `doctor --check managed-drift` | Pass | ok at `5a4d54a53`. At `215c6d107` it warned: the merge-forward kept the pre-merge `AGENTS.md` region hash in `.codeflow/manifest.json`. `codeflow update` rewrote the record to the current region's hash (`bd11d08d...`) and left `AGENTS.md` unchanged, so only the record was stale; pull request 580 fixed it |
| `artifact_budget_contract`, `catalog_craft_contract`, `manifest_consistency`, `model_catalog_surfaces`, `model_eval_contract` | Pass | 18, 13, 23, 10 and 47 passed at `215c6d107` |

Not run on this head: `codeflow test` as one command and
`cargo doc --workspace --no-deps`; the primary runs the full gate when it
lands the body.

## Update journey

A sample initialised by the installed `codeflow 3.0.0` (SHA-256 prefix
`08ee48e66d112afd`, a build without the `models` command, catalog schema 4)
was updated by the candidate, once at the standard tier and once at the full
tier.

| Check | Standard | Full |
|---|---|---|
| `update` exit | 0 | 0 |
| Catalog schema before and after | 4, then 5 | 4, then 5 |
| `current-ensemble.json` in `.claude` and `.agents` equals its shipped source | yes | yes |
| `cf-method/references/autonomy.md` present in `.claude` and `.agents`, equal to its shipped source | yes | yes |
| `cf-evaluate-model` cases and `eval_kit.py` equal their shipped sources | yes | yes |
| `AGENTS.md` and `CLAUDE.md` point at `autonomy.md` | yes | yes |
| `codeflow models resolve --help` | exit 0 | exit 0 |
| `doctor --check managed-drift` in the sample | ok | ok |
| A second `update` | no change | no change |

## Native identity canary

Each launch ran in its own Herdr tab on a nonce-named sample folder the
canary created, with Claude Code 2.1.283, Grok CLI 1.0.34 and Codex CLI
0.157.1. Each session got one prompt ("Reply with the single word ok").
The observed id comes from the harness's own display and its session record;
the served id comes from the vendor's response record in that session.
Answering the workspace-trust prompt for the canary's own sample followed
the harness rule of TSK-083 (exact path and session folder match).

| Seat use | Launch | Requested id and effort | Observed | Served (session record) | Result |
|---|---|---|---|---|---|
| `claude-primary` first line | pinned | `claude-opus-5-5`, high | status `claude-opus-5-5`; header "Opus 5.5 with high effort" | `claude-opus-5-5` | Match |
| `claude-primary` fallback | pinned | `claude-fable-5-1`, high | status `claude-fable-5-1`; header "Fable 5.1 with high effort" | `claude-fable-5-1` | Match |
| Opus line | alias | `opus`, high | status `opus (claude-opus-5-5)` | `claude-opus-5-5` | No newer version observed |
| Fable line | alias | `fable`, high | status `fable (claude-fable-5-1)` | `claude-fable-5-1` | No newer version observed |
| `grok-primary` | pinned (the alias is the same id) | `grok-4.7`, high | footer "Grok 4.7 (high)"; session `model_id` `grok-4.7`; `reasoning_effort` high | `grok-4.7-build` (response `model_id` and usage key) | Mismatch: drift under the ADR-0069 rule until TSK-139 settles whether `grok-4.7-build` names the same version |
| Grok line | model list | `grok models` | `grok-4.7` (default), `grok-4.7-build-fast`, `grok-4.6`, `grok-4.5` | not applicable | No newer version than `grok-4.7`; `grok-4.7-build-fast` is known and not routed, as ADR-0069 records |
| `codex-primary` first line | pinned, running review seats | `gpt-6-astra`, high | every turn context `gpt-6-astra`, high (seats `cx01`, `cx03`, `cx04`; below) | not recorded by Codex | Match on the requested and recorded identity |
| `codex-primary` second line | pinned, running review seat | `gpt-6-sol`, high | every turn context `gpt-6-sol`, high (seat `cx02`) | not recorded by Codex | Match on the requested and recorded identity |
| `codex-primary` last fallback | pinned | `gpt-5.6-sol`, high | not launched | not launched | Unavailable, reduced assurance (TSK-139) |
| light execution | pinned | `gpt-6-luna`, medium | not launched | not launched | Unavailable, reduced assurance (TSK-139) |

The Codex rows come from the session records of the four Codex review
seats already running in Herdr, read without prompting them. Each seat's
Codex process (0.155.1) was found from its Herdr pane, and its open session
files were read for the `model` and `effort` of every turn context:

| Seat | Session file | Turns | Model and effort in every turn | Latest turn |
|---|---|---|---|---|
| `cf-codeflow-review-cx01` | `rollout-2026-09-23T12-43-15-01a0cf26-66ce...` | 69 | `gpt-6-astra`, high | 2026-09-27 06:00 UTC |
| `cf-codeflow-review-cx02` | `rollout-2026-09-23T17-27-44-01a0d02a...` | 63 | `gpt-6-sol`, high | 2026-09-27 05:30 UTC |
| `cf-codeflow-tsk079-cx03` | `rollout-2026-09-27T00-46-41`, `00-46-52` and `01-28-27` | 14 | `gpt-6-astra`, high | 2026-09-27 05:29 UTC |
| `cf-codeflow-tsk080-cx04` | `rollout-2026-09-24T10-47-43-01a0d3e2...` | 37 | `gpt-6-astra`, high | 2026-09-27 05:49 UTC |

Each seat also holds an automatic approval reviewer thread whose turns
record `codex-auto-review` at low; that is Codex's own reviewer, not a
catalog seat. Codex records the model it was asked for in each turn and no
separate served id, so these rows match on requested and recorded identity
only. The canary's own Codex launches stopped at Codex's folder-trust
dialog and were not continued. `gpt-5.6-sol` and `gpt-6-luna` are
unavailable with reduced assurance and listed in TSK-139. The Codex lines'
aliases equal their pinned ids, so alias discovery for them is the same
evidence. Retired versions (`gpt-5.6-terra`, `grok-4.6`) were not launched.

## Model names in the tree

A search of every tracked file at this head for each catalog alias, pinned
id and selector, plus `grok-4.7-build-fast`:

| Where | Hits | Files |
|---|---|---|
| The catalog and its four mirrors | 300 | 5 |
| Evaluation kit fixtures (`fixtures.json` and its copies) | 10 | 5 |
| History paths (`docs/decisions/`, `docs/verification/`, `project-management/`, `docs/plan/`, `CHANGELOG.md`) | 68 | 21 |
| Listed preservation fixtures: the attribution patterns in `hooks/standards.rs` and `"model": "opus"` in the settings tests (`settings_merge.rs`, `settings/mod.rs`, `scaffold_test.rs`) | 10 | 4 |
| The fictional-name guard's own list of real names in `model_eval_contract.rs` | 2 | 1 |

No hit falls outside those homes. The real-tree scans
(`real_tree_scans_find_no_catalog_selector_or_stray_retired_selector`) pass.
The evaluation fixture hits are one sentence naming `gpt-5.6-sol`
(from `main`, `5e5eca4a0`), inside the exact fixture exclusion; ADR-0069
describes those fixtures as fictional, which that sentence is not.

## Ratchets

`git diff origin/main HEAD -- crates/codeflow-core/tests/artifact_budget_contract.rs`
adds semantic pins only (the finish-line and stop-scope sentences, the
autonomy pointers). No byte ceiling or skill ratchet changed, so EPC-018
records no ratchet move.

## Not verified

- A served model id for Codex: its session records carry the requested
  model only. `gpt-5.6-sol` and `gpt-6-luna` were not observed at all.
- The served effort of the Claude sessions: the header shows the requested
  effort, and the session record does not carry it.
- `codeflow test` as one command and rustdoc on this head.
- Linux, WSL2 and Windows hosts.

## Recheck at the line tip, 2026-09-28

The record above verifies `5a4d54a53`. Since then the line took TSK-151
(pull request 650), TSK-154 (669) and the planning changes 584, 647, 667 and
721; together they change `autonomy.md`, the orchestrator skill, the quality
contract, `pipeline.workflow.js` and two contract tests. Every check below
ran at `c72f80396` with a release binary built from that head in a private
target directory (`codeflow 3.0.0`, SHA-256 prefix `834438d3f492f3e6`).

| Check | Result | Evidence |
|---|---|---|
| `codeflow validate`, `validate --docs` | Pass | exit 0; 88 records clean; doc graph clean |
| `codeflow ci --base origin/main --head HEAD` | Pass | exit 0; 92 commits and 21 merges; the same two `breaking_watch_paths` warnings as above (`242a55a7`, `36234984`), both compatible additions |
| `doctor --check managed-drift` | Pass | exit 0; no managed-region drift |
| `doctor --check model-bindings` | Pass with warnings | exit 0; rows labeled illustrative; one "designated version with no full-suite record" warning per designated version and harness (nine), which TSK-139 closes |
| `artifact_budget_contract`, `catalog_craft_contract`, `manifest_consistency`, `model_catalog_surfaces`, `model_eval_contract` | Pass | 18, 13, 23, 10 and 47 passed |
| `models_cli`, `models_managed_catalog`, `orchestration_contract` | Pass | 15, 4 and 23 passed |
| `eval_kit.py validate-suite` | Pass | suite digest `sha256:d606789b0423aa57b12217a7a303a99d3d1eb1a92cc218e61f2937be3287b078` |

The real-tree model-name scan is part of `model_catalog_surfaces` and passes
at this head.

**Update journey and identity canary.** Their evidence at `5a4d54a53` still
holds at this head. `git diff --stat 5a4d54a53..c72f80396` changes 28 files:
`autonomy.md`, the orchestrator skill, the quality contract,
`pipeline.workflow.js` and their mirrors, two contract tests,
`.codeflow/manifest.json` and task records. It touches none of the five
copies of `current-ensemble.json` and no file under `crates/*/src`, so the
catalog the canary checked and the update mechanism the journey ran are
unchanged. The new hashes in `.codeflow/manifest.json` are covered by
`manifest_consistency`, which passes 23 of 23 above.

**Ratchet move.** The "no ratchet changed" finding above no longer holds.
TSK-154's `58031bc7d` raised the orchestrator skill ratchet and the routing
skill ceiling from 29 KiB (29,696 bytes) to 29 KiB + 512 (30,208 bytes). The
file measures 29,867 bytes at this head, against 29,682 on `main`. The reason
is audit rows H36 and H37: the operator ruled on 2026-09-27 that nothing key
is cut for bytes. The move is recorded in EPC-018's byte budget by this
change.

Not run at this head by this task: `codeflow test` as one command (the
primary runs it at the task branch head), `cargo test --workspace`, clippy,
rustdoc and coverage.

## EPC-018 section of the 3.0.0 release pull request body

The coordinator assembles the release body; this is EPC-018's part.

EPC-018 turns model routing into data and gives agents one written rule for
when to act and when to stop. Seats, product lines and duties now live in the
managed catalog, and one read-only command resolves them. The autonomy
reference is the only full list of what belongs to the operator.

- **Model catalog (ADR-0069).** Schema 5 carries families, product lines
  with ordered versions, seats, duties, effort floors and the identity drift
  rule. `codeflow models resolve` returns participants with pinned ids, or
  an open duty. `doctor --check model-bindings` labels its rows
  illustrative.
  - Breaking: the managed catalog moves from schema 4 to schema 5, and the
    binary no longer reads schema 4. Until an adopter runs `codeflow
    update`, `codeflow doctor --check model-bindings` fails and `codeflow
    models resolve` refuses on the older tree (TSK-085, marked `major` in
    the changelog).
- **Autonomy reference (ADR-0070).** `cf-method/references/autonomy.md`
  holds the finish line, the four-rung ladder, the operator-owned list, the
  trust prompt rule and settled dissent. The skills continue where they used
  to stop:
  - the primary merges green reviewed pull requests into unprotected
    integration branches;
  - "ready on local evidence" needs a completed result for every owed check;
  - the standing seats approve a reassignment after a seat loss.
  - TSK-154 restored risk tolerance and a material security boundary to the
    operator's list.
- **Evaluation.** Seventeen blind autonomy cases and the routing cases, each
  with a faulty control that fails.
- **Roster.** The operator designated the 2026-09-23 roster (Q2).
  Designation is not qualification: every seat version reads "designated,
  full suite not run" until TSK-139 runs.
- **Evidence.**
  - Validators, `ci`, both doctor checks, the targeted contract tests and
    the suite check pass at `c72f80396` (recheck above). `codeflow test` as
    one command is the primary's gate at the task branch head.
  - The native identity canary matched Claude by pinned id. The Codex
    identity matched on the requested and recorded model only, since Codex
    records no served id. Grok served `grok-4.7-build` for `grok-4.7`, which
    stays recorded as drift until TSK-139 settles it.
- **Not verified.** `gpt-5.6-sol` and `gpt-6-luna` were never launched. No
  seat version is qualified. The body-review duty runs once, on the release
  pull request: seat `codex-primary` review and seat `claude-primary`
  integrated judgment, seat `grok-primary` per the Q4 default, and the
  `design-and-editorial-review` second opinion.
- **Operator answers.**
  - Q2 was answered on 2026-09-23.
  - Q1 (automatic adoption of newer versions) and Q4 (Grok in every
    engineering review) stay open with their seed defaults: `manual`
    adoption, and today's `routing-policy.json` triggers.
  - EPC-015 D21 (stop before a new departure) is not carried: the operator
    closed the EPC-015 line on 2026-09-26, and only its holistic-fix
    doctrine moved on, through TSK-131.
  - TSK-083 was not selected. Its trust-prompt fix landed on the harness's
    own line through pull request 578.
- **Follow-ups.**
  - TSK-082, the Agent OS mirror: the routing half starts now, and the
    autonomy half waits for TSK-164.
  - TSK-164, `blocked`: the autonomy reference after the harness permission
    units.
  - TSK-139, `blocked`: native qualification after settings units 1 and 3,
    unit 5's D1 spike and the D10 fixture route.
  - TSK-125, on `integration/EPC-020-delivery-system`: record
    reconciliation after this line syncs from `main`. The record is not on
    this line; it resolves once the release branch merges both lines.
  - The workspace "Seats and models" and trust bullets, and
    `RELEASE-PLAN.md`, are updated on `docs/workspace` to point at the
    catalog.
