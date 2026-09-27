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

## CAP-001 — scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs", "codeflow-cli tests/tier_floor_e2e.rs", "codeflow-cli tests/settings_presets.rs", "codeflow-cli tests/codex_config.rs"]
epics: [EPC-001, EPC-005, EPC-009, EPC-011, EPC-012, EPC-020]
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
and the six-layer docs spine; `--full` adds project-management/. Every tier's
`AGENTS.md` is a moment-keyed rule map rendered from one kernel
(`assets/base/rule-map.toml`): at most 12 one-line always rules, a "when you
are about to" table and pointers one hop away to the references in
`.codeflow/rules/`, leaving at least 16 KiB for the project section under
Codex's 32 KiB limit. Idempotent, non-destructive, offline (assets embedded via
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
epics: [EPC-001, EPC-005, EPC-012, EPC-020]
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
epics: [EPC-001, EPC-011, EPC-017, EPC-020]
adrs: [ADR-0002, ADR-0006, ADR-0007, ADR-0017, ADR-0067]
```

Git discipline enforced across four planes reading one config (the `git`
section of `.codeflow/policy.json`). Two give fast local feedback — the git
client hooks (pre-commit secret scan + staged-.env, commit-msg
format/attribution/emoji and the ADR-0067 em and en dash check,
pre-merge-commit and reference-transaction protected-branch merge/ref rules,
pre-push branch naming and protected-branch rules) and the Claude
`git-guard` PreToolUse hook (in-session immediacy, plus
the `gh pr merge` and PR-body checks no client hook can see). Two are the
authoritative perimeter — CI, which re-runs the same checks through the
`codeflow ci` binary (the same Rust functions the hooks call, so no inline
drift, portable across CI hosts via thin GitHub/GitLab/Bitbucket/generic
wrappers — ADR-0017), and remote branch protection (`codeflow remote protect`).
Rule *levels* are policy values, user-flippable per repo (`off`/`warn`/`allow`/
`block`), so a team tunes strictness to its risk tolerance — including the
PR-body structure gate (`git.pr_sections`: required sections present with real
content, a code-touching range carries the testing sections, and leftover
template placeholders draw a warn naming their line). Markdown parsing rejects
fake headings and duplicate required sections, accepts nested evidence, and
requires a nonempty supplied body on PR events. Bitbucket without a body
channel warns and skips that check; an explicitly empty body fails. Inline
HTML never hides a heading, and an unclosed HTML block warns instead of hiding
later sections. Freshly scaffolded policy also requires Reviews and Release
impact; without an explicit list the built-in default stays Summary and
Changes, and existing consumers retain their configured section lists.
Summary style, missing `Not tested:`, long fences, prose width and approximate
rendered rows warn under `pr_sections`. The independent `pr_release_impact`
check defaults to warn: it validates generic fields, compatibility consistency,
migration guidance and breaking commit floors against `pr_breaking_level`
(default major). It requires no release automation or project-specific fields.
The ADR-0067 dash check (`policy_characters`) also defaults to warn; CodeFlow's
own policy sets block. Its added-lines scan skips a file only when its bytes
equal the whole-file managed asset the running binary ships for that path, so
unmodified scaffold content never trips it and a project record proves nothing.
The generic PR template ships at every tier. The structural
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
detection (`quick` is the push set; a manual run aliases it to `essential`, the
lighter mode, when no target defines it). No stack
detected is a loud no-op (exit 0) so the bootstrap/early-setup path stays green;
`--strict` escalates that no-op to a non-zero exit for scripted/unattended callers
(CI, the pipeline verify gate) where "ran nothing" must not read as a pass. With a
stack it is a real gate, re-run in CI. The pre-push hook runs the push set (the
targets with a `quick` mode) plus `codeflow validate --docs` and `codeflow ci` on
the pushed range, blocking by default under `test_gate_on_push`. It blocks on
what it can see and names what it left to CI (an unresolved range, a sibling
ref, a dirty, sparse or submodule-incomplete checkout); the test suite belongs
to the full gate. One full gate runs at a time on a machine (a second refuses,
naming the holder), a gate that runs cargo warns about a `CARGO_TARGET_DIR`
outside the worktree, and each target prints a start line on stderr as it begins.
File and aggregate coverage thresholds all contribute to the gate
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
epics: [EPC-001, EPC-002, EPC-003, EPC-020]
adrs: [ADR-0002, ADR-0007, ADR-0025, ADR-0054]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider (GitHub via `gh api`: require PR + green CI, block force-push and
deletion) with a legible report of anything the plan tier cannot apply.
`codeflow doctor` runs eighteen health checks: hooks, Claude wiring, Codex wiring, Grok wiring, config,
permissions, network, delegates, qualified model bindings, delegate round-trip, repo integrity, CI
perimeter, managed-region
drift, consuming-project customization, always-loaded instruction size (a warning when the
`AGENTS.md` chain Codex loads for any directory, root to nested, exceeds its 32 KiB limit), test config, the id registry, and adopter fit. The Grok check reports
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
verified_by: ["codeflow-core tests/manifest_consistency.rs", "codeflow-core tests/model_eval_contract.rs", "codeflow-core src/model_qualification.rs", "codeflow-cli tests/orchestration_contract.rs", "cargo test validate::docs::tests", "cargo test models::task::tests", "docs/verification/task-graph-verification-canary-2026-07-25.md", "docs/verification/design-direction-canary-2026-07-26.md", "docs/verification/design-language-appearance-canary-2026-08-01.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test doctor::tests::test_check_delegates", "docs/verification/grok-host-duo-canary-2026-09-07.md", "cargo test workgraph::lifecycle", "cargo test workgraph::record_text", "codeflow-cli tests/record_lifecycle_journey.rs"]
epics: [EPC-002, EPC-003, EPC-004, EPC-005, EPC-008, EPC-009, EPC-011, EPC-012, EPC-017, EPC-020]
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

Claude Code reaches Codex through the official plugin/app-server. Codex
App/interactive CLI reaches Claude through Herdr (tmux degraded). Grok Build
reaches Codex through the official `codex` CLI and local app-server daemon,
and Claude through Herdr plus schema-v2. Grok-hosted lane canaries are in
`docs/verification/grok-host-duo-canary-2026-09-07.md`; they are not a
qualified binding. The standing pair remains the quality floor. Extra
catalog families (today Grok) are named when a routing-policy trigger fires
and the family is available; unavailable is an evidenced limitation, never a
silent third vote (ADR-0054). Primaries default to high, use proportionate
worker effort when useful, and obtain same-family xhigh reasoning on trigger
mid-session rather than restarting the host
(ADR-0056). Default UI assignment is Claude execution and implementer check plus Codex
Computer Use QA on the app-server; if Codex produced the UI, Claude QAs
independently. Another harness, including Hermes, normally delegates the
repository task to one native CodeFlow host; direct coordination requires both
native lanes and the full contract. Explicit host/peer/worker roles prevent recursive orchestration. The
shared quality and routing resources require reproducible
evidence, relevant unit/integration/e2e and UI tests, an 80% production-code
coverage floor where measurable (90% normal target), security review, and
bounded rework. It also blocks material avoidable complexity: both seats review
design proportionality, every executor first-verifies the smallest coherent
implementation, the accountable primary inspects it, a lineage different from
the actual author's independently reviews it, and the directly
invoked Claude judgment primary reviews the settled design and actual
integrated diff for the final quality verdict. Substantial prose additionally
loads `cf-editorial-review`: both seats protect technical meaning and evidence,
while the Claude judgment primary owns the final contextual voice and editorial
verdict. Cross-model
callers invoke both primary seats directly using the selectors, default and
escalation effort, triggers, and permitted internal routes in the current
ensemble record. Primary seats retain their plan, integration and approval
duties; each owning primary controls its internal routes, and the selected
Claude primary owns Claude-side judgment. A natively proven candidate may
execute bounded non-design work under primary review without becoming
qualified. A scoped-qualified claim is limited to its evidenced tuples and
remains distinct from full primary promotion or an economy/default claim. Each
run records actual model versions, applied effort and route,
requested-versus-observed provenance, and scoped usage evidence rather than
inferring availability, application, quota, or savings.

For a material changed journey, the end-to-end plan maps the affected entry,
in-project components, persistence/queue, external seam, infrastructure/runtime
wiring, observable result, and recovery path. One faithful vertical run crosses
every applicable changed boundary; disconnected unit/integration passes and a
mocked changed service are not whole-flow proof. Parallel UI tasks allocate
task-owned isolated browser state, applicable listening/application endpoints,
namespaced test data, run-scoped artifacts, and teardown evidence without
attaching to the operator's browser or active desktop. The project supplies its
own allocator/ranges, namespace, artifact, retention, and cleanup commands
during customization (ADR-0044).

For multi-task work, both approvals cover one acyclic Plan vN graph. Ordinary
completion uses bare edges; only genuine pre-approved decisions use observable
guards. Task frontmatter keeps non-executable structural `depends_on` data.
`validate --docs` checks canonical identities and filenames, relationship
shape, parent-or-standalone ownership, spec readiness, stable integration
targets, completed acceptance criteria, and malformed, dangling,
self-referential, duplicate, or cyclic topology. Explicit `codeflow work start`
checks the planning anchor of the task the branch carries on any work prefix
(`task/`, `fix/`, `feat/`, `spike/` and the rest; not `plan/` or
`integration/`). CI applies the same read-only merge-base check once per pull
request when full-tier or recognizable historical task tracking is active; the
per-commit hook no longer does. It proves validated planning is present on the
declared stable target. Both report at the `git.work_planning` level: `block`
by default, or `warn`, which reports the finding and lets the work continue. A declared target whose local branch is strictly
behind its configured upstream anchors on that upstream, with a note; a
diverged pair is refused. With tracking on, `codeflow ci` classifies every pull
request: tracked (`Task: TSK-NNN`, or the id the branch carries), direct change
(`Task: none: <reason>`), planning-only (records and `docs/plan/` only), an
epic's integration line (a task of the epic targets it, it lands on the
default target, and it holds only merges), or an automation profile. The
range is one diff from the merge-base, and tracking is read at the target
as well as the head. An unclassified one, a
mismatched `Task:` line, a pull request that adds the record it claims, and a
spike that lands anything but `docs/research/` findings and its own record
block. A direct change is refused on the floor of one embedded path table
(policy, hooks, managed instructions, CI files, manifests, record schema,
shipped templates) plus the project's own `git.product_paths` and
`git.breaking_watch_paths`; `init` writes a stack default for
`git.product_paths`, `update` adds it once, and `git.direct_changes: forbid`
refuses direct changes entirely. `task new --follow-up-of`, `epic new
--integration` and `adr new` (numbered, written `proposed`) are one command
each. One readiness core judges a task for `work next`, `work claim`,
`work start`, `status`, `orient` and CI: status `todo`, no Blocker, no
`awaiting_selection`, specs approved, epic open or standalone, code
dependencies complete in the execution base, and research or decision
dependencies (`{id, kind, pin}`) complete at their pinned commit. `work
next` lists ready, then waiting and blocked tasks with reasons from the
refs as last fetched; `work claim` fetches, refuses a task a visible branch
already carries, and pushes `task/TSK-NNN-<slug>` as an advisory claim.
`status` shows derived active, ready, landed and conflicting branches and
epic progress, and never calls a live integration line removable. A
selection that removes `awaiting_selection` lands only from `plan/`, and
`spec new --for` links every consumer in one change. Material graph or
cross-task contract changes force Plan vN+1; in-node implementation detail
does not.
Record status moves only by legal transitions (SPC-013 R-30 to R-35).
`codeflow task status`, `epic status` and `spec status` write the status and
only the sections the transition needs: a `## Blocker` with reason, owner and
revisit for a blocked task, Closeout lines `- cancelled:` and `- scope:` for a
cancelled record, and a fenced `yaml` acceptance block on completion.
Reopening keeps the old block under `acceptance_superseded:` with its reason;
a task completed before the migration, with no block, records a Closeout line
`- reopened: <reason>` instead. Sections and blocks inside HTML comments or
enclosing fences never count. A spec is approved or superseded only in a
planning-only change, and supersession adds its successor in that change.
The verbs are safe editors, not the only writers: one core judge rules on a
verb's proposal, on a hand edit (`validate --docs --since <ref>`) and on each
record a pull request changes (`codeflow ci`). No verb writes `in_progress`,
and spec `implemented` is derived from the consumers. Epic close needs every
task terminal, every criterion verified and every consumed spec implemented
or still consumed. New records list criteria as `- AC-n` without a checkbox.
The rules apply from the `work_records_baseline` commit in project config,
which `codeflow update` records once, and by transition: an unchanged older
record keeps its exact-blob exemption. `git.work_records` accepts `block` or
`warn`, never `off`. The ledger's producerless work-graph event types are
retired.
A completion is bound to the reviewed commit (SPC-013 R-52 to R-54, R-60 to
R-62): `task status complete` and `codeflow ci` check that the block's
`reviewed` commit, named by object id, is the head or an ancestor after
which only the record's status and Closeout changed, and that each waiver
names a planning-only amendment on the target that changed that criterion;
the verb also refuses uncommitted changes outside the record. Only a
planning-only change or a checked epic line can change a task's criteria;
the pull request's class decides it, not the branch prefix. A range touching the
adopter-facing path set needs a `(journey)` criterion or one serving the
epic's journey, and a leaf serving it says what ran or its narrower path. A
criterion tagged `(after release)` is `deferred` with owner, window and a
listed follow-up. `git.work_records` sets the binding and journey rules;
frozen criteria always block. The check states that it proves structure and
binding only, and cf-reviewer, cf-consult and cf-ship ask whether each
criterion is supported on this source and achieves the outcome.
Review-relevant bounded discoveries persist at task closeout; closeout cannot
retroactively approve a
material change. Project organization keeps one authoritative work-item home
and links, rather than mirrors, external planning methods or trackers. A foreign
tasks folder alone does not activate those durable gates; malformed relevant
tracking state yields a diagnostic instead of a silent opt-out. New projects
earn structure from accepted ownership and interface boundaries; existing
projects retain credible native layouts. Current requirements stay living
authority while SPC files freeze only warranted change agreements. External
approval never waives active CodeFlow execution gates.
Verification planning selects property tests, targeted mutation testing, or
project-owned architecture fitness checks only when the risk and oracle
evidence earn them. CodeFlow adds neither a scheduler nor mandatory
consuming-project tools.

Independent implementation tasks use bounded, host-resource-aware parallelism:
one owner/branch/worktree per task, a single owner for shared files, serialized
landing through `codeflow integrate` to `integration/<epic>`, affected gates
after each landing, and aggregate gates plus review on the combined diff.
Missing seats degrade legibly to solo; mid-run failure blocks and escalates.

The unattended Claude workflow is explicitly single-vendor and rejects the old
`duo` preset semantics. Manifest parity tests pin byte mirrors, while
`orchestration_contract.rs` pins the two-draft anti-anchoring rule, design and
review roles, hard coverage floor, security lenses, always-loaded reasoning
duties, host, UI, and reverse-lane contract markers. Runtime adapter behavior
is exercised by the CAP-009 hook unit and CLI tests. No engine model router is
added; deterministic gates and the human-merged PR remain authoritative.

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
epics: [EPC-003, EPC-004, EPC-005, EPC-009, EPC-010, EPC-011, EPC-012, EPC-020]
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
epics: [EPC-003, EPC-004, EPC-005, EPC-008, EPC-010, EPC-011, EPC-012, EPC-017, EPC-020]
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

The kit separates durable doctrine from fast-changing bindings. A
source-controlled harness catalog marks a harness `capability-supported` only
after evidence of native-interactive execution, runtime provenance, configured
tools, scoped work, bounded failure, recheckable results, an effective
permission boundary, and the git backstop. Catalog status does not qualify a
concrete model binding. One current ensemble record owns concrete primary
selectors, effort policy, typed internal routes, route status, and escalation
triggers. A new harness or model name is not usable as a standing primary merely
because it parses; the harness needs catalog evidence and a concrete primary
binding needs approved native full qualification.
Standard/full projects may then reference an approved binding ID for an exact
stable role in `.codeflow/model-selection.json`. The file is reference-only;
doctor resolves it atomically and fails closed on malformed, ineligible,
unsupported, drifted, or lineage-collapsing overrides. An absent or empty file
keeps the managed ensemble.
Composable diagnostic packs select existing cases without changing graders or
promotion. Approved full results can emit non-secret local binding records;
doctor detects record contradictions and observable harness/settings drift
without launching, inferring, promoting, or routing a model (ADR-0039).

A configured candidate is distinct from native availability and applied
selection: current routing evidence may permit bounded non-design execution
under owning-primary inspection without establishing scoped quality. A
scoped-qualified claim covers only exact evidenced harness/selector/effort/
workload tuples and requires three fresh accepted trials per pre-registered
case and arm, complete applied provenance, primary integration, cross-family
review, and no unresolved validity threat. The evidence path belongs to the
catalog's source repository. This focused status never substitutes for full
primary-binding promotion; an economical-default recommendation separately
requires measured all-attempt benefit including coordination and rework
(ADR-0060).

The hard `CF-OUT-002` contract evaluates contextual editorial quality without
surface-cue policing. Its cases cover technical semantic preservation,
operator uncertainty, consuming-project voice, sycophancy/inflation/formatting,
medium-appropriate emoji, and false positives for legitimate punctuation,
terms, and lists.

The hard `CF-DES-005` contract keeps language/voice and appearance modes
contextual and collapsible. Its cases require distinct project-evidenced
voices, legible titles/actions/states, honest localization claims, applicable
system/user mode and persistence evidence, and isolation between CodeFlow
utility defaults and consuming-product design authority.

The hard `CF-DES-006` and `CF-DES-007` contracts keep later exploration and
asset use bounded and reviewable. Their cases require a named unresolved choice
before in-direction variants, an explicit stop condition, exact reviewed-
version provenance for material Plan vN+1 feedback, separation of inspiration
from user evidence, rights/consent and transformation evidence, refusal of
unauthorized private-data uploads, and verification in the actual product
context. Provider catalogs and parallel design databases remain outside the
portable doctrine (ADR-0051).

The hard `CF-QA-002` contract separates browser headlessness from interactive
peer-model transport and requires claim-matched behavior, visual, runtime,
trace, and accessibility evidence. Its regression canary rejects
screenshot-only verdicts, indiscriminate tracing, Computer Use as the default
web driver, and helper-model ownership of the Claude primary's design judgment.

The hard `CF-QA-007` contract requires complementary deterministic and
contextual verification. Applicable syntax/style, SCA, source/data-flow, taint,
secret, and architecture checks stay distinct from independent intent and
semantic review; a deterministic red result cannot be waived by model
consensus, and missing relevant SAST/taint evidence remains visible residual
risk. `CF-QA-008` adds a bounded-history craftsmanship case so repeated
dependency and duplication erosion cannot hide behind a passing point diff.

The hard `CF-QA-005` contract evaluates materiality-led review, execution
focus, and proactive routing. Cases require consequential findings to lead
cosmetic nits, approve when only non-blocking preferences remain, recognize
repeated symptoms as a possible systemic cause, keep remediation effort out of
severity, and preserve CVSS-aligned security severity and separate confidence
before mapping the result to the general gate. A paired execution fixture
distinguishes an actionable material blocker from cosmetic bait, then a
completed critical path from a clear, safe, in-scope improvement: models must
protect the required gates without reflexively deferring bounded work. Only
genuinely uncertain observations are consolidated for one natural duo
checkpoint and, when retained, tracked once with evidence and a deterministic
revisit event. Other cases escalate or track evidenced out-of-scope risk
without silently expanding scope or generating one issue per nit.

The paired governance cases separate persistence from assumption. A
discoverable tool failure must move through a new evidenced hypothesis or an
accepted-outcome-preserving reversible strategy rather than repeat or ask the
operator to choose a tactic. Missing product intent or a public contract still
blocks for a well-framed operator decision. Retry counts alone prove neither
case.

Canary mode runs selected regressions once while maintaining the corpus. Full
qualification runs every case three times and is required for promotion. Each
trial uses a fresh one-commit disposable repository; the materializer removes
the evaluation skill and expected answers, keeps evaluator state outside the
subject tree, and gives the subject an opaque path with a neutral repository
name before rebuilding fixture history. The model runs only in a supervised
native interactive Codex App/CLI or Claude Code session with the actual
tools/MCPs being qualified. Results retain model, effort, harness, settings,
permissions, tools, network and resource budgets; status is recomputed from
expected versus observed signals and evidence.

Promotion requires no hard regression from the pinned baseline, resolved
validity and grader findings, complete full-suite evidence, and explicit human
approval. Token/latency improvements are diagnostics and never compensate for
lost behavior. The feature adds no CLI subcommand, model runtime, headless peer
execution, CI model call, or generic cleanup surface (ADR-0027).

## CAP-014 — transport-neutral-delegate-lifecycle

```yaml
id: CAP-014
name: transport-neutral-delegate-lifecycle
area: engine
status: building
verified_by: ["cargo test delegate::", "codeflow-cli tests/delegate_cli.rs", "codeflow-cli tests/delegate_pty_stress.rs", "cargo test doctor::tests::test_delegate_roundtrip", "docs/verification/delegate-lifecycle-canary-2026-07-23.md", "docs/verification/delegate-lifecycle-canary-2026-07-24.md"]
epics: [EPC-002, EPC-019]
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

A task the turn backgrounds (a Workflow, a background Bash command) can finish
after the turn's Stop, and Claude Code then submits a task notice as a new
prompt. The hook admits it without an armed turn only as a continuation of the
current turn: the prompt must be exactly one `<task-notification>` envelope,
the turn must have stopped with no other continuation open, and the session
transcript must show the tool call that launched that task within the turn or
one of its continuations. The record under `turns/<turn>/continuations/<task>/`
keeps the notice's `prompt_id` and the SHA-256 of its bytes. Claude Code
2.1.283 gives the prompt hook nothing that tells a real notice from the same
text typed into the session, so the proof waits for the Stop carrying that
`prompt_id`: before it writes the continuation result, the transcript must
record the prompt with origin `task-notification`, `promptSource` `system` and
`turnOrigin` `task_notification`, bytes matching the digest, and an earlier
queue enqueue of the same bytes. A typed copy, a changed body or a missing
entry poisons the run and writes no result. The residual risk stands: the
model may act on a forged notice within that continuation turn; the check
keeps it from being recorded as a clean result.

Status is building: the engine surface and its unit/CLI/doctor tests landed,
and the `delegate-roundtrip` doctor check requires the rebuilt CLI to be
installed before it can pass. The dated PR1 canary record covers the active
Stop-hook set, a live Unicode normalization case, `Stop.prompt_id` binding,
AskUserQuestion, and permission-response routing on the available macOS arm64
host. The reusable sibling-hook rejection procedure, full fake-TUI stress
matrix, and broader native-platform evidence remain PR2/release gates and are
not claimed complete.

## CAP-016 — interactive-presentation-review

```yaml
id: CAP-016
name: interactive-presentation-review
area: engine
status: building
verified_by: ["cargo test -p codeflow-present", "cargo test -p codeflow-cli --test present_cli", "npm run check:browser --prefix crates/codeflow-present/web", "codeflow-core tests/manifest_consistency.rs"]
epics: [EPC-005, EPC-020]
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

Text feedback retains the selected occurrence and revalidates live ranges before
pinning. Toolbar capture survives focus changes; leaving Comment releases its
capture state. Iframe figures use native hit-testing while commenting and regain
their prior pointer interaction afterward, without overlays on neighboring text.

The runtime validates the declarative block tree, embeds its deterministic
renderer, stores immutable revisions and append-only feedback in owner-private
project-keyed durable state, keeps browser-owned profile/cache and bounded
runtime controls in a separate derived root, and exposes open/update/list/show/history/feedback/resolve/
export/close/clear through the CLI. A one-time tokenless file bootstrap opens a
CodeFlow-owned isolated browser profile against an authenticated loopback-only
service. Host/Origin/CSP/path/body limits, inert revision-qualified HTML
sandboxing, strict primitive-token import, crash recovery, bounded retention,
and identity-scoped cleanup are code boundaries. Event parsing and partial-tail
repair are self-bounded and operate through one opened handle. Mermaid input is
capped per diagram and per document; browser enhancement is serialized, yields
between diagrams, and fails remaining items to escaped source when the eager
fallback exhausts its cumulative budget. Browser and auxiliary system-tool
children share one allowlist-only environment. Windows ACL mutation is confined
to creation for every private file, including append and lease files; existing
state uses native read-only owner/protected-DACL/trustee/inheritance
verification. Unix ignores a relative XDG state override and rejects a relative
home rather than placing state in the worktree. Static export remains self-contained and excludes
review/authentication/runtime state.

Native-path repository identity, per-block and whole-document collection
cardinality, a project→session→runtime-control lock order, pre-publication
capacity admission for every durable growth route, and an over-quota-safe
control path keep quota boundaries deterministic under concurrent creation,
revision, feedback, and runtime registration. The browser loads a bounded recent
feedback snapshot, uniquely re-anchors exact selectors across revisions, leaves
missing/ambiguous selectors visibly orphaned, and exposes the current lifecycle
version. Resolve accepts only a current delivered event and appends
addressed/dismissed state. Exact accepted receipt and identical terminal retries
are zero-growth operations; conflicting reuse and unrelated stale versions fail
closed. Retention recomputes bounded durable size after every eviction, selected
cleanup loads only its named session, and bulk cleanup reports isolated partial
failures without deleting ambiguous state. A selected session that cannot be
removed reports its exact retained outcome instead of succeeding silently.
Verified Unix process groups receive bounded graceful shutdown and then an
identity recheck before forced termination. Unverifiable orphan process
groups/trees retain recovery state rather than killing an unproven process or
deleting its profile. Launches are serialized under a session lease and publish
one exact record per attempt; close, show, and retry consume it. Reused PIDs are
never signalled: bounded exact-marker discovery and native profile-resource
proof either complete cleanup or retain an actionable error. Windows checks
exclusive handles across the actual profile tree rather than assuming a
POSIX-style lock file. After a retained group/tree exits or an operator verifies
and terminates it, retrying close completes cleanup. Review input controls expose
the Rust-owned note, text, selection, and payload bounds before submission.

The capability remains `building` until TSK-007 records the full native
macOS/Linux/WSL2/Windows and qualified-browser matrix, adversarial service and
state evidence, design/accessibility/responsive comparisons, deterministic
asset/release checks, and fresh native interactive model trials. That matrix
includes Windows Unicode known-folder/profile paths, creation-time ACL
hardening plus read-only weakened-ACL rejection, trusted system tools, exact
process-tree identity and file URLs; Linux/WSL2
bounded `/proc` identity and group signaling; and a dense multi-diagram browser
corpus with long-task evidence. Cross-builds alone do not claim native runtime
support.

## CAP-015 — opt-in-documentation-portal

```yaml
id: CAP-015
name: opt-in-documentation-portal
area: scaffold
status: shipped
verified_by: ["codeflow test --mode full --strict", "cargo test scaffold::portal", "cargo test validate::portal", "codeflow-core tests/manifest_consistency.rs", "node --test docs-portal/tests/adapter.test.mjs", "npm run build --prefix docs-portal", "codeflow validate --portal docs-portal", "docs/verification/tsk-009-docs-portal/", "docs/verification/tsk-020-portal-ownership.md"]
epics: [EPC-005, EPC-007, EPC-013, EPC-017]
adrs: [ADR-0048, ADR-0058]
```

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the exact-pinned Starlight and Pagefind repository-guide utility. The
portal build requires Node 22.19.0 or newer. The aggregate CI gate runs on Node
26.4.0, and its full strict target installs, checks, builds, and validates the
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

The repository-owned full gate runs the complete locked JavaScript authority
suite, a real Starlight build, and the Rust verifier locally and on Ubuntu; the
Windows lane runs the same JavaScript authority/path suite. Shared fixtures pin
configuration, strict-frontmatter, 40/64-character Git object ID, exact
case-sensitive route, and URL-boundary behavior across the producer and
verifier. These dogfood gates do not leak a Node requirement into the generic
consumer CI scaffold: adopted consumer portals opt into their project test
configuration.

The mirrored `cf-docs-portal` skill owns proportional adoption, layered
information design, safe source interpretation, exact dependency operations,
browser/accessibility evidence, and cleanup. It applies the same utility
presentation craft as `cf-present` (tokens, altitude, stage grammar) to
durable source-linked docs for CodeFlow or any consuming project, without a
session Comment lifecycle. The repository dogfoods the starter under
`docs-portal/`; a generated local site is evidence and never an implicit
publish action.
