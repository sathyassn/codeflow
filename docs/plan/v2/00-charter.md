# CodeFlow v2 — Charter

**Status:** HISTORICAL — the v2 plan of record as authored on the date below. It was approved and executed, and v2.0.0 has since shipped; where a later ADR (`docs/decisions/`) supersedes it, the ADR wins. For current state see `docs/plan/v2/01-execution-status.md`. This body is preserved as the original plan, not current instruction — e.g. §10's `curl | sh` install and any "Not in v2.0.0" scoping reflect the plan as written, not today's reality. (Original status: final draft, pending go-ahead; on approval this document, split into sections, became `docs/plan/v2/`.)
**Date:** 2026-06-11
**Provenance:** Distills and supersedes the strategic review (`codeflow-strategic-review-2026-06-11.md`) and landing proposal (`codeflow-landing-proposal-2026-06-11.md`), plus all subsequent design discussion. Where this charter conflicts with those documents, this charter wins. Supersedes INF-EPC-051 and cancels INF-EPC-025/026/027 (Epic C/D/E as designed); truncates INF-EPC-024.

---

## 1. Identity

**CodeFlow is the AI-development discipline layer you install into any repo: one Rust binary (`codeflow`) that scaffolds, enforces, verifies, and remembers — while Claude Code (or any harness) does the developing.**

**It is:** a single binary + embedded scaffold; an opinionated baseline for starting any AI/LLM-developed project; a knowledge architecture with maintenance wired into workflows; a small set of hard gates around git, secrets, and tests; a durable, cross-repo work-and-decision record with recall.

**It is not:** a harness, an agent framework, an orchestrator, a model router, a GUI, a daemon, or a process-enforcement engine. Claude Code's native primitives (subagents, workflows, worktrees, tasks, memory, sandbox, permissions) are consumed, never reimplemented.

**Why v2 (one paragraph):** v1 was built when models needed every step supervised. Its substrate (PathFlow phases, sentinels, checkpoints, 8-agent role teams, CRDT claims, custom autorun, worktree manager — ~100K+ LOC) was absorbed by native harness features during the four months it was being built, and its own telemetry showed maintenance consuming all capacity (17 of the last 21 epics were fixes to itself). Fable-class models invert the design center: the scarce resources are **clear inputs** and **verified outputs**, not supervised middles.

---

## 2. Design principles

1. **Clarity in → light rails through → verification out.** Input clarity (templates force acceptance criteria; plan workflow asks before building; "no assumptions" as a hard rule). Operational clarity (instructions + a handful of hard gates — never process-step enforcement). Outcome validation (tests, evidence-based review against stated criteria, integrity lint, CI).
2. **Instructions tell, workflows do, gates verify.** Never rely on instructions alone for what matters; never build a gate for what instructions handle with current models.
3. **One source of truth per fact.** Never store what you can compute (status views are generated live); never duplicate across docs (link by ID).
4. **Append-only where possible.** ADRs and the ledger are immutable; append-only artifacts cannot rot.
5. **Docs mutate only inside gated workflows, in the same PR as the code.** Docs maintained by separate ceremony rot (proven by v1); docs updated as a side effect of shipping stay true.
6. **Graduated weight.** One file until it hurts; tiers from throwaway to serious project; the binary validates every shape so growth is mechanical, never re-architecture.
7. **Policy in config, not code.** Anything a user might legitimately want different (protected branches, gate strictness, tiers) lives in `policy.json` / `project.toml`, read by every enforcement plane.
8. **Legible degradation.** Every guard fails loud or visible, never silent. `codeflow doctor` reports the live enforcement matrix per plane and per harness.
9. **Native first.** Claude Code settings (permission deny/ask rules, sandbox) replace custom hook code wherever they can. Worktrees, tasks, memory, workflows: consume native.
10. **90-day humility.** Any code that orchestrates or wraps harness behavior is presumed disposable; quarterly review against the harness changelog.
11. **Nothing crosses from v1 unexamined.** v1 is a quarry, not a source tree. Code modules are imported only after a deliberate keep-decision, then trimmed to v2 idioms and re-tested. Docs, templates, instructions, and Claude artifacts are **never copied — they are re-authored from scratch against v2's philosophy**, even where the underlying standard (e.g. branch prefixes, commit format) carries over. If a v1 artifact's content is wanted, its *rules* are extracted and rewritten at v2 weight; the v1 file itself stays in the archive.

---

## 3. The engine (binary)

### 3.1 Command surface (~13 thin subcommands)

```
codeflow init [--minimal|--standard|--full] [--yes]   # scaffold; idempotent; non-destructive; offline
codeflow update [--all] [--diff <file>]               # manifest + 3-way merge refresh of managed files
codeflow doctor [--harness claude|codex|cursor]       # health + enforcement matrix + drift signals
codeflow test [--mode full|quick]                     # generic test engine; runtime stack detection
codeflow validate [<path>] [--docs]                   # frontmatter/structure + referential integrity
codeflow hook <git-guard|session-orient|session-summary>   # Claude-layer hooks (settings-wired)
codeflow git-hook <pre-commit|commit-msg|pre-push>    # git hook shims target
codeflow integrate <branch> [--into <target>]         # flock(rebase → test → merge) primitive
codeflow status [--all] [--capabilities]              # generated views; never stored
codeflow recall [--all] "<query>"                     # FTS5 over records/summaries/PR bodies/ADRs
codeflow stack add <rust|node|python>                 # stack overlay: skill + test defaults + lint + CI snippet
codeflow remote protect [--provider github]           # apply protected-branch policy to the remote
codeflow mcp serve                                    # workgraph + recall + orient for any harness (read-only first)
codeflow orient                                       # the session-start digest (also callable directly)
```

### 3.2 Module inventory

**Imported from `archive/v1`** (each consciously pulled, trimmed, tests green before next): testing engine (~10.6K LOC), workgraph + ledger + store (store demoted to rebuildable cache), validate, models (subset), security scanner (secret detection), git/ (conflict detection) + ci_wait, file_lock, error (pruned enums), doctor (core), settings (simplified), batch parsing + rescue (if `integrate`/driver needs them; else deferred).

**Built new (all small):** scaffold engine (init/update, manifest, ownership classes, 3-way merge), `git-guard` hook (~200 lines), `integrate`, recall (FTS5, `rusqlite` bundled), `orient` + `session-summary`, `remote protect` (GitHub adapter), `status`, `mcp serve` (later in week one), docs-integrity lint in validate.

**Never imported (dies with archive/v1):** pathflow/ entire, hooks/pre_tool_use+post_tool_use+task_completed+pipeline (the v1 enforcement organ), coordination/ (claims, loro, sync, merge_queue — `integrate` is written fresh against file_lock), transport/, autorun worker+orchestrator+stale, tui/, worktree lifecycle, interactive session manager, types/{phase,stage,sentinel}, the 29-subcommand CLI surface, SurrealDB-as-authority, the 135K-token instruction corpus.

### 3.3 Hook surface — exactly four custom hooks

| Hook | Event | Purpose |
|---|---|---|
| `git-guard` | PreToolUse (Bash) | Intercept git operations that violate `policy.json.git` (see §6) — the commands git client hooks can't reach |
| `session-orient` | SessionStart | Inject the generated ~30-line project digest |
| `session-summary` | SessionEnd | Append session record (task, branch, decisions touched, PR) to the ledger — recall's zero-ceremony corpus |
| git-hook shims | pre-commit / commit-msg / pre-push | Branch protection, secret scan, staged-.env, commit format (warn), test gate — all reading `policy.json` |

Everything else in-session is **native settings**, shipped as presets: permission deny-read rules for secret file patterns (`**/.env*`, `**/*.pem`, `**/*credentials*`, …), sandbox network allowlist, statusline, worktree default, permission mode chosen at init (`default` / `acceptEdits` / `bypassPermissions`+sandbox). No protection-guard, edit-write-guard, webfetch-guard, gh-pr-guard, gate-check, team-guard, sentinel, or checkpoint code exists in v2.

### 3.4 The session-orient digest — deliberately tiny

Budget: **≤30 lines / <500 tokens, pointers not content.** Generated live by `codeflow orient`, never stored. Contents: product one-liner; current branch + worktree state; active epic/task counts (--full only); capability counts by status; titles of the last 2–3 ADRs; one-line enforcement status (e.g. `gates: git✓ secrets✓ tests✓ remote✓`); paths to read more (`docs/product.md`, `docs/capabilities.md`, `docs/decisions/`). No file contents, no instructions (AGENTS.md owns those), no history dumps. Off-switch in `project.toml` (`orient.enabled = false`); the same digest is what `mcp serve` exposes to other harnesses.

---

## 4. The scaffold and consumer contract

### 4.1 Consumer project tree after `codeflow init --standard`

```
myproject/
├── AGENTS.md                  # canonical instructions (≤32KiB); codeflow-managed block inside markers
├── CLAUDE.md                  # "@AGENTS.md" shim + Claude-specific addenda
├── .claude/
│   ├── settings.json          # codeflow hook entries + permission/sandbox presets, structured-merged
│   ├── agents/                # cf-reviewer.md — the ONE agent definition (see §4.5)
│   ├── skills/                # cf-method (the discipline guide); stack standards arrive via profiles
│   ├── workflows/             # develop (build→review→verify w/ bounded rework loop)
│   └── commands/              # /cf-plan /cf-develop /cf-ship — three, thin
├── .codeflow/
│   ├── project.toml           # tier, scaffold version, stack, areas[], recall.share
│   ├── policy.json            # ALL enforcement policy (user-owned, schema-versioned) — see §6
│   ├── test-config.json       # written by stack detection / `codeflow stack add`
│   ├── manifest.json          # managed files + shipped hashes (the update contract)
│   └── .baseline/             # pristine shipped copies (3-way merge bases)
├── .git/codeflow/             # runtime state — untracked, shared across worktrees
├── docs/
│   ├── product.md             # purpose, users, scope, NON-GOALS (init asks 3 questions)
│   ├── architecture.md        # graduates to architecture/<area>.md
│   ├── capabilities.md        # the registry; graduates to capabilities/CAP-*.md at ~15 entries
│   └── decisions/             # ADRs, append-only; ADR-0001 (stack choice) written by init
├── project-management/        # --full tier: epics/ tasks/ specs/ templates/
└── .github/workflows/codeflow-ci.yml   # written only if absent
```

### 4.2 Tiers

| Tier | Adds | For |
|---|---|---|
| `--minimal` | AGENTS.md + secret-scan pre-commit + gitignore. Branch policy at warn. | Throwaways. Blocking policy on a scratch repo trains bypassing. |
| `--standard` (default) | + full git gates, settings presets, agents/skills/workflows/commands, docs/ (product, architecture, capabilities, ADRs), test gate, recall capture, CI template | Real projects |
| `--full` | + project-management/ (epics, tasks, specs, format IDs, validate gates incl. capability hard-gate) | Projects where work outlives sessions |

Tier is recorded in `project.toml`; re-running init at a higher tier is an **idempotent additive upgrade**. Downgrade = stop managing, never delete.

### 4.3 File-ownership classes (the update contract)

1. **Fully-managed** (agents, skills, workflows, commands, git-hook shims, CI template): hash in manifest; unmodified → replaced on update; user-modified → 3-way merge from `.baseline/`, conflicts produce `.new` + report. Never silently skipped, never clobbered.
2. **Managed-region** (AGENTS.md markers; settings.json keys identified by `codeflow` command prefix): only the region/keys are touched; everything else is yours.
3. **User-owned, schema-versioned** (policy.json, project.toml, product.md, all docs/): updates may *add* new keys with defaults and report them; never mutate user values.

### 4.4 Claude-artifact minimalism (counts, caps, formats)

v1's corpus (8 agents, 14 commands, 7 skills, 1,790-line CLAUDE.md ≈ 135K tokens) is the anti-pattern. v2 ships the minimum that earns its tokens, in current-standard formats, with hard size caps — and the standing rule: **an artifact is added only when usage proves the need, never speculatively.**

| Artifact | Count | What & why it survives | Cap |
|---|---|---|---|
| Agent defs | **1** — `cf-reviewer` | The independent evaluator: verifies against stated acceptance criteria with file:line evidence, runs `codeflow test`/`validate`. The only role split with consistent evidence of value. The lead session IS the developer (Fable-class); parallel builders are spawned with workflow prompts, needing no agent file. QA-as-agent is absorbed: the test gate is mechanical (binary), acceptance verification belongs to the reviewer. | ≤80 lines |
| Skills | **1 core** — `cf-method` (+1 standards skill per installed stack profile) | Carries the mental model: how to plan an epic, when an ADR is warranted, capability-entry discipline, worked examples, anti-patterns. On-demand, SKILL.md standard, portable to other harnesses. | ≤300 lines + resources |
| Commands | **3** — `/cf-plan`, `/cf-develop`, `/cf-ship` | Entry points that gather input clarity then invoke the workflow/skill. No `/cf-status` (the binary + orient cover it), no help/doctor/resume commands (binary or native features). | ≤40 lines each |
| Workflows | **1** — `develop` (ship logic starts inside `/cf-ship` + binary gates; promoted to a workflow only if usage demands) | The build→review→verify pipeline with bounded rework. | — |
| AGENTS.md | 1 | The operating contract. | ≤250 lines (aim ~150); 32KiB hard cap |
| CLAUDE.md | 1 | `@AGENTS.md` + Claude addenda. | ≤15 lines |

Always-loaded budget: AGENTS.md + orient digest ≈ **≤3K tokens** (v1: ~15.5K).

### 4.5 Responsibility split

**Binary owns:** all mechanics (gates, tests, validation, recall, views) — a binary upgrade improves every repo with no re-scaffold, because hooks call `codeflow` from PATH. **`codeflow update` owns:** managed scaffold files, per the classes above. **The consuming project owns:** all content — code, docs, policy values, customized workflows, epics/tasks, AGENTS.md outside the markers. Nothing depends on a human remembering to maintain anything; capture is automatic, views are generated, integrity is linted in CI.

---

## 5. Knowledge model (six layers, one spine)

```
WHY     docs/product.md — purpose, users, scope, non-goals      rare change      human-owned
RULES   AGENTS.md + cf-method skill + standards skills          rare change      co-owned
WHAT    docs/capabilities.md — the registry of what the         per ship         agent-maintained,
        system does (planned→building→shipped→deprecated)                        gate-enforced
HOW     docs/architecture.md + docs/decisions/ (ADRs)           per decision     ADRs append-only
WORK    project-management/ epics + tasks (+ specs as inputs)   daily            agent-maintained
TRACE   ledger + session summaries + recall                     continuous       automatic
```

**The spine: traceability by ID, downward.** Capability → epics that built it → ADRs/specs consumed → PRs → ledger. "Why does the system do X this way" is answerable by following frontmatter links or `codeflow recall`, never by reading all code.

**Capability registry.** Each entry: `id (CAP-###), name, area, status, verified_by (test tags), epics[], adrs[]` + one paragraph. It is the agent's index of the system — the develop workflow consults it before building ("does this exist? what does it touch?"). **Gate:** at `--full`, a FEAT epic cannot close without creating/updating a capability entry (`validate` blocks); at `--standard`, doctor warns.

**ADRs.** Yes, emphatically: append-only (cannot rot), written at the moment of decision (when context is loaded — the cheapest "why" capture), and the best-value reading for a fresh agent session. Lightweight format: context / decision / consequences / architecture-impact / status. Workflows *prompt* at Tier-3 decision points (new dependency, schema change, boundary change) — not one-per-task; ADR over-production is its own swamp. The `architecture-impact` field is the trigger for updating architecture.md in the same ship PR.

**Specs.** Inputs to work, not living docs. Drafted by the plan workflow, frozen (`status: implemented`) when the epic ships; truth then lives in architecture + capabilities + tests. This is the spec-rot fix: specs are allowed to be historical.

**spec-kit / BMAD position.** Concept-source, not dependency. Mapping documented (constitution → product.md non-goals + AGENTS.md principles; /specify → epic+spec; /plan → plan workflow; /tasks → tasks; /implement → develop workflow). Not adopted as machinery: would add a second un-validated structure, network-dependent init, rigid phase gates, and no integration with the ID spine, validate gates, or recall. BMAD rejected outright (role ceremony = what v2 deletes). Weight calibration: OpenSpec-weight, not BMAD-weight. Coexistence is fine; artifacts are plain markdown+frontmatter usable from any harness.

**Maintenance matrix (who, when, why it can't rot):**

| Artifact | Maintainer | Trigger | Rot prevention |
|---|---|---|---|
| product.md | Human (agent proposes only) | Rarely | Small + stable; doctor flags epics whose scope violates non-goals |
| AGENTS.md | codeflow (managed block) + human | Update / as needed | 32KiB cap enforced by doctor |
| capabilities | Agent via ship workflow | Epic completion — same PR | validate gate (--full) / doctor warn (--standard) |
| ADRs | Agent drafts, human accepts in PR | Tier-3 decision points | Append-only; only `superseded-by:` |
| architecture | Agent via ship workflow | ADR with architecture-impact | Impact field is the trigger; docs-lint flags dangling module refs |
| specs | Agent via plan workflow | Frozen at epic completion | Historical by design |
| epics/tasks | Agent via develop workflow | Continuous | Status generated live; frontmatter validated |
| ledger/summaries | Binary (hooks) | Continuous | Append-only, zero ceremony |
| Integrity | Binary: `validate --docs` | CI + pre-push | Dangling links/IDs/test-tags fail loud |

---

## 6. Git discipline and enforcement

### 6.1 Policy is config — `policy.json` `git` section (single source of truth for all planes)

```jsonc
{
  "schema_version": 1,
  "git": {
    "protected_branches": ["main", "master"],     // user-extendable: "release/*", "production", …
                                                  // glob patterns; read by ALL enforcement planes
    "commit_to_protected": "block",               // block | warn | allow
    "push_to_protected": "block",                 // direct push to protected branches (merges go via PR or integrate)
    "force_push_protected": "block",              // always-block recommended; still configurable
    "force_push_unprotected": "allow",            // rebasing feature branches is normal
    "delete_protected": "block",                  // git branch -D / push --delete on protected
    "hard_reset_protected": "block",              // git reset --hard while on a protected branch
    "commit_format": "block",                     // conventional commits: type(scope): description — structure enforced
    "commit_types": ["feat","fix","docs","refactor","test","chore","ci","perf","build","revert"],
    "ai_attribution": "block",                    // no AI attribution anywhere: Co-Authored-By AI trailers,
                                                  //   "Generated with" lines, robot emoji — commits AND PR bodies
    "commit_emoji": "block",                      // no emoji in commit subjects/PR bodies (v1 standard, carried)
    "branch_naming": "block",                     // enforced at pre-push against branch_prefixes
    "branch_prefixes": ["feat/","fix/","docs/","refactor/","test/","chore/","ci/","hotfix/","plan/","spike/","experiment/"],
    "secret_scan": "block",                       // pre-commit content scan + staged-.env
    "test_gate_on_push": "warn"                   // warn | block | off — becomes meaningful once a stack is detected
  }
}
```

Every value is a default, user-flippable per repo; `--minimal` tier flips most to `warn`/`off` except `secret_scan`.

All four enforcement planes read this one config:
1. **Git client hooks** (pre-commit / commit-msg / pre-push) — harness-agnostic, work for any agent or human; shims no-op gracefully if the binary is missing (`command -v codeflow || exit 0`).
2. **Claude `git-guard` hook** (PreToolUse) — intercepts what git hooks can't: `push --force` to protected (pre-push sees the push but git-guard gives instant in-session feedback), `reset --hard` on protected, `branch -D` protected, checkout-and-commit dodges. Same policy file, same globs.
3. **Remote protection** — `codeflow remote protect` applies the same `protected_branches` list to the provider: require PR + green CI before merge, block force-push and deletion. GitHub adapter at launch (via `gh api`, rulesets/branch-protection per plan tier, with a legible report of anything the plan can't apply); GitLab/Bitbucket = provider-pluggable interface + printed manual checklist until an adapter is warranted.
4. **CI** — re-runs tests + secret scan + `validate --docs`; the final arbiter that doesn't depend on any local state.

### 6.2 Merging into protected branches — the sanctioned paths

Blocking commits/pushes on protected branches raises the question: how does work *land*? Exactly two sanctioned paths, both gate-passing:

- **Remote path (default):** PR → CI green → merge (auto-merge on green where enabled). Remote protection enforces it.
- **Local path (no-remote or offline):** `codeflow integrate <branch> --into main` — the flock-guarded rebase → test → merge primitive. It runs with a gate-context token (env var set by the binary, verified by the git hooks/git-guard), so the merge commit on the protected branch is permitted **only** through integrate. Raw `git merge` on main stays blocked.

Worktree doctrine: **worktree-per-session as a strong default** (native `--worktree`; AGENTS.md instructs it; doctor warns when an agent session edits on the root protected-branch checkout — warn, not block, so a human one-liner on a branch from root stays painless). With protected branches checked out only at the repo root, git itself refuses a second checkout in any worktree — structural local protection for free.

### 6.3 Secrets (three layers from minute one)

1. Scaffold `.gitignore` (env/key/cert/credential patterns).
2. `pre-commit` content scan (imported v1 scanner) + staged-.env block — works for every harness and human.
3. Native Claude permission **deny-read rules** for secret file patterns — declarative settings, no custom code.

### 6.4 Branch & commit standards (carried from v1, strict)

The v1 discipline survives intact — it was always on the keep-list, and it's now config (D7), not code:

- **Branches:** `{prefix}/{kebab-name}` with the v1 prefix set (`feat/ fix/ docs/ refactor/ test/ chore/ ci/ hotfix/ plan/ spike/ experiment/`). Enforced at pre-push (**block**). Map: work intent → prefix is documented in AGENTS.md and `cf-method`.
- **Commits:** conventional format `type(scope): description` — imperative mood, lower-case type from the whitelist, no trailing period, body explains *why* when non-obvious. Structure enforced at commit-msg (**block**).
- **No AI attribution, ever** (**block**): no `Co-Authored-By` AI trailers, no "Generated with …" lines, no robot emoji — in commit messages *and* PR bodies. Scanned at commit-msg and at PR creation (git-guard intercepts `gh pr create`). This is project policy and overrides any harness default that injects attribution.
- **No emoji** in commit subjects or PR bodies (**block**, v1 standard).
- **PR bodies:** template-driven (summary, changes, test results, linked epic/capability IDs) — guided by the template + reviewer instruction rather than a parser hook (the one v1 mechanism deliberately softened).
- One logical change per commit; squash-merge to protected branches keeps history linear (set in remote protection).

### 6.5 The hard line: CI + remote protection (consistency rule)

Local enforcement — git hooks, git-guard, the `integrate` gate-token — is **fast feedback and discipline support**; any of it can be bypassed by a determined human (`--no-verify`, env vars), and that's accepted by design. The **hard line is the perimeter: CI + remote branch protection.** Concretely, and consistently across the whole design: CI re-runs every gate that matters (tests, secret scan, `validate --docs`, commit-format/attribution lint over the PR's commits) so nothing depends on local state; remote protection (D7's same `protected_branches` list) requires PR + green CI and blocks force-push/deletion, so nothing reaches a protected branch without passing the perimeter. Local layers exist to catch mistakes in seconds instead of at CI-time — never as the only line. `doctor`'s enforcement matrix reports all planes, marking CI/remote as the authoritative ones.

### 6.6 What is deliberately NOT enforced

Process steps, phase ordering, role boundaries, review-before-X sequencing, task-tracker mirroring — all instruction-level now. Models at Fable/Codex-5.5 level follow the operating contract; the gates exist only where a mistake is irreversible or invisible (history rewrites, leaked secrets, untested merges, doc-graph corruption).

---

## 7. Task management (three layers + graduation)

1. **In-session (native, ephemeral, free):** Claude's task tools — dependency tracking, claiming across parallel agents. CodeFlow adds nothing. For small projects this is the whole system, correctly.
2. **Durable planning (`--full`):** epics/tasks as markdown + YAML frontmatter (simplified format IDs), validated; maintained by the agent as a side effect of the develop/ship workflows; reviewed as diffs in the same PR as code. Markdown is the source of truth; the JSONL ledger is the event log; indexes are rebuildable caches. No database authority, no mirroring mandates.
3. **Cross-repo view:** `~/.codeflow/registry.json` (flock'd, upserted by every command run) + `codeflow status --all` + `recall --all`. A view, not a system. No daemon — lazy sync at query time via cursors.

Graduation rule: start at layer 1; add `--full` when work outlives sessions. The tool's job is making the right weight available, not imposing maximum weight on day one.

---

## 8. Memory and recall

- **Capture (zero-ceremony):** `session-summary` hook appends session records; PR bodies and merge commits ingested; ADRs/epics/capabilities are themselves the curated memory. Claude auto-memory handles prose context natively.
- **Recall:** `codeflow recall [--all] "<query>"` — SQLite FTS5 (bundled, no system deps) over ledgers, summaries, ADRs, epics, capabilities across registered repos. Results disclose coverage gaps ("no session records for 6/3–6/5 — non-Claude days").
- **Not built:** embeddings, vector DBs, GraphRAG extraction, daemons, SurrealDB-as-server (the evidence: agentic grep won; file-based memory is the industry-converged shape). If graph semantics ever prove necessary, adopt Graphiti/Beads via MCP rather than building.
- `~/.codeflow/`: `registry.json`, `recall.db`, `config.toml` (user defaults: policy base, harness prefs). Optional later: `recall --archive` indexing `archive/v1` for v1-era "why" questions.

---

## 9. Cross-harness story

- **AGENTS.md canonical** (≤32KiB — Codex cap), read natively by Codex and Cursor; `CLAUDE.md` = `@AGENTS.md` + Claude addenda (Anthropic's documented pattern).
- **Skills** follow the SKILL.md open standard — `cf-method` and stack skills load in other harnesses.
- **Git hooks + CI are the canonical enforcement plane** — identical behavior under any harness or none.
- **`codeflow doctor --harness codex|cursor`** prints the live enforcement matrix (active / degraded / dead per guard per plane) and emits MCP wiring snippets. Degradation is always legible.
- **`codeflow mcp serve`** (read-only first): workgraph queries, recall, orient — any harness reads the same project state.

---

## 10. Distribution and updates

- **Install:** GitHub Releases via cargo-dist (shell/powershell installers; pinned dist version) + `cargo install` / `cargo binstall`. SQLite bundled. No Homebrew/npm until demand.
- **Scaffold pre-packaged in the binary** via rust-embed (debug builds load from disk for instant scaffold iteration). Init is offline, instant, version-locked: scaffold version ≡ binary version, recorded in project.toml + manifest.
- **Two-motion updates:** upgrade binary (improves all repos immediately — hooks call PATH) → `codeflow update [--all]` per repo for scaffold files. Version-skew warning on every command until done.
- **No template repo.** Init is canonical; templates drift, manifests don't.
- **Plugin tier (deferred, gated):** the `.claude/` layer as a Claude Code plugin for people who won't run init — only when a second human asks. Plugins structurally cannot be primary (cannot write AGENTS.md, git hooks, CI, or repo config).
- **Multi-model delegation (deferred, trails):** `cf-delegate` skill + thin adapters shelling to other vendors' CLIs on their own subscription auth (harness-boundary composition — the ToS-compliant shape); routing config in `orchestration` section of policy later. Not in v2.0.0.

---

## 11. The codeflow repo: v2 shape and transition

### 11.1 Transition — clean slate in the same repo

1. `git tag v1-final` + `git branch archive/v1` at current HEAD. Everything browsable/recoverable forever; **the archive branch IS the archive — no archive folders in the v2 working tree.**
2. One **"v2 reset" commit on main**: empty the tree, lay the new skeleton. History stays connected (no orphan, no force-push; clones and the remote unaffected) but the working tree is pristine.
3. **Selective import** from `archive/v1` (`git checkout archive/v1 -- <path>` + trim): the burden of proof flips — modules justify being imported, not being deleted. Nothing legacy enters by accident.
4. v1's 50 epics stay in the archive; **v2 PM numbering starts fresh** in the new tree.

### 11.2 v2 repo tree

```
codeflow/
├── crates/
│   ├── codeflow-core/        # engine library (§3.2 modules)
│   └── codeflow-cli/         # binary; embeds assets/ via rust-embed
├── assets/
│   ├── base/                 # the scaffold (§4.1 sources): AGENTS.md.tmpl, CLAUDE.md.tmpl,
│   │                         #   claude/{agents,skills,workflows,commands}, settings presets,
│   │                         #   policy.json, git-hooks/, ci/, docs templates (product/arch/
│   │                         #   capabilities/ADR), pm/ templates
│   └── profiles/             # rust/ node/ python/ overlays
├── docs/
│   ├── plan/v2/              # this charter, split: charter, knowledge-model, enforcement,
│   │                         #   execution, decisions/ (ADRs of the rebaseline itself)
│   ├── product.md  architecture.md  capabilities.md  decisions/     # dogfood (own knowledge model)
├── project-management/       # dogfood, fresh numbering: EPC-001 = v2 bootstrap
├── .claude/ .codeflow/ AGENTS.md CLAUDE.md          # dogfood: runs on its own scaffold
└── README.md                 # install → init → go
```

The repo **builds the product** (binary with embedded assets) and is merely its own first consumer. `assets/` is as much the product as the code.

---

## 12. Execution plan (~2–3 days, parallel agents in isolated worktrees)

**Context:** orchestrated from the parent-dir session (v1 hooks don't fire); manual discipline until self-hosted: conventional commits, feature branches, PR per workstream where practical; each workstream's gate = its imported/built tests green.

**Day 0 (hours):**
- Tag `v1-final`, branch `archive/v1`, v2-reset commit + skeleton on main.
- Commit `docs/plan/v2/` (this charter split into sections + rebaseline ADRs).
- Probe: native workflow script expressing the develop rework loop (build→review→changes-requested→build, bounded) — Door A: pure native scripts; Door B: thinnest stage helper, stamped disposable.

**Day 1 — five parallel workstreams (no file overlap; Cargo.toml/lib.rs merges at integration):**
- **A — Testing engine import:** `core/testing` + test-config schema + templates; green.
- **B — Records import:** workgraph, ledger, store (cache-demoted), validate (+ new `--docs` lint), models subset; green.
- **C — Guards import:** security scanner, git/conflict + ci_wait, file_lock, error (pruned), doctor core, settings (simplified); green.
- **D — Scaffold engine (new):** init/update, manifest, ownership classes, 3-way merge, rust-embed wiring, `--yes` flow incl. the 3 product questions + permission-preset choice; the critical path (~1.5–2 agent-days).
- **E — Corpus authoring (new):** AGENTS.md template, CLAUDE.md shim, 3 agents, `cf-method` skill, develop/ship workflows, 4 commands, settings presets (incl. deny-read secret patterns, statusline, sandbox), policy.json defaults (§6.1), git-hook shims, CI template, docs templates (product/architecture/capabilities/ADR + ADR-0001 example), epic/task/spec templates.

**Day 2 — new small builds + integration:**
- `git-guard`, `integrate` (with gate-context token), `recall` + `orient` + `session-summary`, `remote protect` (GitHub), `status`.
- Wire init end-to-end; integrate workstreams; full suite green; `doctor` clean.

**Day 3 — dogfood + release:**
- `codeflow init` on the codeflow repo itself (consumer #1) — its own gates now protect it; create `EPC-001` retroactively documenting the bootstrap.
- `codeflow init` on the user's first real project; fix frictions (the only legitimate backlog).
- README/docs, tag `v2.0.0`, cargo-dist release pipeline.
- Trailing (same week, not blocking): `mcp serve`, `doctor --harness` polish, `recall --archive`, cf-delegate skill.

---

## 13. Decision log (to be re-recorded as ADRs in `docs/plan/v2/decisions/`)

| # | Decision | Rationale (short) |
|---|---|---|
| D1 | Binary + embedded scaffold; not framework/harness/plugin-first | Beads model proven; plugins can't write AGENTS.md/CI/git hooks |
| D2 | AGENTS.md canonical, CLAUDE.md shim | Cross-tool standard; Anthropic-documented pattern |
| D3 | Merge queue dropped for `integrate` primitive | No daemon = no pump; GitHub auto-merge serializes; FIFO solves a non-problem solo |
| D4 | Git hooks + CI = canonical enforcement plane; Claude hooks = fast feedback | Harness-agnostic; legible degradation |
| D5 | Exactly 4 custom hooks; native settings replace v1 guard hooks | Native deny/ask/sandbox covers it; less code to maintain |
| D6 | No process/phase/role enforcement | Fable-class instruction-following; gates only where mistakes are irreversible/invisible |
| D7 | Protected branches and all git policy config-driven (`policy.json.git`, globbed), read by all planes | User-extendable (release/*, etc.); one source of truth |
| D8 | Push + force-push + delete + hard-reset to protected blocked; force-push to non-protected allowed | Protect history where it matters; don't obstruct feature-branch rebasing |
| D9 | Protected-branch merges only via PR or `integrate` (gate-context token) | Sanctioned paths both run the gates |
| D10 | Knowledge model: 6 layers; capability registry gate at --full, warn at --standard | Traceability spine; rot prevention via same-PR updates |
| D11 | ADRs yes (append-only); specs frozen at ship | Can't rot; spec-rot fix |
| D12 | spec-kit/BMAD: concepts mapped, machinery not adopted | Avoid second structure; keep validate/recall integration |
| D13 | Tag + archive branch + reset commit + selective import (same repo, connected history) | Clean slate without cleanup-as-a-process; archive = the archive |
| D14 | v2 PM numbering fresh; v1 epics live in archive | Clean working tree; `recall --archive` later if wanted |
| D15 | Worktree-per-session strong default; doctor warns on root-checkout edits (not block) | Structural main protection; human one-liners stay painless |
| D16 | ~~Conventional commits warn by default~~ **Revised:** commit format, branch naming, no-AI-attribution, no-emoji all **block** by default (v1 strictness carried); `--minimal` tier softens to warn except secrets | User direction: strict commit/branch standards are core discipline |
| D17 | No SurrealDB authority; markdown+JSONL truth, FTS5 cache; no embeddings/GraphRAG/daemon | Evidence-based (retrieval verdict); adopt Graphiti/Beads via MCP if ever needed |
| D18 | Multi-user, GitLab/Bitbucket adapters, plugin tier, cf-delegate: deferred behind explicit gates | Build for the actual user first |
| D19 | CI + remote protection are the authoritative enforcement plane; all local layers are fast feedback by design | Consistency: nothing depends on bypassable local state |
| D20 | Artifact minimalism: 1 agent (cf-reviewer), 1 core skill (cf-method), 3 commands, 1 workflow; size caps per §4.4; artifacts added only when usage proves need | v1's 135K-token corpus is the anti-pattern |
| D21 | Orient digest ≤30 lines / <500 tokens, pointers-only, off-switchable | Always-loaded context is the most expensive real estate |
| D22 | Nothing crosses from v1 unexamined: code imported only via keep-decision + trim + re-test; docs/templates/Claude artifacts always re-authored fresh at v2 weight, never copied | v1 is a quarry, not a source tree; prevents legacy style/weight leaking into v2 |

## 14. Deferred / out of scope (explicit)

Multi-user infrastructure (CRDT/sync — archived, ~3–4 weeks to revive if ever needed); Tauri/GUI (gated: ≥10 weekly external users or a paying request); model orchestration as engine code (skill + adapters later); GitLab/Bitbucket remote adapters (manual checklist until warranted); plugin distribution tier; embeddings/knowledge-graph memory; TUI dashboards; the v1 autorun batch system (native background sessions + `integrate` replace it; a <500-line driver only if dogfooding demands).

## 15. Risks

| Risk | Mitigation |
|---|---|
| Native harness churn (workflows/teams are partly experimental) | Kept assets are harness-independent (policy, records, tests, markdown); only thin workflow scripts are coupled — cheap to rewrite; quarterly review |
| Import surprises (hidden deps in kept modules) | Compile-gated import order; trim or stub at the boundary; feasibility scan already mapped the known edges |
| Scaffold/update trust (the S6 failure) | Ownership classes + baselines + 3-way merge + always-report; this is treated as the core product, built Day 1 not bolted on |
| `integrate` gate-token bypass (someone exports the env var) | It's a discipline aid for agents, not a security boundary; CI + remote protection are the hard line |
| Solo-maintainer bus factor / motivation | v2's surface is ~⅓ of v1's; joy budget lives in recall/orient polish; quarterly obsolescence review keeps scope honest |

## 16. Acceptance criteria for v2.0.0

1. `codeflow init --standard --yes` in an empty dir → first feature → PR flow works within the first hour with zero policy walls (bootstrap grace verified).
2. Init into a repo with existing CLAUDE.md/settings/husky merges non-destructively with a printed report.
3. All four enforcement planes read `policy.json.git`; adding `release/*` to `protected_branches` is reflected in git hooks, git-guard, and `remote protect` with no code change.
4. Force-push to a feature branch: allowed. Force-push/push/delete/hard-reset against protected: blocked locally (both planes) and remotely (GitHub).
5. Raw `git merge` into main blocked; `codeflow integrate` and PR+CI both land work.
6. Secret-file read denied in-session (native rules); secret commit blocked at pre-commit; both visible in `doctor`'s matrix.
7. `codeflow test` detects the stack at runtime; no-stack = loud no-op; with stack = real gate.
8. `codeflow update` on a repo with a customized workflow: 3-way merge or `.new` + report; never clobber, never silent-skip.
9. `validate --docs` fails CI on dangling capability/epic/ADR links.
10. `recall` answers a "why" question from session summaries + ADRs across ≥2 registered repos.
11. The codeflow repo itself runs on its own scaffold (consumer #1), full suite green, doctor clean.
12. One real user project initialized and actively developed through the system.
13. Commit with a malformed subject, an AI-attribution trailer, or emoji: blocked at commit-msg; same content in a PR body: blocked at `gh pr create` (git-guard) and failed by the CI lint — verified on a protected and a non-protected branch.
14. Always-loaded context (AGENTS.md + orient digest) measures ≤3K tokens; no Claude artifact exceeds its §4.4 cap; every scaffold artifact is newly authored (no v1 file content reachable via diff against archive/v1).
