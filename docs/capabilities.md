# codeflow — capabilities

<!-- WHAT layer: the registry of what the system does. Consult before building
     ("does this exist? what does it touch?"). Updated in the same PR that
     ships the work — the discipline is the ship flow, not a blocking gate.
     `validate --docs` enforces referential integrity (each entry's epics[] and
     adrs[] resolve to real files) and a non-empty verified_by on shipped
     entries; it does not verify that the test tags resolve, and it does not
     block an epic from closing.
     Statuses: planned → building → shipped → deprecated (never delete).
     At ~15 entries, graduate to docs/capabilities/CAP-*.md. -->

## CAP-001 — scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs", "codeflow-cli tests/tier_floor_e2e.rs", "codeflow-cli tests/settings_presets.rs", "codeflow-cli tests/codex_config.rs"]
epics: [EPC-001]
adrs: [ADR-0019, ADR-0025, ADR-0026]
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline
layer into any repo. Enforcement is the floor; the tiers scale project-management
(ADR-0019): `--minimal` installs the complete four-plane enforcement floor (all
five git-hook shims, the CI check, the in-session `git-guard`/`exec-guard` +
orient/summary hooks in `.claude/settings.json` and the `.codex/` starter, the
armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md`);
`--standard` adds the method (Claude/agent skills, reviewer agents, the pipeline)
and the six-layer docs spine and full contract; `--full` adds
project-management/. Idempotent, non-destructive, offline (assets embedded via
rust-embed), with bootstrap grace, husky/hooksPath detection, and a printed
per-file report. Standard/full reports close with `/cf-customize`, pointing at
the consuming project's product, architecture, agent context, harness settings,
and tools; minimal does not advertise an uninstalled method skill. Re-running
at a higher tier is an additive upgrade.

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
epics: [EPC-001]
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
epics: [EPC-001]
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
adrs: []
```

`codeflow recall [--all] "<query>"` searches project memory — ledger events,
session summaries, ADRs, epics, capabilities — via bundled SQLite FTS5, with
coverage gaps disclosed. The cross-repo view comes from
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
epics: [EPC-001, EPC-002]
adrs: [ADR-0002, ADR-0007, ADR-0025]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider (GitHub via `gh api`: require PR + green CI, block force-push and
deletion) with a legible report of anything the plan tier cannot apply.
`codeflow doctor` runs fourteen health checks — hooks, Claude wiring, codex wiring, config,
permissions, network, delegates, qualified model bindings, delegate round-trip, repo integrity, CI
perimeter, managed-region
drift, consuming-project customization, and test config. The customization
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
verified_by: ["cargo test doctor::tests::test_check_delegates", "cargo test --test orchestration_contract", "docs/verification/host-neutral-duo-canary-2026-07-15.md"]
epics: [EPC-002]
adrs: [ADR-0005, ADR-0018, ADR-0023]
```

Consult or delegate a unit of work to another vendor's coding CLI at the
process boundary, each under its own subscription auth, with CodeFlow's gates
judging the output author-agnostically (ADR-0005). Transport is
interactive-only per ADR-0023, one lane per direction: from Claude Code the
official `codex-plugin-cc` plugin (wrapping the codex app-server); from codex
the interactive `claude` CLI driven via task-scoped tmux, with Stop and
StopFailure hook completion rather than pane stability. The
`codeflow hook delegate-turn` adapter validates a unique run and private path,
writes immutable `0600` terminal evidence, and signals only its scoped waiter;
exact retries recover signalling without rewriting. The transport-neutral
schema-v2 lifecycle (CAP-014, ADR-0036) is this adapter's file-polled
successor under verification; the legacy `--result` mode described here
remains byte-compatible until a later major release. Headless task execution
(`codex exec`, `claude -p`) is prohibited; the earlier headless tier and the
Antigravity `agy` delegate tier (headless-only) are retired. It ships as the
`cf-delegate` and `cf-consult` skills (mirrored to `.agents/skills`), and an
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
verified_by: ["codeflow-core tests/manifest_consistency.rs", "codeflow-core tests/model_eval_contract.rs", "codeflow-cli tests/orchestration_contract.rs", "cargo test validate::docs::tests", "cargo test models::task::tests", "docs/verification/task-graph-verification-canary-2026-07-25.md", "docs/verification/design-direction-canary-2026-07-26.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md", "cargo test doctor::tests::test_check_delegates"]
epics: [EPC-002, EPC-004]
adrs: [ADR-0015, ADR-0018, ADR-0023, ADR-0024, ADR-0025, ADR-0028, ADR-0030, ADR-0032, ADR-0034, ADR-0035, ADR-0040, ADR-0041, ADR-0042, ADR-0043, ADR-0044]
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
the producer and cross-lineage reviewer from verified capability, context,
resources, and observed native usage evidence. Both seats approve those
assignments before implementation; changing a named seat or lineage invalidates
the approvals. Each producer first-verifies its unit, the other lineage reviews
it independently, and the selected `claude-judgment-primary` owns integrated
Claude quality judgment without claiming independent review of its own unit.
For material product, UX, interaction, or visual-direction work, the
orchestrator loads `cf-design` and records a proportionate `DESIGN_INTENT`
inside that same plan. Cosmetic changes may collapse as not applicable,
bounded established-system work may conform, new surfaces settle a direction,
and materially open novel work compares two or three viable directions first.
The Claude judgment role leads intent, Codex challenges feasibility and
fidelity, and both approve the exact plan. Review anchors blocking design
findings in the accepted brief, intent, accessibility target, or observed
behavior rather than taste. The design-direction eval pack covers this
selection, operator precedence, evidence-grounded design-choice review,
accessibility, and rendered fidelity (ADR-0043).

Claude Code reaches Codex through the official plugin. Codex App/interactive
CLI reaches Claude through an interactive task-scoped tmux session. Another
harness, including Hermes, normally delegates the repository task to one native
CodeFlow host; direct coordination requires both native lanes and the full
contract. Explicit host/peer/worker roles prevent recursive orchestration. The
shared quality and routing resources require reproducible
evidence, relevant unit/integration/e2e and UI tests, an 80% production-code
coverage floor where measurable (90% normal target), security review, and
bounded rework. It also blocks material avoidable complexity: both seats review
design proportionality, every producer first-verifies the smallest coherent
implementation, the other lineage independently reviews it, and the directly
invoked Claude judgment primary reviews the settled design and actual
integrated diff for the final quality verdict. Substantial prose additionally
loads `cf-editorial-review`: both seats protect technical meaning and evidence,
while the Claude judgment primary owns the final contextual voice and editorial
verdict. Cross-model
callers invoke both primary seats directly using the selectors, default and
escalation effort, triggers, and permitted internal routes in the current
ensemble record. Primary seats retain their plan and approval duties, each
owning primary controls internal routing, and the selected Claude primary owns
Claude-side judgment.
Each run records actual model versions, effort, routing evidence, and escalation
rationale rather than inferring usage state.

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
guards. Task frontmatter keeps non-executable structural `depends_on` data so
`validate --docs` can reject malformed, dangling, self-referential, duplicate,
or cyclic topology without interpreting branch readiness. Material graph or
cross-task contract changes force Plan vN+1; in-node implementation detail does
not. Verification planning selects property tests, targeted mutation testing,
or project-owned architecture fitness checks only when the risk and oracle
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
epics: []
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
epics: [EPC-004]
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

## CAP-013 — model-binding-evaluation

```yaml
id: CAP-013
name: model-binding-evaluation
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/model_eval_contract.rs", "codeflow-core model_qualification + doctor::tests::model_bindings", "evals/model-artifacts/test_eval_kit.py", "codeflow-cli tests/init_e2e.rs", "docs/verification/model-role-layered-verification-diagnostic-2026-07-25.md", "docs/verification/model-role-quality-diagnostic-2026-07-26.md", "docs/verification/whole-flow-ui-isolation-canary-2026-07-26.md"]
epics: [EPC-003, EPC-004]
adrs: [ADR-0027, ADR-0032, ADR-0034, ADR-0039, ADR-0041, ADR-0042, ADR-0044]
```

`/cf-evaluate-model` qualifies a new model/version, native harness release,
permission profile, or material CodeFlow instruction change as the complete
system users will run. The standard/full managed skill carries stable hard
requirement IDs, source-marker traceability, balanced regression/capability
cases, exact fixture overlays, a native-interactive run protocol, and a
standard-library tool for deterministic validation, materialization, scoring,
baseline comparison, and fail-closed cleanup.

The kit separates durable doctrine from fast-changing bindings. A
source-controlled harness catalog marks a harness `capability-supported` only
after evidence of native-interactive execution, runtime provenance, configured
tools, scoped work, bounded failure, recheckable results, an effective
permission boundary, and the git backstop. Catalog status does not qualify a
concrete model binding. One current
ensemble record owns concrete primary selectors, effort policy, worker classes,
and escalation triggers. A new harness or model name is not usable merely
because it parses; the harness needs catalog evidence and the concrete binding
needs approved native full qualification.
Standard/full projects may then reference an approved binding ID for an exact
stable role in `.codeflow/model-selection.json`. The file is reference-only;
doctor resolves it atomically and fails closed on malformed, ineligible,
unsupported, drifted, or lineage-collapsing overrides. An absent or empty file
keeps the managed ensemble.
Composable diagnostic packs select existing cases without changing graders or
promotion. Approved full results can emit non-secret local binding records;
doctor detects record contradictions and observable harness/settings drift
without launching, inferring, promoting, or routing a model (ADR-0039).

The hard `CF-OUT-002` contract evaluates contextual editorial quality without
surface-cue policing. Its cases cover technical semantic preservation,
operator uncertainty, consuming-project voice, sycophancy/inflation/formatting,
medium-appropriate emoji, and false positives for legitimate punctuation,
terms, and lists.

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
