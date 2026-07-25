# Task-graph and verification-strength canary — 2026-07-25

## Claim and scope

This focused canary checks the five behaviors added by ADR-0040:

1. settle an explicit guarded task graph;
2. create Plan vN+1 for a material graph/ownership mutation;
3. keep ordinary in-node evidence under the approved plan and record it;
4. select property, mutation, and architecture checks only from evidence; and
5. select none of them for a bounded non-trigger.

It ran once per case in fresh disposable fixtures under native interactive
Claude and Codex surfaces. It is instruction-regression evidence, not a full
model/harness qualification or a promoted binding.

## Evaluated system

- CodeFlow branch: `feat/task-graph-verification`, based on `a953bbab`
- final suite digest after the canary correction:
  `sha256:fadc711d83ea84245cb7df1adc1f12a59ea04690db35becfd957d1a62428e800`
- Claude Code: 2.1.220; Fable 5 with high effort observed in each subject TUI;
  `auto` permission mode; distinct schema-v2 lifecycle session/prompt records
- Codex: fresh native Codex App subtasks requested as `gpt-5.6-sol` at high
  effort; the subject surface exposed only the GPT-5 Codex lineage, so the
  exact model and effort remain observed as unknown rather than inferred
- fixtures: evaluator-created, opaque, one-commit repositories under marked
  temporary run roots; no real remote, credentials, or production effects

## Results

| Case | Fable subject | Codex subject | Independent grade |
|---|---|---|---|
| `plan-settles-explicit-task-graph` | pass | pass | both graders approved |
| `graph-mutation-creates-new-version` | pass | pass | both graders approved |
| `in-node-detail-does-not-replan` | pass | initial fail; fresh rerun pass | Fable confirmed the correction |
| `verification-depth-routes-by-evidence` | pass | pass | both graders approved |
| `verification-depth-none-selected` | pass | pass | both graders approved |

The initial Codex in-node response classified the work correctly but did not
direct persistence of the evidence. Fable rejected that strict
`ledger_evidence_recorded` signal. The canonical task-graph and development
instructions now require classification **and** persistence with provenance;
mirrors and contract markers were regenerated. A fresh fixture and fresh Codex
subject then required all four classifications to be written to the execution
ledger before continuing under Plan v1. Fable re-graded that response as pass.

Codex independently graded the five lifecycle-bound Fable results and approved
all five. Fable independently graded the Codex results; after the corrected
fresh rerun, all five pass. No marker parroting, prohibited behavior, or
unresolved grader disagreement remained. Requested Codex model/effort is not
reported as observed identity.

## Retained provenance

Fable subject session IDs:

```text
graph settlement       cf2e13f9-f298-4a1d-adb0-574569017355
graph mutation         739aa367-2509-491c-a1b5-92324c1dc879
in-node detail         e7a3225f-1e03-43fd-8075-a8a3ca24c082
verification evidence  3a1f8150-88d8-4cc1-aace-6f766f80db34
verification none      4a76e50e-93cf-47a5-89b0-5618e4a8f831
```

Content digests:

```text
Fable graph settlement       b39298c9f39aa3af9a623ffb2bbc1e9d91de868c6784c9cf8e221525c2ddfc79
Fable graph mutation         144b92d644ebfc0c29360d7f6d38df7ea2fbdf73e341f917812f5dec541f3795
Fable in-node detail         ccfc4eaa85dbfd5eed7c6c14277dce968b922d7cde165cda09e2cfe8beb0009d
Fable verification evidence c8dd6a9a5789c387c295e077fa478cc56d5357115fec19811c1dc7c2291ddc31
Fable verification none     1b62cee7d6da0572668a81b3a6d86e702ba8c055c0e1059d97703e007a9d6a38
Codex graph settlement       98eec0f7a6e4877344ebbdd25af1e170d91ff8eb20a8c208150f0a49be2ee8a4
Codex graph mutation         311fe1d5de79beaebe1f7dd526eb215b78a3497c6e63769d458d647f23b69e47
Codex in-node initial        5ffc2d9868ab2a0808de75a78c78ab22942152cc597e1258db36d5bdb40fcc28
Codex in-node corrected      a21a119f1eea05cf9cbf191eac0473ec31cbe8989728b7d8475f1f29dbe7d3d5
Codex verification evidence 1c948f389810d5bc5427b8330cee1092bae468e1d2a9e8528cbfc2b748a1f04e
Codex verification none     334a6a868320dd05b7eee6bdd0fc6dc7db647f5350daa874f771bb772bc7a0ad
```

The repository retains digests and session references, not raw transcript
content or credentials.

## Targeted mutation evidence

The first task-local `cargo-mutants 27.1.0` run targeted only
`lint_tasks`, `find_task_cycle`, and `task_files`: 23 mutants, 18 caught, four
missed, and one timeout. The survivors exposed a missing diagnostic-line
assertion, duplicate self-cycle behavior, and redundant file-type logic; the
timeout exposed index-based traversal that could stall under mutation.

The implementation was simplified to direct ID-to-line lookup,
iterator-based traversal, non-following directory classification, and focused
record-parsing/graph-check helpers. Focused tests now pin flat and nested task
discovery, every directory-symlink boundary, precise cycle location, and
non-duplicated self-edge diagnostics. The first rerun caught all 17 generated
mutants. After the helper split, an expanded run covered every new helper: 20
mutants, 19 caught and one compile-time unviable replacement. Extending the
same campaign across the final symlink helper found one equivalent survivor:
non-following metadata already classifies a symlink as not-a-directory, making
an explicit second symlink condition redundant. Removing that condition left
23 mutants: 22 caught, one compile-time unviable, no missed mutant, and no
timeout. Outcome digests:

```text
initial  2b69bbeea3870ece0491ad6e43c7cc8da96308a7707ca7f97b3d67ae5b0c5da0
rerun    5f0f5c954d1bc1a492fb71dc50d5c73fb84eeb0fb752bab3f6329afa10a39b5f
final    952dc1bd3871689669bee943b8abb378e59717d0894c9ccd443a21ab160b20f4
symlink  f4e9fbbfb1c1ce75401642586df7c90731dda80a9ca8eeb3038b4919b3f0e24d
review   8c04336848a0c9940cefbd815b5f6df4ba299eb2ad542411b73fb57bb1af5872
```

The post-review run added the malformed-format-id decision guard to the same
slice: 24 mutants, 23 caught, one compile-time unviable, no missed mutant, and
no timeout.

The tool was installed under a task-local temporary prefix. It is not a
CodeFlow dependency or a consuming-project gate.
