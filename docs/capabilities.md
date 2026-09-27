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
epics: [EPC-001, EPC-005, EPC-009, EPC-011, EPC-012]
adrs: [ADR-0019, ADR-0025, ADR-0026, ADR-0054, ADR-0055]
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline layer
into a repository. `--minimal` installs the complete four-plane enforcement
floor; `--standard` adds the method skills, reviewer agents, pipeline and docs
spine; `--full` adds `project-management/` (ADR-0019). Init is idempotent,
non-destructive and offline, and it preserves conflicting existing content for
explicit reconciliation. Re-running at a higher tier is an additive upgrade.
The harness starters it writes are executable policy, such as a guarded Codex
profile and a fail-closed Claude sandbox (ADR-0025, ADR-0026). Detail:
[adoption](adoption.md) and [harness posture](harness-posture.md).

#### scaffold-update

```yaml
id: CAP-002
name: scaffold-update
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::update", "cargo test scaffold::state::tests", "cargo test scaffold::settings_merge", "cargo test scaffold::region", "cargo test scaffold::manifest", "codeflow-cli tests/tier_floor_e2e.rs"]
epics: [EPC-001, EPC-005, EPC-012]
adrs: [ADR-0011, ADR-0019]
```

`codeflow update` refreshes managed scaffold files by ownership class.
Unmodified files are replaced. User-modified files get a three-way merge from
`.codeflow/.baseline/`, and a conflict produces a `.new` file and a report.
Managed regions are updated in place, and user-owned schema-versioned files
only gain new keys. Update also installs in-tier manifest entries that are
missing on disk. It never clobbers and never silently skips. Detail:
[adoption](adoption.md).

#### scaffold-customize

```yaml
id: CAP-012
name: scaffold-customize
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "cargo test doctor::tests::test_customization", "codeflow-core tests/scaffold_test.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004, EPC-005, EPC-009, EPC-010, EPC-011, EPC-012]
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
verified_by: ["cargo test hooks::git_hook", "cargo test hooks::git_guard", "cargo test hooks::policy", "cargo test hooks::policy_schema", "cargo test hooks::standards", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/policy_cli.rs", "codeflow-cli tests/ci_cli.rs"]
epics: [EPC-001, EPC-011, EPC-017]
adrs: [ADR-0002, ADR-0006, ADR-0007, ADR-0017, ADR-0067]
```

Git discipline is enforced across four planes that read one config, the `git`
section of `.codeflow/policy.json`. Git client hooks and the in-session
`git-guard` give fast local feedback. CI re-runs the same checks through
`codeflow ci`, and together with remote branch protection it forms the
authoritative perimeter (ADR-0017). Rule levels are per-repository policy
values. The anti-bypass layer has no off switch: the strict policy validator,
the gate-context token for protected-branch advances, and the guard's refusal
of override variables set in-session. Secret scanning fails closed. Detail:
[enforcement planes](architecture/enforcement-planes.md) and
[policy reference](policy-reference.md).

#### remote-protect-doctor

```yaml
id: CAP-008
name: remote-protect-doctor
area: engine
status: shipped
verified_by: ["cargo test remote::", "cargo test doctor::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001, EPC-002, EPC-003]
adrs: [ADR-0002, ADR-0007, ADR-0025, ADR-0054]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider. On GitHub it requires a PR and green CI and blocks force-push and
deletion, then reports anything the plan tier cannot apply. `codeflow doctor`
runs fifteen health checks covering hooks, harness wiring, config, delegates,
model bindings, repository integrity, the CI perimeter and test config. It
leaves live interactive account and tool canaries to the harness (ADR-0023).
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
default is warn. Secrets found by gitleaks always block. The
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

`codeflow test [--mode full|quick|essential] [--strict]` runs the generic test
engine against the targets in `.codeflow/test-config.json` or a detected
stack. With no stack detected it is a loud no-op that exits 0, and `--strict`
turns that into a non-zero exit for unattended callers. With a stack it is a
real gate, wired into pre-push by the `test_gate_on_push` policy and re-run in
CI. Coverage thresholds count toward the verdict (ADR-0021). `codeflow test
setup` fills absent configs and never auto-replaces populated ones. Detail:
[commands](cli.md).

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
verified_by: ["cargo test hooks::orient", "cargo test hooks::session_summary", "cargo test status::", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/codex_hooks.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-001, EPC-004]
adrs: [ADR-0013, ADR-0044]
```

`codeflow orient` builds the session-start digest live. It is at most 30 lines
of pointers: the product line, branch and worktree state, work and capability
counts, recent ADR titles and gate status. The `session-summary` SessionEnd
hook appends a session record to the ledger that recall searches. Claude wires
both through `.claude/settings.json`, and an interactive Codex session gets
the same digest through `.codex/hooks.json` (ADR-0013). `codeflow status` also
prints a read-only cleanup inventory of linked worktrees and unattached local
branches (ADR-0044). Detail: [commands](cli.md).

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
epics: [EPC-002, EPC-011, EPC-012]
adrs: [ADR-0005, ADR-0018, ADR-0023, ADR-0036, ADR-0054, ADR-0059]
```

Consult or delegate a unit of work to another vendor's coding CLI at the
process boundary, each under its own subscription auth. CodeFlow's gates judge
the output whoever wrote it (ADR-0005). Transport is interactive only. From
Claude Code it uses the official `codex-plugin-cc` plugin, with a qualified
native client fallback (ADR-0059). From Codex it uses the interactive `claude`
CLI through the delegate lifecycle. When `HERDR_ENV=1`, `cf-herdr` hosts that
terminal in a named Herdr tab. Headless task execution is prohibited. Delegates
edit only inside a worktree on a feature branch, under the same gates. Detail:
[delegation](delegation.md).

#### duo-model-orchestration

```yaml
id: CAP-010
name: duo-model-orchestration
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "codeflow-core tests/model_eval_contract.rs", "codeflow-core src/model_qualification.rs", "codeflow-cli tests/orchestration_contract.rs", "cargo test validate::docs::tests", "cargo test models::task::tests", "docs/verification/task-graph-verification-canary-2026-07-25.md", "docs/verification/design-direction-canary-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test doctor::tests::test_check_delegates", "docs/verification/grok-host-duo-canary-2026-09-07.md"]
epics: [EPC-002, EPC-003, EPC-004, EPC-005, EPC-008, EPC-009, EPC-011, EPC-012, EPC-017]
adrs: [ADR-0015, ADR-0018, ADR-0023, ADR-0024, ADR-0025, ADR-0028, ADR-0030, ADR-0032, ADR-0034, ADR-0035, ADR-0040, ADR-0041, ADR-0042, ADR-0043, ADR-0044, ADR-0045, ADR-0046, ADR-0051, ADR-0054, ADR-0055, ADR-0060]
```

`/cf-model-orchestrator` is the host-neutral default for every non-trivial
repository task. It selects the smallest complete outcome mode, so research or
planning work stops before implementation.
Both seats independently research, analyze risks, and draft complete plans from
the same immutable brief before either sees the other's conclusions.
Claude then leads design. The host reconciles a versioned plan that names
each task's primary, actual executor and cross-lineage reviewer, and both
seats approve it before implementation. Material product or visual work also
records a `DESIGN_INTENT` in that plan (ADR-0043, ADR-0051). Detail:
[duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md).

#### model-binding-evaluation

```yaml
id: CAP-013
name: model-binding-evaluation
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/model_eval_contract.rs", "codeflow-core model_qualification + doctor::tests::model_bindings", "evals/model-artifacts/test_eval_kit.py", "cf-evaluate-model scripts/test_fake_effects.py + test_configure_fake_endpoint.py + test_security_sim.py", "codeflow-cli tests/init_e2e.rs", "docs/verification/model-role-layered-verification-diagnostic-2026-07-25.md", "docs/verification/model-role-quality-diagnostic-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004, EPC-005, EPC-008, EPC-010, EPC-011, EPC-012, EPC-017]
adrs: [ADR-0027, ADR-0032, ADR-0034, ADR-0039, ADR-0041, ADR-0042, ADR-0044, ADR-0054, ADR-0055, ADR-0060]
```

`/cf-evaluate-model` qualifies a new model or version, native harness release,
permission profile, or material CodeFlow instruction change as the complete
system users will run. The standard and full skill carries stable requirement
IDs, balanced regression and capability cases, a native-interactive run
protocol, and a standard-library tool for validation, scoring, baseline
comparison and fail-closed cleanup. Focused diagnostic packs do not qualify a
model binding. Detail: [model and harness upgrades](model-upgrades.md).

#### transport-neutral-delegate-lifecycle

```yaml
id: CAP-014
name: transport-neutral-delegate-lifecycle
area: engine
status: building
verified_by: ["cargo test delegate::", "codeflow-cli tests/delegate_cli.rs", "codeflow-cli tests/delegate_pty_stress.rs", "cargo test doctor::tests::test_delegate_roundtrip", "docs/verification/delegate-lifecycle-canary-2026-07-23.md", "docs/verification/delegate-lifecycle-canary-2026-07-24.md"]
epics: [EPC-002]
adrs: [ADR-0036, ADR-0037]
```

`codeflow delegate init|arm|wait` and the `hook delegate-turn --state-dir` mode
drive a delegated harness turn through durable, owner-only protocol records
instead of tmux signalling. The binary never launches a harness or delivers a
prompt; the host does both, and the current event adapter is Claude hooks.
`arm` records one turn as the SHA-256 of the exact prompt bytes, and
acceptance requires a matching `UserPromptSubmit`. `wait` has a stable exit
contract. Poison is durable, and recovery is a new run in a fresh directory.
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
epics: [EPC-005, EPC-014, EPC-016]
adrs: [ADR-0049, ADR-0050, ADR-0052, ADR-0053]
```

`codeflow present` turns a closed, versioned JSON and Markdown document into
one bounded local review surface with anchored feedback. The standard and full
tiers supply the `cf-present` authoring skill and its schemas. Simple answers
stay in chat, and durable docs belong to the portal. The runtime serves an
authenticated loopback-only page to an isolated browser profile and keeps
immutable revisions and append-only feedback in owner-private state. It
retains ambiguous state rather than deleting it. The status stays building
until task TSK-007 records the full native platform and browser matrix.
Detail: [present architecture](architecture/present.md) and
[present guide](present-guide.md).
