---
name: cf-customize
description: Tailor a CodeFlow scaffold to its consuming project. Verify both interactive duo lanes, their effective autonomy/network/secret boundaries, and the research, source-control, test, security, browser/UI, design, and project-specific MCP tools the work needs. Then derive and confirm the consuming project's product, architecture, operating-contract, policy, workflow, parallelism, and model-routing specifics from evidence already in the repo. Use when customizing after init, after `codeflow update` brings new defaults, or when the operator asks to tailor the scaffold. Analyze before editing; never invent project facts or auto-install tools. Do not use for ordinary feature work.
---

# cf-customize — tailor a scaffolded project to itself

`codeflow init` scaffolds the generic surface; this fills in the project. Init
seeds one-liners, generic templates, and warn-level defaults — the reasoning
about *this* project's purpose, its tools, and where its gates should sit is left
for a thinking session. That is this skill: the reasoning-layer companion to
`init`, not a mechanical command.

Two jobs, run in this order: **Part A** verifies the tools the project's flows
need and offers to fix what is missing; **Part B** fills the project-owned
artifacts still sitting at template defaults.

## How to run it

1. **Analyze first, propose second — the default.** Produce one prioritized
   report of what is missing (Part A) and what is still generic (Part B) *before*
   touching anything. Then walk the fixes interactively, one at a time.
2. **On a working branch.** Do the work on `chore/codeflow-customize` (or another
   `chore/` branch), scoped commits, and land it through a PR like any change —
   never on a protected branch. Push the branch for durability as you go (backup,
   not a merge).
3. **Idempotent.** Safe to run at init *and* to re-run post-init — e.g. after a
   `codeflow update` ships new defaults (like the `security_review` / `dep_audit`
   gate levels) that need a project decision. A section already tailored is left
   alone; only what is still at a default is proposed.

## Part A — flow-aware tool preflight

First decide **which flows this project uses**, then verify each flow's tools.
Standard/full installs default every non-trivial repository task to the duo;
solo is a preflight-proven degradation, not an equivalent preference:

- **Solo** (`/cf-develop`) — always in play.
- **Duo** (`/cf-model-orchestrator`) — host-neutral when both native
  interactive seats are available: Claude Code → Codex through the official
  plugin/app-server, Codex → Claude through Herdr (tmux degraded), Grok →
  Codex through official `codex` CLI + app-server (CodeFlow ADR-0023, ADR-0055).
- **Batch** — the pipeline preset; single-vendor by design, even when its
  assurance stages resemble parts of the duo.

Then verify and **offer** remediation — never install silently.

- **Core (every flow).** git, the `codeflow` binary, the active harness, and the
  stack test toolchain. Read inspectable health from **`codeflow doctor`**; its
  `delegates` check covers both CLIs, Codex auth/MCP, the Claude plugin/MCP, and
  tmux. Retain live interactive canaries because status commands cannot prove a
  native TTY session and its tools work.
- **Duo flow** (codex configured / `cf-model-orchestrator` in use):
  - **Claude-host lane** — `codex login status`, `codex mcp list`, the enabled
    `codex@openai-codex` plugin, and a scoped `/codex:setup`/tool canary. The
    plugin/app-server is preferred. If unavailable or incompatible, qualify
    the official native fallback in `cf-delegate`; missing plugin alone does
    not establish a missing Codex seat. Verify task tools and safety boundaries.
    Never use headless `codex exec`, a hand-rolled app-server driver,
    or a third-party Grok Codex plugin.
  - **Codex-host lane** — `claude --version` (2.1.187 or newer for sandbox
    environment-variable denies), `herdr` when `HERDR_ENV=1` else `tmux`, and
    `claude mcp list`, followed by authenticated interactive TTY canaries for
    the current ensemble's Claude primary at default effort. Escalation
    efforts are exercised by in-family workers, not canaried on the primary.
    Production uses `bypassPermissions`; consult/no-edit review uses
    auto with `autoMode.classifyAllShell: true` through `--settings`, plus one
    schema-v2 round trip — `delegate init` → wait-ready → `arm` → canonical
    UTF-8/internal-LF exact-byte delivery → wait-accepted → wait-terminal with
    bounded cleanup — including the `cf-delegate` sibling Stop-hook preflight.
    Never use `claude -p`, bare `tmux wait-for`, or pane
    stability as the work protocol. If Fable is unavailable, record Opus as
    the fallback; never claim the fallback was the selected primary.
  - **Autonomy settings** — parse and inspect the effective files rather than
    trusting their comments:
    - `.claude/settings.json`: sandbox enabled and fail-closed, sandboxed Bash
      auto-approved, classified unsandboxed retry enabled only for trusted
      installed tools that fail because they require host state, wildcard
      public-domain egress present for dependency/tool subprocesses, common private/link-local
      destinations denied, destructive/privileged operations asked or
      classified, secret reads denied, and the shipped raw Anthropic/OpenAI/AWS
      variables absent from sandboxed Bash. Claude ignores project
      `defaultMode: auto` and `autoMode`, so use the user-level setting for a
      lifecycle session (its one immutable CLI settings file carries the
      hooks), then prove the composed boundary with a live canary.
    - `.codex/config.toml`: `default_permissions` selects the guarded workspace
      profile, no legacy `sandbox_mode` shadows it, public network and live web
      search are enabled, `approval_policy = "never"` (no approval prompts), and
      production launch adds `--sandbox danger-full-access` (OS sandbox off;
      git-guard, exec-guard, git hooks, and CI remain the floor). Confirm the
      consult/`workspace-write` profile still denies workspace key/certificate
      files and `~/.codex/auth.json`. `ignore_default_excludes = false` keeps
      Codex's secret-bearing environment filter active even in production.
  - **Research and task tools** — live web search/fetch and authoritative docs;
    GitHub/source-control; the project's format, lint, test, coverage,
    dependency, and security tools; one supported Playwright route in every
    native harness that will operate web UI; other-lineage Computer Use QA of
    changed UI (Codex via app-server; Claude Code Computer Use when Claude
    reviews a Codex-authored UI); Computer Use or a surface-specific
    driver for native/mobile/desktop UI; design tools for UI work; and
    project-specific issue-tracker, database, cloud, or private-document MCPs.
    Prove tool access through the actual peer lane, not only by listing
    configuration.
- **Stack test toolchain.** The runner the detected stack tests with — cargo /
  npm / pytest / go — aligned with `cf-stack` and what `codeflow test` invokes.
  A missing runner means the test gate cannot run.
  Inspect the selected deterministic security lane too: language-native
  analysis, SAST/dataflow/taint, SCA, and secret scanning are different
  evidence. For an eligible GitHub repository, CodeQL default setup is one
  low-maintenance option; Semgrep, a native analyzer, Sonar, or another
  maintained service may fit a different stack or governance model. Verify the
  actual rule coverage, scanned-file/tool status, suppressions, owner, cadence,
  and local/CI gate instead of choosing by brand. Never assume a private-repo
  entitlement or auto-enable a remote service. If relevant SAST/taint evidence
  is not selected, record the residual risk and why; model review and SCA do not
  impersonate it.
  Also inspect any project-owned property/generative, mutation, and architecture
  fitness commands named by the settled plan or stack standards. Verify only
  techniques whose trigger evidence satisfies the orchestrator's
  `resources/verification-selection.md`; do not install a tool or create a gate
  merely because another project uses one.

**Remediation rule (critical).** For each missing or outdated tool, **print the
exact fix and confirm before running it.** Never silently auto-install:
installing or updating a system tool is privileged and reaches outside the repo,
so it gets the same offer-and-confirm posture codeflow takes for any irreversible
or outward action. The fixes:

- codex-plugin-cc not installed → **from a Claude Code session only**:
  `/plugin marketplace add openai/codex-plugin-cc` → `/plugin install
  codex@openai-codex` → `/reload-plugins` → `/codex:setup`. These are Claude
  Code slash commands — if this session is another harness, do **not** offer
  them; report the gap for the user to fix from a Claude Code session.
- codex present but unauthenticated → `codex login`
- outdated Codex or Claude CLI → present the vendor-supported update command
  after checking current release guidance; never rely on a version pinned here
- tmux absent →
  `brew install tmux` (or the platform's package manager)
- no canaried Playwright route for a web-operating harness → choose the official
  CLI/skill when bounded high-throughput work and context economy dominate, or
  MCP when persistent browser state and rich iterative introspection are
  material. Do not install both merely for parity. Prove the actual actions,
  locators/accessibility snapshots, screenshots or visual comparisons,
  console/network inspection, and failure/first-retry trace capture the flow
  requires. Configuring Codex does not prove the Claude-hosted route. For Codex,
  the MCP is one supported option; add this block to `~/.codex/config.toml`, then
  confirm it reads READY with `codex mcp list`:

  ```toml
  [mcp_servers.playwright]
  command = "npx"
  args = ["-y", "@playwright/mcp@latest"]
  ```

  Do not add `--headless` globally: select browser mode per task. Headless is
  the efficient routine E2E/CI path; headed/UI mode is for materially visual,
  browser-chrome, environment-rendering, or interactive-debugging claims.
  For a project that can run concurrent UI work, also settle and canary its
  browser-resource contract rather than relying on MCP defaults:
  - a task/run ID owns a fresh isolated context/profile; use `--isolated` by
    default, or a task-specific `--user-data-dir` only when persistent state is
    required—never the operator's default browser profile;
  - the project allocates non-overlapping loopback application/service ports
    and, only for a listening MCP transport, a distinct MCP port. Record the
    allocator or safe range; CodeFlow does not impose universal port numbers;
  - mutable test data uses a run namespace/account/schema and a worker suffix
    when tests parallelize. Artifact/report/trace output uses an absolute
    run-scoped directory with a declared retention policy;
  - fixtures/finalizers stop only task-owned processes or containers, release
    ports, close browser state, remove disposable namespaced data, and retain
    bounded failure evidence. Canary teardown as well as launch/readiness.
  A headed run still launches a test-owned browser/profile and must not attach
  to or manipulate the operator's existing tabs, browser, profile, or active
  desktop. Computer Use requires a dedicated test desktop/session or explicit
  operator control.

- Claude auto mode unavailable → show the failed capability check; use
  `acceptEdits` with the fail-closed project sandbox for this run. Do not write
  `defaultMode: auto` into `.claude/settings.json`—Claude ignores it at project
  scope. Offer the user-level setting only with approval. A lifecycle peer run
  already uses its one immutable CLI settings file for hooks, so repeated
  `--settings` flags are not a supported composition mechanism; when user-scope
  auto is absent, use the documented `acceptEdits` fallback and record the
  reduced autonomy.
- a tool needs a credential → prefer its OAuth/keychain/app connector or MCP
  authentication. For Claude CLI subprocesses that require an environment
  token, offer user/CLI-level credential masking with an exact `injectHosts`
  list; project settings cannot safely configure masking. Never put a secret
  value in repository settings, prompts, logs, or an allow rule. Offer
  `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` only at user/managed scope and only after
  proving the project's hooks and stdio MCPs do not require provider
  credentials; it strips those credentials from all three subprocess classes,
  not only arbitrary Bash.
- GitHub CLI or Docker needs its config → verify, without printing credential
  values, that `gh` uses secure keychain storage and Docker uses a credential
  helper/`credsStore` before treating those config files as non-secret. On a
  keyring-less host with an inline plaintext/base64 credential, retain or add
  the file deny and configure a broker instead. If storage cannot be proven,
  fail closed and report the tool unavailable rather than reading the file.

If a tool is absent and the user declines the fix, **degrade legibly**—name the
missing lane/evidence and use solo `/cf-develop`. Never label the run duo or
quietly replace the missing vendor with another instance of the host model.

## Part B — project-artifact customization

Analyze what is still at template defaults, then propose filling each. First
discover candidate facts from the consuming repository's README, manifests,
package metadata, CI workflows, code layout, existing docs, and supported
commands. Reconcile contradictions and cite where every proposed fact came
from. Walk the result interactively—never invent project facts; the owner
confirms the final content.

- **`docs/product.md`** — the WHY layer: purpose, users, scope, non-goals. Init
  seeds only the one-liner; fill the four sections. Non-goals are the
  load-bearing part—the orchestrator/`cf-plan` checks new work against them.
  This describes the consuming project, never CodeFlow itself.
- **`docs/architecture.md`** — the HOW layer: major components, boundaries,
  ownership, state, integrations, and repository paths, with ADR links rather
  than duplicated decisions. Fill its overview and area sections from the
  actual layout.
- **`AGENTS.md`** — the common project-owned operating section *outside* the
  `codeflow:managed` markers: supported setup/dev/build/format/lint/test/security
  commands, prerequisites, repository map, environment constraints, and
  project-specific guardrails. Common project facts live here, not duplicated
  into every harness adapter.
- **`CLAUDE.md`** — append only genuinely Claude-specific project differences
  outside its managed region. Do not repeat the project brief or common commands
  already owned by `docs/product.md`, `docs/architecture.md`, or `AGENTS.md`.
  **Never edit inside either managed block**—`codeflow update` owns it.
- **Project shape and work authority** — identify whether this is a
  single-surface repo, a shared-release monorepo, or independently governed
  products in one tree; name the durable area boundaries and nearest local
  instructions. For each class of work, settle one authority: CodeFlow Git
  records, an external team tracker, an existing planning method, or a
  high-volume issue queue. Load
  `cf-method/references/project-organization.md`; configure only opaque loose
  links between authorities, never status mirroring or a host-local database as
  shared team truth. Existing project practice wins when it is coherent and
  durable—propose migration only for an evidenced failure.
- **Agentic operating/estimation method** — when useful to this project's
  delivery or capacity decisions, invoke `cf-estimate` for a concrete preview
  against its existing planning authority. Confirm new adoption, reuse a
  compatible profile or honor a recorded decline until its material re-offer
  event. Never seed forecasts, calibration tables or `.codeflow/estimate.json`
  automatically; a setup walkthrough does not authorize method adoption.
- **`.codeflow/policy.json`** — gate levels: harden `dep_audit` /
  `security_review` from `warn` → `block` once the project's scanners and
  allowlists are ready; protected-branch globs; branch prefixes. **Footer
  policy** — the commit body is `-` bullets + a `BREAKING CHANGE:` footer only by
  default; every other trailer blocks until opted in. Decide per class: (a)
  **track tickets in commits?** if yes, set `commit_ticket_keys` (e.g.
  `["Refs","Closes"]`) to *allow* them, then `commit_ticket_required`
  (`warn`/`block`) if every commit must *carry* one, and `commit_ticket_pattern`
  (e.g. `^PROJ-\d+$`) to constrain the ID format; (b) **DCO / sign-off?** add the
  token to `commit_required_footers` (e.g. `["Signed-off-by"]`) to require it on
  every commit; (c) any other trailer a workflow needs → add it to
  `commit_footer_tokens` to allow it. Leave every list empty for the strict
  default — an agent fills any slot you open, so open only what you mean.
- **`.claude/workflows/pipeline.workflow.js`** — per-stage model selection and
  the honest single-vendor assurance preset. Define bounded concurrency from
  the host's actual memory/CPU/tool budget; independent work gets separate
  branches/worktrees, explicit file ownership, and serialized integration. It
  is user-owned; `codeflow update` never touches it.
- **Harness model/MCP config** — read the current primary selectors, effort
  defaults/escalations, and permitted internal routes from
  `../cf-model-orchestrator/resources/current-ensemble.json`. Then inspect
  `.codeflow/model-selection.json`: absent/empty means the managed default;
  every override references only a local binding ID qualified for that exact
  stable role. Run `codeflow doctor --check model-bindings` before proposing an
  override. A failure blocks the selection—never write a raw selector, partially
  apply entries, or silently fall back. Invoke each effective primary directly,
  leave internal routing to that primary, require observed native routing for
  any worker, and keep planning, approval, interpretation, and judgment with the
  primaries.
  Configure the research,
  GitHub, docs, MCP, browser/UI, design, and project-service tools the project
  needs. Record actual selected versions and tool canaries as run evidence;
  keep fast-aging version pins out of shared doctrine.
- **Presentation utility** — leave `.codeflow/present/config.toml` and primitive
  tokens absent unless the project explicitly wants different closed-session
  retention or a one-way adaptation of project primitives into `cf-present`.
  When warranted, use the managed schemas and the examples in the `cf-present`
  skill, confirm the exact project-owned token authority, create both files as
  project-owned content, and run one light/dark accessibility canary. Never
  import product CSS, components, classes, frameworks, font files, paths, or
  runtime dependencies; never make the utility's defaults product authority.
- **Repository guide** — use `cf-docs-portal` when durable sources justify a
  layered guide. Confirm adoption separately from customization. Use supported
  config/token seams; preserve managed-runtime edits and missing project files.
  Bespoke runtime changes require explicit whole-runtime transfer, not a source
  merge or automatic abandonment of updates. Read the skill's operations
  reference before legacy migration or recovery; no portal is installed merely
  because setup or customization was requested.
- **Product and design direction** — for projects with user-facing surfaces,
  locate the product brief, audience/user research, operator-approved
  references, brand guidance, design-system source, platform conventions, and
  accessibility target that `cf-design` should consult. Reconcile conflicting
  authorities and add only a short pointer from the project-owned AGENTS
  section when discovery would otherwise be unreliable. Do not fabricate
  research, prescribe a visual style, create a design system for one surface,
  or add a mandatory design document where the project already has a durable
  home.
- **README and CI reconciliation** — update human-facing setup or CI only when
  the discovered canonical commands and documented behavior disagree. README is
  the human front door, not a second agent authority.
- **Editorial voice** — find existing project-owned voice/style guidance and
  representative human-approved examples. If none exists, ask whether durable
  guidance is wanted; never infer a persona from generated text. Keep the
  canonical guidance in the project's existing content/style home and add only
  a short pointer in the AGENTS.md project-owned section when agents need it.

For each: propose the change, get the user's content, write it, commit in a
scoped unit, and push for durability. `codeflow validate` and `codeflow doctor`
green before reporting done.

## Reminder, not auto-run

cf-customize needs an interactive thinking session, so it is **never auto-run**
(`codeflow init` may be non-interactive or run in CI). The intended surface is a
*reminder*: a `codeflow init` closing hint to run `/cf-customize`, plus a
doctor nudge while `docs/product.md`, `docs/architecture.md`, or the AGENTS.md
project section remains at template defaults, clearing once they are filled.
The reminder and doctor nudge are informational; they never mutate project
content. Do **not** propose auto-running the skill.

Report completion with the prioritized findings, the tool fixes applied or
declined (and what each declined fix costs), the artifacts filled, and the gate
output. Apply `cf-editorial-review` to the substantive report and artifact
edits, then hand off to `cf-ship` to land the PR — a human merges it.
