# codeflow — capabilities

<!-- WHAT layer: the registry of what the system does. Consult before building
     ("does this exist? what does it touch?"). Updated in the same PR that
     ships the work — the discipline is the ship flow, not a blocking gate.
     `validate --docs` enforces referential integrity (each entry's epics[] and
     adrs[] resolve to real files) and a non-empty verified_by on shipped
     entries; it does not verify that the test tags resolve, and it does not
     block an epic from closing.
     Statuses: planned → building → shipped → deprecated (never delete).
     Keep this registry while it remains easy to scan; graduate to
     docs/capabilities/CAP-*.md when navigation or merge costs materially
     outweigh a single overview. -->

**Check here before building: every capability has one row, one status, and one home.**

| Capability | Name | Area | Status | Epics | ADRs | Purpose |
|---|---|---|---|---|---|---|
| CAP-001 | scaffold-init | scaffold | shipped | EPC-001, EPC-005, EPC-009, EPC-011, EPC-012 | ADR-0019, ADR-0025, ADR-0026, ADR-0054, ADR-0055 | `codeflow init` lays the discipline layer into any repo at the chosen tier |
| CAP-002 | scaffold-update | scaffold | shipped | EPC-001, EPC-005, EPC-012 | ADR-0011, ADR-0019 | `codeflow update` refreshes managed scaffold files by ownership class |
| CAP-003 | git-policy-gates | engine | shipped | EPC-001, EPC-011 | ADR-0002, ADR-0006, ADR-0007, ADR-0017 | Git discipline enforced across four planes reading one policy file |
| CAP-004 | test-gate | engine | shipped | EPC-001 | ADR-0021, ADR-0031 | `codeflow test` runs the configured or detected targets as one verdict |
| CAP-005 | integrate | engine | shipped | EPC-001 | — | `codeflow integrate` is the local rebase to test to fast-forward landing |
| CAP-006 | recall-registry | engine | shipped | EPC-001 | ADR-0045 | `codeflow recall` searches project memory over FTS5 and a cross-repo registry |
| CAP-007 | orient-session-summary | engine | shipped | EPC-001, EPC-004 | ADR-0013, ADR-0044 | Session-start digest and session-end ledger record for Claude and interactive Codex |
| CAP-008 | remote-protect-doctor | engine | shipped | EPC-001, EPC-002, EPC-003 | ADR-0002, ADR-0007, ADR-0025, ADR-0054 | `remote protect` applies branch protection; `doctor` runs fifteen health checks |
| CAP-009 | cross-vendor-delegation | scaffold | shipped | EPC-002, EPC-011, EPC-012 | ADR-0005, ADR-0018, ADR-0023, ADR-0036, ADR-0054, ADR-0059 | Consult or delegate a unit of work to another vendor's coding CLI |
| [CAP-010](capabilities/CAP-010-duo-model-orchestration.md) | duo-model-orchestration | scaffold | shipped | EPC-002, EPC-003, EPC-004, EPC-005, EPC-008, EPC-009, EPC-011, EPC-012 | ADR-0015, ADR-0018, ADR-0023, ADR-0024, ADR-0025, ADR-0028, ADR-0030, ADR-0032, ADR-0034, ADR-0035, ADR-0040, ADR-0041, ADR-0042, ADR-0043, ADR-0044, ADR-0045, ADR-0046, ADR-0051, ADR-0054, ADR-0055, ADR-0060 | `/cf-model-orchestrator` is the host-neutral default for non-trivial repository work |
| CAP-011 | security-redteam-review | engine | shipped | EPC-003, EPC-012 | ADR-0016 | The mandatory security and red-team stage, bound at three planes |
| CAP-012 | scaffold-customize | scaffold | shipped | EPC-003, EPC-004, EPC-005, EPC-009, EPC-010, EPC-011, EPC-012 | ADR-0025, ADR-0044 | `/cf-customize` is the post-init tailoring walk-through over tools and project facts |
| [CAP-013](capabilities/CAP-013-model-binding-evaluation.md) | model-binding-evaluation | scaffold | shipped | EPC-003, EPC-004, EPC-005, EPC-008, EPC-010, EPC-011, EPC-012 | ADR-0027, ADR-0032, ADR-0034, ADR-0039, ADR-0041, ADR-0042, ADR-0044, ADR-0054, ADR-0055, ADR-0060 | `/cf-evaluate-model` qualifies a model, harness, profile, or instruction change |
| CAP-014 | transport-neutral-delegate-lifecycle | engine | building | EPC-002 | ADR-0036, ADR-0037 | `codeflow delegate init/arm/wait` drives a delegated turn through durable records |
| [CAP-015](capabilities/CAP-015-opt-in-documentation-portal.md) | opt-in-documentation-portal | scaffold | shipped | EPC-005, EPC-007, EPC-013, EPC-014 | ADR-0048, ADR-0058 | `codeflow portal setup` adopts the exact-pinned Starlight and Pagefind guide |
| [CAP-016](capabilities/CAP-016-interactive-presentation-review.md) | interactive-presentation-review | engine | building | EPC-005, EPC-014 | ADR-0049, ADR-0050, ADR-0052, ADR-0053 | `codeflow present` turns a catalog document into one bounded local review surface |
| CAP-017 | optional-agentic-estimation | engine | shipped | EPC-006 | ADR-0057 | The optional cf-estimate method plus a read-only allocation checker |

Statuses run planned → building → shipped → deprecated; entries are
deprecated, never deleted. Four capabilities have outgrown a single
overview and are graduated to `docs/capabilities/`; their registry entry
keeps the machine-read block, a summary, and the link.

## CAP-001 — scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs", "codeflow-cli tests/tier_floor_e2e.rs", "codeflow-cli tests/settings_presets.rs", "codeflow-cli tests/codex_config.rs"]
epics: [EPC-001, EPC-005, EPC-009, EPC-011, EPC-012]
adrs: [ADR-0019, ADR-0025, ADR-0026, ADR-0054, ADR-0055]
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline
layer into any repo. Enforcement is the floor; the tiers scale project-management
(ADR-0019): `--minimal` installs the complete four-plane enforcement floor (all
five git-hook shims, the CI check, the in-session `git-guard`/`exec-guard` +
orient/summary hooks in `.claude/settings.json`, the `.codex/` starter, and
`.grok/hooks/codeflow.json`, the armed `policy.json`, `.gitignore` including
`.worktrees/`, and a lean `AGENTS.md` + `CLAUDE.md`);
`--standard` adds the method (Claude/agent skills, reviewer agents, the pipeline)
and the six-layer docs spine and full contract; `--full` adds
project-management/. Idempotent, non-destructive, offline (assets embedded via
rust-embed), with bootstrap grace, husky/hooksPath detection, and a printed
per-file report. Standard/full reports close with `/cf-customize`, pointing at
the consuming project's product, architecture, agent context, harness settings,
and tools; minimal does not advertise an uninstalled method skill. Re-running
at a higher tier is an additive upgrade. Init writes into the current working
directory; standard document destinations are fixed, and conflicting existing
content is preserved for explicit reconciliation. A lower-tier request does
not undo a recorded higher tier. The installed tier describes what init lays
down, not whether recognizable historical CodeFlow task records already make
durable work tracking active.

The harness starters are executable policy, not prompt-only guidance. Codex
ships one current-schema guarded permission profile with public research,
loopback UI testing, reviewer-subagent escalation review, workspace key-file
denies, the current primary seat's configured fallback, and the default
secret-bearing environment filter pinned on. Claude-hosted plugin turns pass
the current ensemble's model and effort explicitly so they cannot inherit a
different user default. Claude
ships a fail-closed sandbox on macOS, Linux, and WSL2, public web/tool access,
raw model/cloud credential removal for sandboxed Bash, and ask rules for
destructive source-control operations. A sandbox failure may request an
auto-classified unsandboxed retry
only for a trusted installed tool that needs host state; this enables the
official Codex plugin without granting a general bypass. Project `acceptEdits`
remains the ordinary fallback because
repository-scoped Auto is intentionally ignored (ADR-0025, ADR-0026,
ADR-0029).

## CAP-002 — scaffold-update

```yaml
id: CAP-002
name: scaffold-update
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::update", "cargo test scaffold::state::tests", "cargo test scaffold::settings_merge", "cargo test scaffold::region", "cargo test scaffold::manifest", "codeflow-cli tests/tier_floor_e2e.rs"]
epics: [EPC-001, EPC-005, EPC-012]
adrs: [ADR-0011, ADR-0019]
```

`codeflow update` refreshes managed scaffold files by ownership class:
unmodified files are replaced, user-modified files get a 3-way merge from
`.codeflow/.baseline/` (conflicts produce `.new` + report), managed regions
(AGENTS.md markers, settings.json codeflow keys) are surgically updated, and
user-owned schema-versioned files only gain new keys with defaults. It also
installs any manifest entry that is in-tier but missing on disk — so a file that
became in-tier since the last install (e.g. an old `--minimal` repo gaining the
enforcement floor under ADR-0019) is reconciled into place and recorded, not just
refreshed. Managed files, baselines, and state use synced same-directory atomic
replacement; state reads fail on non-absence errors, and malformed
`[scaffold].ignore` values identify their key or index. Never clobbers, never
silently skips.

## CAP-003 — git-policy-gates

```yaml
id: CAP-003
name: git-policy-gates
area: engine
status: shipped
verified_by: ["cargo test hooks::git_hook", "cargo test hooks::git_guard", "cargo test hooks::policy", "cargo test hooks::policy_schema", "cargo test hooks::standards", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/policy_cli.rs", "codeflow-cli tests/ci_cli.rs"]
epics: [EPC-001, EPC-011]
adrs: [ADR-0002, ADR-0006, ADR-0007, ADR-0017]
```

Git discipline enforced across four planes reading one config (the `git`
section of `.codeflow/policy.json`). Two give fast local feedback — the git
client hooks (pre-commit secret scan + staged-.env, commit-msg
format/attribution/emoji, pre-merge-commit and reference-transaction
protected-branch merge/ref rules, pre-push branch naming and protected-branch
rules) and the Claude `git-guard` PreToolUse hook (in-session immediacy, plus
the `gh pr merge` and PR-body checks no client hook can see). Two are the
authoritative perimeter — CI, which re-runs the same checks through the
`codeflow ci` binary (the same Rust functions the hooks call, so no inline
drift, portable across CI hosts via thin GitHub/GitLab/Bitbucket/generic
wrappers — ADR-0017), and remote branch protection (`codeflow remote protect`).
Rule *levels* are policy values, user-flippable per repo (`off`/`warn`/`allow`/
`block`), so a team tunes strictness to its risk tolerance — including the
PR-body structure gate (`git.pr_sections`: required sections present with real
content, a code-touching range carries the testing sections, and leftover
template placeholders draw a warn naming their line). The structural
anti-bypass layer is not flippable, by design: the strict policy validator (an
invalid file fails loud rather than silently reverting to defaults), the schema
drift-guard pinning the registry to the policy struct, the gate-context token
that admits a protected-branch advance only through integrate or a PR, and the
git-guard's refusal to honor override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate
tokens) set in-session — these preserve the boundary itself and have no off
switch. Configurable levels tune how strict a rule is; the invariants keep the
rules from being routed around. The policy file is
discoverable and strict from the binary alone: `codeflow policy explain`
renders every key's type, default, and valid values from a schema registry
drift-guarded against the policy struct; `codeflow policy show` prints the
effective values and their source; and a present-but-invalid file fails
loudly — naming each offending key, its value, and the valid set — at the
commit-msg hook, `codeflow ci`, and `codeflow validate`, instead of silently
reverting every key to the built-in defaults.
Secret scanning fails closed if libgit2 cannot traverse the complete staged
diff. Hook stdin read failures remain advisory but print an explicit degraded
ref-check warning instead of passing silently.

## CAP-004 — test-gate

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
engine against configured targets (`.codeflow/test-config.json`) or runtime stack
detection (`quick` is an alias for `essential`, the lighter mode). No stack
detected is a loud no-op (exit 0) so the bootstrap/early-setup path stays green;
`--strict` escalates that no-op to a non-zero exit for scripted/unattended callers
(CI, the pipeline verify gate) where "ran nothing" must not read as a pass. With a
stack it is a real gate, wired into pre-push via the `test_gate_on_push` policy and
re-run in CI. File and aggregate coverage thresholds all contribute to the gate
verdict; `changed_files` rules are rejected until an explicit comparison base is
available (ADR-0021). Captured stdout/stderr is bounded and reports truncation.
`codeflow test setup` safely fills absent/empty root-detected configs, lists and
applies release-embedded templates, and appends explicit targets. Detection is
root-only; monorepos declare package `cwd` targets explicitly. Populated or
malformed configs are never auto-replaced, and a malformed configured pre-push
gate is a violation rather than a skip. Legacy `structural` blocks remain
loadable but doctor warns that the public gate does not enforce them; setup no
longer emits dormant rules.

## CAP-005 — integrate

```yaml
id: CAP-005
name: integrate
area: engine
status: shipped
verified_by: ["cargo test integrate::", "codeflow-cli tests/cli_flow.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow integrate <branch> [--into <target>]` is the sanctioned local
landing path for protected branches: a flock-guarded rebase → test →
fast-forward primitive. It rebases the branch onto the target, runs the test
gate, then advances the target with `git merge --ff-only` — a linear
fast-forward that adds no merge commit; the target ref moves only when every
stage succeeds. The sequence runs under a gate-context token the git hooks and
git-guard verify, so the protected ref advances only through integrate or a
human-merged PR — a raw `git merge` or manual ref move stays blocked. A human
retains the local override path (`CODEFLOW_HUMAN_OVERRIDE=1`), an env the
git-guard never honors for an agent. After landing, clean linked worktrees that
have the target checked out are reset to the tested tip; dirty worktrees and a
failed checkout restoration are reported as partial-success warnings.

## CAP-006 — recall-registry

```yaml
id: CAP-006
name: recall-registry
area: engine
status: shipped
verified_by: ["cargo test recall::", "cargo test registry::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001]
adrs: [ADR-0045]
```

`codeflow recall [--all] "<query>"` searches project memory — ledger events,
session summaries, ADRs, epics, tasks, frozen specs, and capabilities — via
bundled SQLite FTS5, with coverage gaps disclosed. The cross-repo view comes from
`~/.codeflow/registry.json`, a flock'd registry upserted on every command run;
a lazy view at query time, not a daemon.
Recursive source discovery skips directory symlinks and obeys depth/count
budgets. Reversible path encoding supplies index identity, while lossy text is
reserved for display.

## CAP-007 — orient-session-summary

```yaml
id: CAP-007
name: orient-session-summary
area: engine
status: shipped
verified_by: ["cargo test hooks::orient", "cargo test hooks::session_summary", "cargo test status::", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/codex_hooks.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-001, EPC-004]
adrs: [ADR-0013, ADR-0044]
```

`codeflow orient` generates the session-start digest live (≤30 lines,
pointers not content): product one-liner, branch/worktree state, work and
capability counts, recent ADR titles, gate status, paths to read more. The
`session-summary` SessionEnd hook appends a session record to the ledger —
recall's zero-ceremony corpus. Both are wired through `.claude/settings.json`;
the orient digest is also wired for Codex via `.codex/hooks.json` (SessionStart,
all sources — ADR-0013), so an **interactive** Codex session opens with the same
digest and re-orients after a compaction (`source=compact`). One handler serves
both harnesses — plain-text stdout each injects as session context — so there is
no per-harness duplication. Headless `codex exec` does not fire project hooks
(ADR-0008), so this is an interactive-session aid; the `codex_hooks` test pins the
JSON wiring, while live firing rests on Codex's documented hooks contract.

`codeflow status` also emits a read-only cleanup inventory for linked
worktrees and unattached local branches. Against the locally known target it
distinguishes clean ancestry/patch-equivalent resources, dirty work, and
unproven work; the report performs no mutation and never substitutes for an
active-owner check (ADR-0044).

## CAP-008 — remote-protect-doctor

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
provider (GitHub via `gh api`: require PR + green CI, block force-push and
deletion) with a legible report of anything the plan tier cannot apply.
`codeflow doctor` runs fifteen health checks — hooks, Claude wiring, Codex wiring, Grok wiring, config,
permissions, network, delegates, qualified model bindings, delegate round-trip, repo integrity, CI
perimeter, managed-region
drift, consuming-project customization, and test config. The Grok check reports
structural `.grok/hooks` wiring and the one-time `/hooks-trust` step; it does
not inspect trust state (ADR-0054). The customization
check remains quiet for minimal/non-method repos, warns while product,
architecture, or AGENTS sentinels remain, and points to `/cf-customize`. The
delegates check inspects both directions:
Codex auth/MCP, Claude plugin/MCP, and tmux; it explicitly leaves live
interactive account/tool canaries to the harness (ADR-0023). The
delegate-roundtrip check drives the installed binary through a synthetic
schema-v2 lifecycle (CAP-014) and fails when any transition breaks.
The binding check validates user-owned approved full-suite records against the
trusted harness capability catalog, compares externally observable harness
versions and declared settings-source digests, and keeps live model/effort
unknown unless native evidence proves it (ADR-0039).

## CAP-009 — cross-vendor-delegation

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
process boundary, each under its own subscription auth, with CodeFlow's gates
judging the output author-agnostically (ADR-0005). Transport is
interactive-only: from Claude Code prefer the official `codex-plugin-cc` plugin
(wrapping the codex app-server), with a qualified official native client
fallback under ADR-0059; from codex
the interactive `claude` CLI through the schema-v2 lifecycle. When
`HERDR_ENV=1`, `cf-herdr` hosts that TTY in a named tab (cwd-matched resume,
no hijack of other panes); tmux is the degraded host. Herdr `idle`/`done` is
not turn completion — Stop and StopFailure hook completion and schema-v2
waits remain the signal rather than pane stability. The 2026-08-30 canary exercised named-tab consult hosting. The 2026-09-07
canary exercised Grok-started schema-v2 armed `send-text` on Herdr (consult
posture, macOS arm64) and a Grok-started Codex Herdr consult thread. The
`codeflow hook delegate-turn` adapter validates a unique run and private path,
writes immutable `0600` terminal evidence, and signals only its scoped waiter;
exact retries recover signalling without rewriting. The transport-neutral
schema-v2 lifecycle (CAP-014, ADR-0036) is this adapter's file-polled
successor under verification; the legacy `--result` mode described here
remains byte-compatible until a later major release. Headless task execution
(`codex exec`, `claude -p`) is prohibited; the earlier headless tier and the
Antigravity `agy` delegate tier (headless-only) are retired. It ships as the
`cf-delegate`, `cf-consult`, and `cf-herdr` skills (mirrored to `.agents/skills`), and an
optional single-vendor `consult` pipeline stage — plus one deterministic
`delegates` doctor check for both lanes; delegates edit only inside a worktree
on a feature branch, so
pre-commit, commit-msg, the test gate, and the independent review pass
constrain them exactly as they do the orchestrating harness. No engine model
router is added; the binary owns only the deterministic terminal adapter.

## CAP-010 — duo-model-orchestration

```yaml
id: CAP-010
name: duo-model-orchestration
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "codeflow-core tests/model_eval_contract.rs", "codeflow-core src/model_qualification.rs", "codeflow-cli tests/orchestration_contract.rs", "cargo test validate::docs::tests", "cargo test models::task::tests", "docs/verification/task-graph-verification-canary-2026-07-25.md", "docs/verification/design-direction-canary-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test doctor::tests::test_check_delegates", "docs/verification/grok-host-duo-canary-2026-09-07.md"]
epics: [EPC-002, EPC-003, EPC-004, EPC-005, EPC-008, EPC-009, EPC-011, EPC-012]
adrs: [ADR-0015, ADR-0018, ADR-0023, ADR-0024, ADR-0025, ADR-0028, ADR-0030, ADR-0032, ADR-0034, ADR-0035, ADR-0040, ADR-0041, ADR-0042, ADR-0043, ADR-0044, ADR-0045, ADR-0046, ADR-0051, ADR-0054, ADR-0055, ADR-0060]
```

`/cf-model-orchestrator` is the host-neutral default for every non-trivial
repository task: research, analysis, planning, design, implementation,
debugging, security, substantive documentation, review, or verification. It
selects the smallest complete outcome mode, so research/planning-only work
settles an evidenced artifact and stops before implementation.
Both seats independently research, analyze risks, and draft complete plans from
the same immutable brief before either sees the other's conclusions. This is an
anti-anchoring requirement: Codex must not be reduced to critiquing a plan
Claude has already supplied. After both drafts exist,
Claude leads design. The host reconciles a versioned plan whose task rows name
the responsible primary, actual binding-or-route executor, execution mode,
routing reason and provenance, available usage evidence with freshness or an
explicitly unknown value, and cross-lineage reviewer.
Both seats approve those assignments before implementation; changing ownership,
scope, lineage, isolation, or a named reviewer invalidates the approvals, while
a permitted primary-owned executor change inside that boundary does not. Each
actual executor first-verifies its unit, the responsible primary inspects and
accepts it, and a lineage different from the actual author's reviews it
independently. The selected `claude-judgment-primary` owns integrated Claude quality judgment
without claiming independent review of its own unit.
For material product, UX, interaction, or visual-direction work, the
orchestrator loads `cf-design` and records a proportionate `DESIGN_INTENT`
inside that same plan. Cosmetic changes may collapse as not applicable,
bounded established-system work may conform, new surfaces settle a direction,
and materially open novel work compares two or three viable directions first.
Language/voice and appearance modes are resolved only where applicable from
project evidence: localized quality needs localized evidence, mode claims need
rendered preference and persistence evidence, and CodeFlow utility defaults do
not become product design authority. After selection, further variants require
one named unresolved material choice and stop when it is settled. Material
references and assets retain proportionate authority, rights/privacy,
transformation, and product-use provenance, while material feedback names the
exact reviewed version in Plan vN+1 rather than a parallel design database.
The Claude judgment role leads intent, owns real design implementation and
fidelity, and directly executes until a matching Claude design route is
scoped-qualified. Candidate design routes are limited to controlled disposable
qualification fixtures; scoped-qualified routes execute only exact evidenced
tuples and never acquire direction or fidelity-approval authority. Another
family designs only under an explicit task-specific operator override—Claude
absence alone is not one. Codex challenges feasibility and fidelity, and both
approve the exact plan. Review anchors blocking design
findings in the accepted brief, intent, accessibility target, or observed
behavior rather than taste. The design-direction eval pack covers this
selection, operator precedence, evidence-grounded design-choice review,
bounded refinement, sourcing/privacy, reviewed-version retention, distinct
evidenced product voices, localization honesty, utility/product isolation,
appearance-mode behavior, accessibility, and rendered fidelity
(ADR-0043, ADR-0051).

Full detail: [CAP-010 — duo-model-orchestration](capabilities/CAP-010-duo-model-orchestration.md).

## CAP-011 — security-redteam-review

```yaml
id: CAP-011
name: security-redteam-review
area: engine
status: shipped
verified_by: ["cargo test hooks::policy", "codeflow-core tests/manifest_consistency.rs"]
epics: [EPC-003, EPC-012]
adrs: [ADR-0016]
```

The duo develop flow's mandatory security / red-team stage, bound at three
planes that copy the git-rules model. Deterministic floor: the CI
`security-review` job runs `osv-scanner` (stack-agnostic SCA over every lockfile
ecosystem — the universal floor today; per-stack scanners are a future
extension), gated by the `security_review` (whole-job umbrella) and `dep_audit`
(SCA sub-gate) policy keys beside `secret_scan`, with the advisory blocking when
either is `block`. Model layer: the `cf-security-reviewer` agent runs a
dual-vendor adversarial red-team (Claude defender lens + codex assume-breach
attacker, ADR-0005) across seven axes mapped to OWASP Top 10:2025 / OWASP LLM
Top 10:2025 / CWE Top 25 (2025), emitting structured `SecurityFinding` /
`SecurityVerdict` output; the pipeline `security` stage sets its verdict from the
severity+confidence block rule. The deterministic floor hard-blocks CI only
when `security_review` or `dep_audit` is hardened to `block` (the shipped
default is warn); secrets via gitleaks always block, and a High+ severity
filter is future work alongside the per-stack scanners. Model-reasoned findings
warn locally and force bounded rework, with the human merger as the backstop
for judgment a machine cannot adjudicate (ADR-0007).

## CAP-012 — scaffold-customize

```yaml
id: CAP-012
name: scaffold-customize
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "cargo test doctor::tests::test_customization", "codeflow-core tests/scaffold_test.rs", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004, EPC-005, EPC-009, EPC-010, EPC-011, EPC-012]
adrs: [ADR-0025, ADR-0044]
```

`/cf-customize` is the post-init tailoring walk-through: after `codeflow init`
(or a `codeflow update` that ships new defaults to decide), it runs a
flow-aware tool preflight — verifying the tools needed by each flow the
project actually uses (git and the harness for every flow; the `codex-plugin-cc`
plugin, codex, and its MCP servers for a Claude-hosted duo; Claude CLI/MCP,
tmux, and the completion-hook canary for a Codex-hosted duo or reverse consult;
the stack's test toolchain) —
then reconciles the consuming project's actual README, manifests, code, and CI
against its project-owned artifacts (`docs/product.md`,
`docs/architecture.md`, common instructions/commands in `AGENTS.md`, only
Claude-specific differences in `CLAUDE.md`, policy levels, and runtime model
routing). It locates existing human-approved editorial voice guidance and
examples without inventing a persona, and points agents to the project-owned
canonical source when one exists. It verifies effective Claude/Codex sandbox,
network, live-search, and approval posture rather than trusting comments. It
also canaries the task's
research, GitHub, stack, coverage/security, browser/UI, design, and
project-specific MCP tools with brokered authentication and no raw secrets.
For concurrent browser work it additionally settles the project's resource
contract: isolated profile/context, ports only where a listener or local
service needs them, namespaced test data, run-scoped artifacts, and owned
teardown/release verification. CodeFlow never supplies universal port numbers
or reuses the operator's browser profile.

Analysis-then-propose: one prioritized report first, then fixes applied
interactively on a working branch through a PR; it never auto-installs a tool or
silently changes global harness settings. `codeflow init` prints the next step,
and doctor keeps a nudge visible while scaffold sentinels remain. Ships as the
`cf-customize` skill, mirrored across
`.claude/skills`, `.agents/skills`, and the `assets/base` scaffold source.
The walk-through maps existing source, build, runtime, data/trust, and work
authorities before proposing changes. It preserves credible native layouts and
does not treat an external tracker or foreign tasks directory as reason to
duplicate status or force a CodeFlow workgraph; pre-init cwd, fixed paths, and
collisions remain explicit onboarding decisions.
Release customization discovers the project's existing units, version and
curated-note sources, single calculator, candidate checks and publication
authority. It reuses that process or offers a verified opt-in starting process;
independent package/version domains and calendar schemes remain project-owned.
The shared release-policy reference routes customization, shipping and review
without turning CodeFlow scaffold metadata into the consumer's version.

## CAP-013 — model-binding-evaluation

```yaml
id: CAP-013
name: model-binding-evaluation
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/model_eval_contract.rs", "codeflow-core model_qualification + doctor::tests::model_bindings", "evals/model-artifacts/test_eval_kit.py", "cf-evaluate-model scripts/test_fake_effects.py + test_configure_fake_endpoint.py + test_security_sim.py", "codeflow-cli tests/init_e2e.rs", "docs/verification/model-role-layered-verification-diagnostic-2026-07-25.md", "docs/verification/model-role-quality-diagnostic-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004, EPC-005, EPC-008, EPC-010, EPC-011, EPC-012]
adrs: [ADR-0027, ADR-0032, ADR-0034, ADR-0039, ADR-0041, ADR-0042, ADR-0044, ADR-0054, ADR-0055, ADR-0060]
```

`/cf-evaluate-model` qualifies a new model/version, native harness release,
permission profile, or material CodeFlow instruction change as the complete
system users will run. The standard/full managed skill carries stable hard
requirement IDs, source-marker traceability, balanced regression/capability
cases, exact fixture overlays, a native-interactive run protocol, and a
standard-library tool for deterministic validation, materialization, scoring,
baseline comparison, and fail-closed cleanup.
The `release-policy` diagnostic pack tests compatibility judgment, misleading
commit labels, compatible/no-release counterexamples, independent version
domains, project-owned tool/adoption choices and stale or conflicting
publication evidence. Deterministic suite/grader checks are distinct from
retained native trials; focused packs do not qualify a model binding or prove
universal detection. The `responsible-autonomy` pack registers ten standard-
tier synthetic cases for privacy/delegation, outbound authority/retry,
pressured incident/security work, and identity/fair decisions. Effectful cases
use a finite loopback simulator; its journal stays outside the subject tree and
setup records both materialized and configured tree digests. This is not a full
promotion or a real-service authorization test.

Full detail: [CAP-013 — model-binding-evaluation](capabilities/CAP-013-model-binding-evaluation.md).

## CAP-014 — transport-neutral-delegate-lifecycle

```yaml
id: CAP-014
name: transport-neutral-delegate-lifecycle
area: engine
status: building
verified_by: ["cargo test delegate::", "codeflow-cli tests/delegate_cli.rs", "codeflow-cli tests/delegate_pty_stress.rs", "cargo test doctor::tests::test_delegate_roundtrip", "docs/verification/delegate-lifecycle-canary-2026-07-23.md", "docs/verification/delegate-lifecycle-canary-2026-07-24.md"]
epics: [EPC-002]
adrs: [ADR-0036, ADR-0037]
```

`codeflow delegate init|arm|wait` plus the schema-v2 `hook delegate-turn
--state-dir` mode drive a delegated harness turn through durable, owner-only
protocol records instead of tmux signalling. The binary never launches a
harness or delivers a prompt — host delivery stays outside CodeFlow; the
current event adapter is Claude hooks, and the legacy `--result` mode
(CAP-009) remains byte-compatible.

The operational sequence: `init` creates the `0700` state directory (which
must sit outside any Git worktree) and prints the path of the task-scoped
Claude settings it generates, wiring SessionStart, UserPromptSubmit, Stop,
and StopFailure back to the hook. The host launches the harness with those
settings and the caller runs `wait --until ready` (a `startup` SessionStart;
any other source — resume, clear, compact, fork — poisons the run). `arm`
accepts non-empty canonical UTF-8 text with internal LF line endings, no
terminal line break, and no other control characters, then records one turn as the
SHA-256 of those exact
prompt bytes (≤ 1 MiB). The
host delivers that same file after a bounded input-settle, and acceptance
requires a session-bound `UserPromptSubmit` whose hook-payload prompt matches
the digest. `wait --until terminal`
returns the turn's result record on stdout — Stop with the bounded assistant
message, or StopFailure with bounded error payloads — after which the host
consumes it and removes the state directory. Request and acceptance records
carry digests and identifiers, never the prompt text itself. Raw schema-v2
hook input is capped at 32 MiB before parsing, in addition to the smaller
decoded-field limits.

The exit contract is stable: `wait` exits 0 on the observed state (terminal
completed), 10 on a failed terminal, 11 on a poisoned, unsafe, or invalid
run, 124 on timeout, and 130 when interrupted — an interrupt after
acceptance poisons the run; the hook exits 2 to make the harness block a
rejected prompt submission or any unreadable/oversized schema-v2 input, and
exits 1 on other failures; `init`/`arm` exit 0 or 1. Every command must reuse
the exact state-directory path string given to `init` — run binding
regenerates the task settings and requires exact equality with the stored
ones — and every state mutation serializes on a single run lock with bounded
(one-second) acquisition. `prompt_id` is
validated and must agree across acceptance and terminal records; a session
without one takes the recorded pre-2.1.196 compatibility path, limited to a
single turn. Native Windows fails closed (use WSL2). Poison is durable and
write-once; recovery is a new run in a fresh directory.

Status is building: the engine surface and its unit/CLI/doctor tests landed,
and the `delegate-roundtrip` doctor check requires the rebuilt CLI to be
installed before it can pass. The dated PR1 canary record covers the active
Stop-hook set, a live Unicode normalization case, `Stop.prompt_id` binding,
AskUserQuestion, and permission-response routing on the available macOS arm64
host. The reusable sibling-hook rejection procedure, full fake-TUI stress
matrix, and broader native-platform evidence remain PR2/release gates and are
not claimed complete.

## CAP-015 — opt-in-documentation-portal

```yaml
id: CAP-015
name: opt-in-documentation-portal
area: scaffold
status: shipped
verified_by: ["codeflow test --mode full --strict", "cargo test scaffold::portal", "cargo test validate::portal", "codeflow-core tests/manifest_consistency.rs", "node --test docs-portal/tests/adapter.test.mjs", "npm run build --prefix docs-portal", "codeflow validate --portal docs-portal", "docs/verification/tsk-009-docs-portal/", "docs/verification/tsk-020-portal-ownership.md"]
epics: [EPC-005, EPC-007, EPC-013, EPC-014]
adrs: [ADR-0048, ADR-0058]
```

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the exact-pinned Starlight and Pagefind repository-guide utility. The
portal build requires Node 22.19.0 or newer. The aggregate CI gate runs on Node
26.4.0 — the presentation renderer's pin — and its full strict target installs, checks, builds, and validates the
dogfood portal; the portal-local `.node-version` and the Windows adapter-test
lane pin Node 24.18.0.
The starter is absent from ordinary initialization, materializes offline once at
the selected root, preserves user-owned configuration, and participates in
replace-only updates without pristine runtime copies or source merges. Runtime
drift and collisions stop all portal writes; missing project-owned configuration
remains absent. `codeflow portal transfer --confirm` preserves edits and
intentional deletions while handing ongoing runtime maintenance to the project.
V2 adoption state freezes transferred-from provenance and declares the current
generator identity; v1 evidence stays strict in both ownership modes. Legacy
journals recover before migration, and unknown or changed baseline content
blocks both migration and transfer without deletion. The source-authority adapter generates disposable pages, Markdown
twins, `llms.txt`, search output, and a versioned evidence manifest from one
clean committed snapshot. Its pinned GFM pipeline, bounded no-follow reads,
literal bounded Git pathspec batches, committed-blob authority,
configured-tree source coverage, semantic source-root-relative routes,
reserved generated-public namespaces, locale-independent ordering, workflow
lease, and recoverable publication transaction fail closed before mixed or
active content can be claimed. Index flags cannot hide changed runtime,
configuration, source, token, or media bytes. The locked installer verifies the
exact lifecycle-script inventory and disables dependency scripts. Install,
build, preview, browser, and Git subprocesses receive only a small non-secret
environment allowlist; inherited provider, cloud, package-registry credential,
and loader variables never cross the boundary, while Git also rejects inherited
configuration. A broken current Markdown blob yields only a
bounded, visible, non-searchable current-source error page; Git history and
previous generated data are never republished.
`codeflow validate --portal <directory>` executes no project code and writes
nothing; it independently checks bounded path, hash, complete configured-source
coverage, exact source-derived identity/relationship, error-page, provenance, version, raster-dimension,
output-coverage, twin, and
`llms.txt` claims.
It compares generator evidence with the declared identity, retaining managed
release pins but accepting a genuinely renamed transferred generator. Matching
evidence is not runtime attestation or a substitute for rendered qualification.

Full detail: [CAP-015 — opt-in-documentation-portal](capabilities/CAP-015-opt-in-documentation-portal.md).

## CAP-016 — interactive-presentation-review

```yaml
id: CAP-016
name: interactive-presentation-review
area: engine
status: building
verified_by: ["cargo test -p codeflow-present", "cargo test -p codeflow-cli --test present_cli", "npm run check:browser --prefix crates/codeflow-present/web", "codeflow-core tests/manifest_consistency.rs"]
epics: [EPC-005, EPC-014]
adrs: [ADR-0049, ADR-0050, ADR-0052, ADR-0053]
```

`codeflow present` turns a closed versioned JSON+Markdown document into one
bounded local review surface. The standard/full scaffold supplies the
cross-harness `cf-present` authoring skill, canonical document/token/history
schemas, representative assets, and proportional routing: simple answers stay
in chat; a complex explanation, comparison, plan, decision, evidence set, diff,
or review uses the utility only when coherent visual inspection or anchored
feedback materially helps. Agents author **this session's** subject into the
catalog; the runtime owns chrome, themes, and the Comment system. The
design-exploration board is craft reference, not a document to clone. Durable
docs belong to `cf-docs-portal`.

Full detail: [CAP-016 — interactive-presentation-review](capabilities/CAP-016-interactive-presentation-review.md).

## CAP-017 — optional-agentic-estimation

```yaml
id: CAP-017
name: optional-agentic-estimation
area: engine
status: shipped
verified_by: [estimate, estimate_cli, estimate_adoption_e2e, manifest_consistency]
epics: [EPC-006]
adrs: [ADR-0057]
```

SPC-007 defines the optional cf-estimate method: active project-specific offer,
confirmed adoption or respected decline, evidence-anchored grades, full-delivery
scenarios and resource-feasible allocations. Standard/full skills are managed;
profiles, forecasts and outcomes stay project-owned and link existing authority.
EPC-006 supplies the read-only allocation checker, installed-path tests and
native diagnostic evidence, including retained failures and a focused consent
repair. Implementation readiness and predictive usefulness remain separate;
this registry does not establish calibrated delivery predictions.
