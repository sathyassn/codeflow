# CodeFlow capabilities

<!-- WHAT layer: the registry of what the system does. Consult before building
     ("does this exist? what does it touch?"). Updated in the same PR that
     ships the work; the ship flow carries that discipline, and there is no
     blocking gate.
     The yaml fence under each capability is the machine-read authority: the
     portal generates the summary table and each definition table from it, and
     `status`, `orient` and `validate --docs` parse it. The four-column table
     in Architecture is a placeholder the adapter replaces at render time and
     must stay in fence order. Do not add any other registry table.
     The Technical section keeps one short operating summary per capability
     and links to the page that owns the detail; put operating detail on that
     owner page, not here.
     `validate --docs` enforces referential integrity (each entry's epics[] and
     adrs[] resolve to real files) and a non-empty verified_by on shipped
     entries; it does not verify that the test tags resolve, and it does not
     block an epic from closing.
     Statuses: planned, then building, then shipped, then deprecated (never
     delete).
     Keep this registry while it remains easy to scan; graduate to
     docs/capabilities/CAP-*.md when navigation or merge costs materially
     outweigh a single overview. -->

## Concept

**Check this registry before you build something new.**

The registry lists each CodeFlow capability once, with its status and the page
that owns its detail. It is for a contributor who needs to know whether a
capability already exists and what it touches. A capability's group says what
it is for. It does not say which tier installs it; the tier table on the
[adoption](adoption.md) page does.

## Architecture

The registry has one row per user-meaningful capability, keyed by its
capability id (CAP-###).

- Each rendered row is generated from that capability's yaml fence in
  Technical, so a row cannot disagree with its entry.
- In the portal the adapter replaces the table below with the generated one.
  The copy in this file is a placeholder the adapter needs; keep its rows in
  fence order.
- `codeflow validate --docs` checks referential integrity: every epic (EPC)
  and architecture decision record (ADR) an entry names must resolve to a real
  file, and a shipped entry needs a non-empty `verified_by`.
- `codeflow status` and `codeflow orient` also parse the fences.

`Area` is the `area` field of the yaml fence, naming the part of the codebase
that owns the capability. The six headings in Technical are the reading groups
instead, so the two answer different questions. Five rows read under one word
and carry another: CAP-017 reads under Scaffold with area `engine`, and
CAP-009, CAP-010, CAP-013 and CAP-015 read under Delegate and Present with area
`scaffold`.

| Capability | Name | Area | Status |
|---|---|---|---|
| CAP-001 | scaffold-init | scaffold | shipped |
| CAP-002 | scaffold-update | scaffold | shipped |
| CAP-012 | scaffold-customize | scaffold | shipped |
| CAP-017 | optional-agentic-estimation | engine | shipped |
| CAP-003 | git-policy-gates | engine | shipped |
| CAP-008 | remote-protect-doctor | engine | shipped |
| CAP-011 | security-redteam-review | engine | shipped |
| CAP-004 | test-gate | engine | shipped |
| CAP-005 | integrate | engine | shipped |
| CAP-007 | orient-session-summary | engine | shipped |
| CAP-006 | recall-registry | engine | shipped |
| CAP-009 | cross-vendor-delegation | scaffold | shipped |
| CAP-010 | duo-model-orchestration | scaffold | shipped |
| CAP-013 | model-binding-evaluation | scaffold | shipped |
| CAP-014 | transport-neutral-delegate-lifecycle | engine | building |
| CAP-015 | opt-in-documentation-portal | scaffold | shipped |
| CAP-016 | interactive-presentation-review | engine | building |

Statuses run planned, then building, then shipped, then deprecated. Entries are
deprecated, never deleted. Each entry's full field set, including the epics,
decisions and test tags that prove it, is the definition table beside its
summary in Technical. Two capabilities explain a subsystem large enough to
carry its own page:
[duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md) and
the [opt-in documentation portal](capabilities/CAP-015-opt-in-documentation-portal.md).
Both keep their registry entry here.

## Technical

Each capability below has its definition table, generated from its yaml fence,
then a short operating summary and a link to the page that owns the detail.

### Scaffold

#### scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs", "codeflow-cli tests/tier_floor_e2e.rs", "codeflow-cli tests/settings_presets.rs", "codeflow-cli tests/codex_config.rs"]
epics: [EPC-001, EPC-005, EPC-009, EPC-011, EPC-012, EPC-020]
adrs: [ADR-0019, ADR-0025, ADR-0026, ADR-0054, ADR-0055]
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline layer
into a repository. `--minimal` installs the complete four-plane enforcement
floor; `--standard` adds the method skills, reviewer agents, pipeline and docs
spine; `--full` adds `project-management/` (ADR-0019). Init is idempotent,
non-destructive and offline, and it preserves conflicting existing content for
explicit reconciliation. Re-running at a higher tier is an additive upgrade.
The harness starters it writes are executable policy, such as a guarded Codex
profile and a fail-closed Claude sandbox (ADR-0025, ADR-0026).

| Written file | What it holds |
|---|---|
| `AGENTS.md`, every tier | a moment-keyed rule map rendered from one kernel, `assets/base/rule-map.toml`: at most 12 one-line always rules, a "when you are about to" table, and pointers one hop away to the references in `.codeflow/rules/`; it leaves at least 16 KiB for the project section under Codex's 32 KiB limit |
| Claude starter | no ask rules: the actions the operator performs are denied by rules generated from one action table, which also generates the Codex command rules (ADR-0075) |
| Grok starter | a sandbox profile (ADR-0075) |

Detail: [adoption](adoption.md) and [harness posture](harness-posture.md).

#### scaffold-update

```yaml
id: CAP-002
name: scaffold-update
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::update", "cargo test scaffold::state::tests", "cargo test scaffold::settings_merge", "cargo test scaffold::region", "cargo test scaffold::manifest", "codeflow-cli tests/tier_floor_e2e.rs"]
epics: [EPC-001, EPC-005, EPC-012, EPC-018, EPC-020]
adrs: [ADR-0011, ADR-0019]
```

`codeflow update` refreshes managed scaffold files by ownership class.
Unmodified files are replaced. User-modified files get a three-way merge from
`.codeflow/.baseline/`, and a conflict produces a `.new` file and a report.
Managed regions are updated in place, and user-owned schema-versioned files
gain new keys with defaults. Update also installs in-tier manifest entries
that are missing on disk. It never clobbers and never silently skips.

| Policy scalar | On update |
|---|---|
| Still equal to the prior shipped default | moves to the new default and is reported |
| Differs from the prior shipped default | kept |
| No baseline recorded | the settings and policy are compared with the copies 2.1.0 shipped |

Detail: [adoption](adoption.md).

#### scaffold-customize

```yaml
id: CAP-012
name: scaffold-customize
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "cargo test doctor::tests::test_customization", "codeflow-core tests/scaffold_test.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004, EPC-005, EPC-009, EPC-010, EPC-011, EPC-012, EPC-020]
adrs: [ADR-0025, ADR-0044]
```

`/cf-customize` tailors a project after `codeflow init`, or after an update
that ships new defaults. It verifies the tools each flow the project uses
needs, then reconciles the project's real README, manifests, code and CI
against its project-owned docs, instructions and policy. It checks the
effective harness sandbox and approval posture instead of trusting comments.
It reports first and then applies fixes interactively through a PR. It never
auto-installs a tool or silently changes global harness settings. Detail:
[adoption](adoption.md).

#### optional-agentic-estimation

```yaml
id: CAP-017
name: optional-agentic-estimation
area: engine
status: shipped
verified_by: [estimate, estimate_cli, estimate_adoption_e2e, manifest_consistency]
epics: [EPC-006]
adrs: [ADR-0057]
```

Spec SPC-007 defines the optional `cf-estimate` method. A project confirms
adoption, or its decline is respected. Estimates carry evidence-anchored
grades, full-delivery scenarios and resource-feasible allocations. The
standard and full tiers manage the skill; profiles, forecasts and outcomes
stay project-owned. EPC-006 supplies the read-only allocation checker. This
registry does not establish calibrated delivery predictions. Detail:
[commands](cli.md).

### Enforce

#### git-policy-gates

```yaml
id: CAP-003
name: git-policy-gates
area: engine
status: shipped
verified_by: ["cargo test hooks::git_hook", "cargo test hooks::conflict_markers", "cargo test hooks::git_guard", "cargo test hooks::policy", "cargo test hooks::policy_schema", "cargo test hooks::standards", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/policy_cli.rs", "codeflow-cli tests/ci_cli.rs", "cargo test release_local", "codeflow-cli tests/release_journey.rs", "codeflow-cli tests/release_impact_corpus.rs", "scripts/test_release.py", "cargo test ledger::refusal", "cargo test ceremony::", "codeflow-cli tests/report_cli.rs", "codeflow-cli tests/refusal_journey.rs", "cargo test hooks::git_discard", "cargo test hooks::edit_guard", "cargo test security::outward", "cargo test security::interpreter"]
epics: [EPC-001, EPC-011, EPC-017, EPC-020]
adrs: [ADR-0002, ADR-0006, ADR-0007, ADR-0017, ADR-0062, ADR-0067]
```

Git discipline is enforced across four planes that read one config, the `git`
section of `.codeflow/policy.json`. Git client hooks and the in-session
`git-guard` give fast local feedback. CI re-runs the same checks through
`codeflow ci`, and together with remote branch protection it forms the
authoritative perimeter (ADR-0017). Rule levels are per-repository policy
values. The anti-bypass layer has no off switch: the strict policy validator,
the gate-context token for protected-branch advances, and the guard's refusal
of override variables set in-session. Secret scanning fails closed.
Agent sessions are judged by the landed policy on the remote, so a local
edit, commit or branch cannot relax the session checks.

Other checks on these planes:

| Check | Key and default | What it judges |
|---|---|---|
| Unresolved conflict markers (TSK-170) | `git.conflict_markers`, block | The lines a change adds to a text file: in pre-commit over the staged diff, and in `codeflow ci` over the range, which also catches a marker left while resolving `git rebase --continue`. A separator line counts only between an opening and a closing marker, so a Markdown heading underline passes. A file that must hold markers sets `conflict-marker-size` for its path in `.gitattributes` |
| En and em dashes (ADR-0067) | `policy_characters`, warn; CodeFlow's own policy sets block | Added lines. The scan skips a file only when its bytes equal the whole-file managed asset the running binary ships for that path, so unmodified scaffold content never trips it and a project record proves nothing |
| Pull request sections by change class (TSK-135) | `pr_sections` | Read from one checked merge-base tree diff that includes merge resolutions, deletions, both rename sides and file modes. A range of only regular Markdown under `docs/` or `project-management/`, outside every shared path set (product and watched contract paths from the checkout and the target, shipped templates, the record schema, dependency manifests, hooks, instructions and CI), needs Summary and Changes, under a mapped heading where the project accepted a mapping. An absent Release impact there reads as no impact unless a commit is marked breaking. The Release impact section is required only on a pull request into a protected branch or one that carries a breaking commit; elsewhere it is optional and checked when present (ADR-0076). A range that cannot be listed is code |
| Pull request Summary shape (TSK-218) | `git.pr_summary`, block | The one Summary section, under its mapped heading: one prose paragraph, then a list or a table, then at most one closing paragraph, read from the visible blocks only, so an HTML comment never supplies the lead or the list. It judges shape, never a word or sentence count (ADR-0071, note of 2026-10-03). It runs at warn while a kept PR template is diagnosed, and a trusted automation profile skips it |
| Local release state | `release.backend = "codeflow"` with `scripts/release.py`, for a project that adopted CodeFlow's release calculator | Pre-push runs the preflight for each pushed branch: a warning for a behaviour change with no pending entry, a block only for a push that breaks a tree its base kept valid. `codeflow integrate` runs the structural state check in its test stage. Both say what was not checked against the host, and the pull request's `release impact` job stays the gate. `codeflow ci` reads the Release impact block with the calculator's parser, and both pass one shared fixture set. The release jobs live in the project's own workflow, never in the managed CI file |

| Refusal record and report | Behaviour |
|---|---|
| Ledger `refusal` event | each operation a git hook or session guard stops appends one to the clone's ledger, naming the plane, the effective level and the rules, never the command; a finding at warn stops nothing and is not written |
| `codeflow report ceremony` | reads the events with the merge history over a window of pull requests or dates |
| Pull requests per logical change | from the merge history |
| Record status pull requests | on their own row |
| Review rounds asked of the host | `unknown` when the host cannot answer; the one host-backed read of SPC-013 R-103 |
| Refusals | from the ledger; `unknown` before the clone began recording |
| Baseline | pull requests 568 to 644 in `docs/verification/ceremony-baseline-2026-09-28.md`; the release checklist compares each release's window with it |

Detail: [enforcement planes](architecture/enforcement-planes.md) and
[policy reference](policy-reference.md).

#### remote-protect-doctor

```yaml
id: CAP-008
name: remote-protect-doctor
area: engine
status: shipped
verified_by: ["cargo test remote::", "cargo test doctor::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001, EPC-002, EPC-003, EPC-020]
adrs: [ADR-0002, ADR-0007, ADR-0025, ADR-0054]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider. On GitHub it requires a PR and green CI and blocks force-push and
deletion, then reports anything the plan tier cannot apply. `codeflow doctor`
runs twenty health checks on git hooks and CI, harness wiring, policy and
config, delegates and models, the repository and managed files, and
customization and test config.

| Doctor check | Reports |
|---|---|
| Always-loaded instruction size | a warning when the `AGENTS.md` chain Codex loads for any directory, root to nested, exceeds its 32 KiB limit |
| Reading sizes | the kernel, the per-task reading chain and each shipped skill against guideline numbers: information within them, a warning above them that names the detail to move behind a trigger, never a failure |

Doctor leaves live interactive account and tool canaries to the harness
(ADR-0023).
Detail: [troubleshooting](troubleshooting.md) and [commands](cli.md).

#### security-redteam-review

```yaml
id: CAP-011
name: security-redteam-review
area: engine
status: shipped
verified_by: ["cargo test hooks::policy", "codeflow-core tests/manifest_consistency.rs"]
epics: [EPC-003, EPC-012]
adrs: [ADR-0016]
```

The duo develop flow's mandatory security and red-team stage (ADR-0016). The
CI `security-review` job runs `osv-scanner` for software composition analysis
(SCA) over every lockfile ecosystem. It blocks CI only when the
`security_review` or `dep_audit` policy key is set to `block`; the shipped
default is warn. Secrets found by gitleaks always block, with exemptions
read from the trusted commit. The
`cf-security-reviewer` agent runs a two-vendor adversarial review; its
findings warn locally and force bounded rework, with the human merger as the
backstop (ADR-0007). Detail:
[enforcement planes](architecture/enforcement-planes.md).

### Verify

#### test-gate

```yaml
id: CAP-004
name: test-gate
area: engine
status: shipped
verified_by: ["cargo test testing::gate", "cargo test testing::config", "codeflow-core tests/integration_testing_setup.rs"]
epics: [EPC-001]
adrs: [ADR-0021, ADR-0031]
```

`codeflow test [--mode full|quick|essential] [--strict] [--since <rev>] [--all] [--only <targets>]`
runs the generic test engine against the targets in `.codeflow/test-config.json`
or a detected stack. With no stack detected it is a loud no-op that exits 0, and `--strict`
turns that into a non-zero exit for unattended callers. With a stack it is a
real gate, re-run in CI. Coverage thresholds count toward the verdict
(ADR-0021). `codeflow test setup` fills absent configs and never auto-replaces
populated ones.

| Pre-push case | Behaviour |
|---|---|
| The push set | `quick` names the targets with a `quick` mode; a manual run aliases it to `essential`, the lighter mode, when no target defines it |
| Every push | runs the push set plus `codeflow validate --docs` and `codeflow ci` on the pushed range, blocking by default under `test_gate_on_push`; `codeflow ci` judges every pushed branch with the strictly validated `.codeflow/policy.json` at the destination default branch's advertised tip, a candidate destination authority, not the known pull request target, and that policy's `test_gate_on_push` decides whether and at what level the check gates the push, so a branch that loosens its own policy is refused before the push; for a head that shares history with that tip, its commit checks run over everything the head adds to the tip, whatever history the destination already holds or target a task record declares, so a branch on an integration line has the line's inherited commits judged by the default branch's policy too, stricter than the hosted job for a pull request into the line; a head whose recorded history shares nothing with the tip, such as a deployment branch, is never diffed against it: a fast-forward of an existing branch keeps its own new commits and a new branch keeps the commits the destination does not hold yet, under that policy, named not default-target parity, while a shallow clone, a graft or a replace ref keeps the tip; no other branch is an authority, even a protected one, so a pull request into an integration line is judged locally by the default branch's policy and the host may differ; a policy there that this codeflow cannot read or validate refuses the push whether or not its range resolves; with no candidate authority (no answer, a failed fetch, no policy on the default branch) the hook says so and, where a range resolves, still runs the check at block level with the policy at the range's base; the tree checks, the quick targets and the release preflight follow the working copy's gate, and the commit-msg and other local hook stages read the working copy; hosted CI is the enforcement; it blocks on what it can see, and the test suite belongs to the full gate |
| Existing protected or `integration/` branch, fast-forward | the range starts at its advertised tip |
| Branch with a declared target | the merge base with that target's advertised tip, reading the target from the task record at the pushed commit; the hook names that target |
| Line rewrite, undeclared branch, unavailable target | the advertised-history fallback; an advertised target missing locally is noted |
| Unresolved range, sibling ref, or a dirty, sparse or submodule-incomplete checkout | named as left to CI |

| Full gate | Behaviour |
|---|---|
| Scheduling | a target may declare `requires`, `outputs`, `narrow` inputs and `exclusive`: producers and prerequisites run first, targets with no producer and consumer relation run in parallel up to `max_parallel`, a dependent of a red target reports "not run: prerequisite failed", and a missing or cyclic prerequisite fails the run before any target starts. The run names its candidate tree after the producers ran and fails when generation changed tracked bytes |
| Change-aware selection | a target is skipped only when its declared inputs are unchanged since a `--since` base that has a recorded green run for the same config; an unmatched path, a Rust, asset, build, config or workflow change, a rename or deletion, no base, and `--all` (the epic close) run every target, and the run prints what it selected and skipped and why |
| Split across runners | `--only` runs the named targets and their prerequisites, so one gate can run as parallel CI jobs; a limited run is recorded as not complete, so no single part serves as a green base, and a name that is not an enabled target of the mode is refused before any target starts. CodeFlow's own CI runs its full gate in four such parts behind one `codeflow gates` check, and `scripts/gate-parity.py` refuses a full-mode target no part runs |
| Preflight | refuses before any target when the temp directory is not writable, a configured lock cannot be taken, or a tool a selected target needs is missing |
| Concurrency | one full gate runs at a time on a machine; a second refuses, naming the holder |
| Cargo | a gate that runs cargo warns about a `CARGO_TARGET_DIR` outside the worktree |
| Progress | each target prints a start line and a completion line on stderr |
| Result artifact | a full run writes a result bound to its revision, tree hash and config digest and copies it to `~/.codeflow/gate-runs/<repo>/<run-id>/`, outside any worktree, for pull request bodies to cite. CodeFlow's own gate runs the Rust suite once, instrumented, and its journey check reads that run's results |
| Killed gate on Unix | the lock stays held until the target's process group exits, so no second gate runs beside the target |
| Killed gate on Windows | the target's job object ends its process tree with the gate; if Windows cannot put a suspended target in that job, it ends the target and reports a failed test before the target command runs |

Detail: [commands](cli.md).

#### integrate

```yaml
id: CAP-005
name: integrate
area: engine
status: shipped
verified_by: ["cargo test integrate::", "codeflow-cli tests/cli_flow.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow integrate <branch> [--into <target>]` is the sanctioned local landing
path for protected branches. Under a file lock it rebases the branch onto the
target, runs the test gate, then advances the target with
`git merge --ff-only`. No merge commit is added, and the target moves only
when every stage succeeds. A gate-context token lets the protected ref advance
only through integrate or a human-merged PR. Detail: [commands](cli.md) and
[enforcement planes](architecture/enforcement-planes.md).

### Remember

#### orient-session-summary

```yaml
id: CAP-007
name: orient-session-summary
area: engine
status: shipped
verified_by: ["cargo test hooks::orient", "cargo test hooks::guidance", "cargo test hooks::session_summary", "cargo test status::", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/codex_hooks.rs", "codeflow-cli tests/init_e2e.rs", "codeflow-core tests/rule_reinjection_update.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-001, EPC-004]
adrs: [ADR-0013, ADR-0044]
```

`codeflow orient` builds the session-start digest live. It is at most 30 lines
of pointers: the product line, branch and worktree state, work and capability
counts, recent ADR titles and gate status. The `session-summary` SessionEnd
hook appends a session record to the ledger that recall searches. Claude wires
both through `.claude/settings.json`, and an interactive Codex session gets
the same digest through `.codex/hooks.json` (ADR-0013).

Rules come back where they were lost or where they apply (TSK-128). One
advisory command, `session-orient`, is wired on `SessionStart` and
`UserPromptSubmit` and reads the event from the payload. Sizes are guidelines.

| Event | What it adds |
|---|---|
| Compaction, resume or Claude fork (source `compact`, `resume` or `fork`) | a guidance block after the digest, about 1.5 KB: the always rules by title, the "when you are about to" moments with their first pointer, and every skill and agent the tier installs, from the rule-map kernel and the scaffold manifest |
| A prompt that asks for a duration, a status or a complex explanation | one rule line, about 300 bytes |
| Any other prompt | nothing |

| Setting or host | Behaviour |
|---|---|
| `guidance.prompt_reminders` | defaults to `warn`; `off` or `allow` silences it; every path exits 0 |
| An older binary receiving the prompt event | prints its digest and exits 0, so a machine that has not upgraded loses the reminder, not the prompt |
| Claude and Codex | carry the text to the model |
| Grok Build 1.0.41 | has the events but ignores their output, so the Grok hook file wires only the guards |

`codeflow status` also prints a read-only cleanup inventory of linked worktrees
and unattached local branches (ADR-0044). Detail: [commands](cli.md).

#### recall-registry

```yaml
id: CAP-006
name: recall-registry
area: engine
status: shipped
verified_by: ["cargo test recall::", "cargo test registry::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001]
adrs: [ADR-0045]
```

`codeflow recall [--all] "<query>"` searches project memory: ledger events,
session summaries, ADRs, epics, tasks, frozen specs and capabilities. It uses
bundled SQLite full-text search (FTS5) and discloses coverage gaps. The
cross-repository view comes from `~/.codeflow/registry.json`, a locked
registry updated on every command run and read lazily at query time, with no
daemon. Detail: [commands](cli.md).

### Delegate

#### cross-vendor-delegation

```yaml
id: CAP-009
name: cross-vendor-delegation
area: scaffold
status: shipped
verified_by: ["cargo test doctor::tests::test_check_delegates", "cargo test --test orchestration_contract", "codeflow-core tests/herdr_host_contract.rs", "evals/skill-triggers/test_triggers.py", "docs/verification/host-neutral-duo-canary-2026-07-15.md", "docs/verification/herdr-primary-consult-canary-2026-08-30.md"]
epics: [EPC-002, EPC-011, EPC-012, EPC-018]
adrs: [ADR-0005, ADR-0018, ADR-0023, ADR-0036, ADR-0054, ADR-0059, ADR-0077]
```

Consult or delegate a unit of work to another vendor's coding CLI at the
process boundary, each under its own subscription auth. CodeFlow's gates judge
the output whoever wrote it (ADR-0005). Transport is interactive only and
stated once, in `cf-model-orchestrator/resources/routing/transport.md`
(ADR-0077): another family runs its own interactive CLI in a Herdr tab that
`cf-herdr` hosts, Codex on its app-server; the Codex plugin is an optional
fallback and tmux the last. Headless task execution is prohibited. Delegates
edit only inside a worktree on a feature branch, under the same gates. Detail:
[delegation](delegation.md).

#### duo-model-orchestration

```yaml
id: CAP-010
name: duo-model-orchestration
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "codeflow-core tests/model_eval_contract.rs", "codeflow-core src/model_qualification.rs", "codeflow-cli tests/orchestration_contract.rs", "cargo test validate::docs::tests", "cargo test models::task::tests", "docs/verification/task-graph-verification-canary-2026-07-25.md", "docs/verification/design-direction-canary-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test doctor::tests::test_check_delegates", "docs/verification/grok-host-duo-canary-2026-09-07.md", "cargo test workgraph::lifecycle", "cargo test workgraph::record_text", "codeflow-cli tests/record_lifecycle_journey.rs", "cargo test -p codeflow-cli --test models_cli", "cargo test -p codeflow-core --test model_catalog_surfaces", "cargo test -p codeflow-cli --test models_managed_catalog", "codeflow-cli tests/acceptance_cli.rs", "codeflow-cli tests/acceptance_journey.rs", "codeflow-cli tests/release_line_cli.rs"]
epics: [EPC-002, EPC-003, EPC-004, EPC-005, EPC-008, EPC-009, EPC-011, EPC-012, EPC-017, EPC-018, EPC-020]
adrs: [ADR-0015, ADR-0018, ADR-0023, ADR-0024, ADR-0025, ADR-0028, ADR-0030, ADR-0032, ADR-0034, ADR-0035, ADR-0040, ADR-0041, ADR-0042, ADR-0043, ADR-0044, ADR-0045, ADR-0046, ADR-0051, ADR-0054, ADR-0055, ADR-0060, ADR-0069]
```

`/cf-model-orchestrator` is the host-neutral entry for routed repository
work, decided by touched paths: adopter-facing paths, research that will
drive such a change, and plan, design, security or irreversible work. It
selects the smallest complete outcome mode, so research or planning work
stops before implementation. Planning, review and batch landing follow
[how work moves to main](delivery.md) (ADR-0076).
Both seats independently discover from the same immutable brief before
either sees the other's findings. Claude then drafts the one plan, Codex
challenges it, and both approve one version before implementation; material
product or visual work also records a `DESIGN_INTENT` in that plan
(ADR-0043, ADR-0051).

The work records a plan produces are judged by one core (SPC-013):

| Work record rule | Behaviour |
|---|---|
| Planning anchor | `codeflow work start` checks the anchor of the task the branch carries on any work prefix except `plan/` and `integration/`: the epic's planning change, or the record at head for a standalone task whose record arrives in its own pull request; CI applies the same read-only merge-base check once per pull request, and the per-commit hook no longer does. Both report at the `git.work_planning` level, `block` by default or `warn`. A reviewed but incomplete predecessor is accepted only through `--on TSK-NNN@<sha>` |
| Pull request class | with tracking on, `codeflow ci` classifies every pull request as tracked, an epic's planning-only range, an epic's integration line or an automation profile; one that names no task and no epic is refused, and a task may change only its own criteria, which CI prints for the reviewer |
| Readiness | one readiness core judges a task for `work next`, `work claim`, `work start`, `status`, `orient` and CI |
| Record status | `task status`, `epic status` and `spec status` move records only by legal transitions; the same judge rules on hand edits (`validate --docs --since <ref>`) and on each record a pull request changes |
| Deliverables | a task record lists each output and its home as a path in the project's structure under `## Deliverables`, after its Description, and an epic names the homes its tasks write or points to the project's structure authority; `validate --docs` warns, and never blocks, about an open task with no filled section and no path in its Description, and never reads a complete or cancelled record for it. The warning is advisory, so it errs toward silence: any token with `/` or `\` that is not a URL, a version or a slash word such as and/or counts as a path, as do a file name with an extension, a local link target and a common extensionless file such as `README`; in the section, a heading, an empty or checkbox-only item, `TODO` or the template's `<output>` and `<path>` count for nothing |
| Completion and release | a completion is bound to the reviewed commit, at a batch landing at the commit that introduced its block; on a release branch each change is judged where it was introduced |
| Release integration workflow | CodeFlow's own workflow imports verified epic-line tips into the release branch; it is not a task pull request gate and is not installed for adopters |

Detail:
[duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md).

#### model-binding-evaluation

```yaml
id: CAP-013
name: model-binding-evaluation
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/model_eval_contract.rs", "codeflow-core model_qualification + doctor::tests::model_bindings", "evals/model-artifacts/test_eval_kit.py", "codeflow-cli tests/live_eval_pack.rs (with the live delivery holdout checked out)", "cf-evaluate-model scripts/test_fake_effects.py + test_configure_fake_endpoint.py + test_security_sim.py", "codeflow-cli tests/init_e2e.rs", "evals/model-artifacts/test_retention_pack.py", "codeflow-cli tests/retention_pack.rs", "docs/verification/model-role-layered-verification-diagnostic-2026-07-25.md", "docs/verification/model-role-quality-diagnostic-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test -p codeflow-cli --test models_cli", "cargo test -p codeflow-core --test model_catalog_surfaces", "cargo test -p codeflow-cli --test models_managed_catalog"]
epics: [EPC-003, EPC-004, EPC-005, EPC-008, EPC-010, EPC-011, EPC-012, EPC-017, EPC-018, EPC-020]
adrs: [ADR-0027, ADR-0032, ADR-0034, ADR-0039, ADR-0041, ADR-0042, ADR-0044, ADR-0054, ADR-0055, ADR-0060, ADR-0069]
```

`/cf-evaluate-model` qualifies a new model or version, native harness release,
permission profile, or material CodeFlow instruction change as the complete
system users will run. The standard and full skill carries stable requirement
IDs, balanced regression and capability cases, a native-interactive run
protocol, and a standard-library tool for validation, scoring, baseline
comparison and fail-closed cleanup. Focused diagnostic packs do not qualify a
model binding.

| Evaluation part | What it does |
|---|---|
| `guidance-retention` pack | uses the scripted multi-turn case kind: the fixture supplies warm-up turns and only the probe turn is graded. Three hard probes (a landing time asked for in a plan, a status report, a multi-part explanation) and their paired negatives each run in a fresh arm and an after-compaction arm on the Claude host. Offline checks prove the kit, not live behaviour |
| `expected.files` and `expected.effects` | `eval_kit.py grade` checks them against the work a session left. This does not prove CLI use, readiness checks or review before completion; those need the harness's own record of the session (TSK-116) |
| Graded suites | live outside the shipped kit and binary: a public development suite in `evals/grader-dev/`, and the live delivery holdout of SPC-013 R-105 on a private archive ref that is never merged, recorded in `evals/holdout.json` |
| Trial results | a timed-out or errored session is kept and graded as a failure, and a pack result keeps every trial (TSK-111) |
| Qualification tooling | `evals/qualification/` uses the kit's isolated HOME, TMPDIR and CODEFLOW_HOME, records the exact requested launch flags and compares metadata snapshots of declared directories: a new, changed or removed entry or an incomplete read invalidates the observation. It does not observe writes outside those directories. Its judge controls cover process-round and R-105 evidence; native judge calibration and subject trials stay separate from tooling tests |
| Closeout fixture | creates the clean landed, dirty active and unmerged worktrees its inventory describes |

Detail: [model and harness upgrades](model-upgrades.md).

#### transport-neutral-delegate-lifecycle

```yaml
id: CAP-014
name: transport-neutral-delegate-lifecycle
area: engine
status: building
verified_by: ["cargo test delegate::", "codeflow-cli tests/delegate_cli.rs", "codeflow-cli tests/delegate_pty_stress.rs", "cargo test doctor::tests::test_delegate_roundtrip", "docs/verification/delegate-lifecycle-canary-2026-07-23.md", "docs/verification/delegate-lifecycle-canary-2026-07-24.md"]
epics: [EPC-002, EPC-019]
adrs: [ADR-0036, ADR-0037]
```

`codeflow delegate init|arm|wait` and the `hook delegate-turn --state-dir` mode
drive a delegated harness turn through durable, owner-only protocol records
instead of tmux signalling. The binary never launches a harness or delivers a
prompt; the host does both, and the current event adapter is Claude hooks.
`arm` records one turn as the SHA-256 of the exact prompt bytes, and
acceptance requires a matching `UserPromptSubmit`. `wait` has a stable exit
contract. Poison is durable, and recovery is a new run in a fresh directory.

| Background task notice | Check |
|---|---|
| Admission | a task the turn backgrounds, such as a Workflow or a background Bash command, can finish after the turn's Stop, and Claude Code then submits its notice as a new prompt; the hook admits it without an armed turn only as a continuation of the current turn |
| Prompt hook | the prompt is exactly one `<task-notification>` envelope, the turn stopped with no other continuation open, and the transcript shows the tool call that launched that task within the turn or one of its continuations; the record under `turns/<turn>/continuations/<task>/` keeps the notice's `prompt_id` and the SHA-256 of its bytes |
| Stop carrying that `prompt_id` | Claude Code 2.1.283 gives the prompt hook nothing that tells a real notice from typed text, so the proof waits for this Stop: the transcript must record the prompt with origin `task-notification`, `promptSource` `system` and `turnOrigin` `task_notification`, bytes matching the digest, and an earlier queue enqueue of the same bytes |
| Failure | a typed copy, a changed body or a missing entry poisons the run and writes no result |
| Residual risk | the model may act on a forged notice within that continuation turn; the check keeps it from being recorded as a clean result |

The status stays building while broader native-platform and stress evidence
is open. Detail: [delegation](delegation.md).

### Present

#### opt-in-documentation-portal

```yaml
id: CAP-015
name: opt-in-documentation-portal
area: scaffold
status: shipped
verified_by: ["codeflow test --mode full --strict", "cargo test scaffold::portal", "cargo test validate::portal", "codeflow-core tests/manifest_consistency.rs", "node --test docs-portal/tests/adapter.test.mjs", "npm run build --prefix docs-portal", "codeflow validate --portal docs-portal", "docs/verification/tsk-009-docs-portal/", "docs/verification/tsk-020-portal-ownership.md"]
epics: [EPC-005, EPC-007, EPC-013, EPC-014, EPC-016, EPC-017]
adrs: [ADR-0048, ADR-0058]
```

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the pinned Starlight and Pagefind repository guide. Ordinary
initialization never installs it. Setup preserves user-owned configuration,
and runtime drift or a collision stops all portal writes.
`codeflow portal transfer --confirm` hands runtime maintenance to the project.
The adapter generates pages, Markdown twins, `llms.txt` and search output from
one clean committed snapshot, and it fails closed rather than publish mixed
content. `codeflow validate --portal` runs no project code, writes nothing,
and checks the generated evidence. Detail:
[opt-in documentation portal](capabilities/CAP-015-opt-in-documentation-portal.md).

#### interactive-presentation-review

```yaml
id: CAP-016
name: interactive-presentation-review
area: engine
status: building
verified_by: ["cargo test -p codeflow-present", "cargo test -p codeflow-cli --test present_cli", "npm run check:browser --prefix crates/codeflow-present/web", "codeflow-core tests/manifest_consistency.rs"]
epics: [EPC-005, EPC-014, EPC-016, EPC-020]
adrs: [ADR-0049, ADR-0050, ADR-0052, ADR-0053]
```

`codeflow present` turns a closed, versioned JSON and Markdown document into
one bounded local review surface with anchored feedback. The standard and full
tiers supply the `cf-present` authoring skill and its schemas. Simple answers
stay in chat, and durable docs belong to the portal. The runtime serves an
authenticated loopback-only page to an isolated browser profile and keeps
immutable revisions and append-only feedback in owner-private state. It
retains ambiguous state rather than deleting it. The `diagram` block was
removed with Mermaid: new input carrying one is refused with its conversion
named, and a revision stored with one loads read only with a notice and its
escaped source. The status stays building
until task TSK-007 records the full native platform and browser matrix.
Detail: [present architecture](architecture/present.md) and
[present guide](present-guide.md).
