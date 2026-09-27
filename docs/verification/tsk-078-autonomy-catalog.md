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
