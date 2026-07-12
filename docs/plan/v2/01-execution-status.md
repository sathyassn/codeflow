# v2 Execution Status

Live tracker for the charter's §12 execution plan. Updated as waves complete.

## Day 0 — transition

- [x] `v1-final` tag + `archive/v1` branch created and pushed to origin
- [x] Stale session worktrees deregistered and removed
- [x] v1 local git hooks (symlinks into `.codeflow/scripts/git-hooks/`) superseded: `core.hooksPath` → `.git/hooks-v2/` with transition hooks (protected-branch commit/push block via `CODEFLOW_TRANSITION` escape, staged-.env block, conventional-commit + no-AI-attribution check). v1 hook content preserved in `archive/v1` and `.git/hooks-v1-backup/`
- [x] Tree reset: all v1 content removed from working tree (preserved in archive)
- [x] v2 skeleton: README, .gitignore, `docs/plan/v2/`
- [x] Cargo workspace skeleton (`crates/codeflow-core`, `crates/codeflow-cli`, `assets/`) compiling
- [x] Push main
- [x] Probe: native workflow rework-loop (charter §12 Day 0) — verdict Door A (native scripts suffice, no bespoke runtime); analysis + draft script in `docs/plan/v2/probe/`

## Day 1 — parallel workstreams

- [x] A — testing engine import: 327 tests green, clippy clean (23 files; config path moved to `.codeflow/test-config.json`; schema+9 templates shipped in `assets/base/testing/`)
- [x] B — records import: 180 tests green, clippy clean (models/ledger/workgraph/validate; SurrealDB NOT imported — `RecordStore` trait + `MarkdownStore` over markdown+frontmatter per D17; format ids simplified to `EPC-NNN`/`TSK-NNN-NNN`)
- [x] C — guards import: 376 tests green, clippy clean (security scanner ×10 modules, git/conflict + ci, file_lock, error pruned 1178→476 lines, doctor v2 check table, settings structured-merge per §4.3 class 2)
- [x] D — scaffold engine: 43 tests green, clippy clean (init w/ bootstrap grace + tier system + husky/hooksPath detection; update w/ 3-way merge via .baseline + `.new` conflicts + additive policy key sync; managed-region + structured settings merge; scaffold-manifest.toml spec; rust-embed CLI with disk-loading in debug; version-skew warning). Integrated: **926 tests green**, clippy 0; asset gaps (gitignore, develop.workflow.js) filled at integration
- [x] E — corpus authoring, all §4.4 caps respected: AGENTS.md.tmpl 116 lines, CLAUDE.md.tmpl 14, cf-reviewer 64, cf-method 131, 3 commands ≤26, policy.json (§6.1 exact), git-hook shims, 3 settings presets, CI template, docs+pm templates; repo's own AGENTS.md/CLAUDE.md instantiated (dogfood)
- [x] Integration: probe+E+A+B+C merged serially into main; **883 tests green** (exact workstream sum), **clippy 0 warnings workspace-wide**

## Day 2 — new builds + integration

- [x] F — hooks plane: 170 tests green (git-guard PreToolUse handler, git client
  hook stages, orient digest, session-summary ledger capture; end-to-end hook
  coverage in tempdir repos)
- [x] G — work-landing flow: 56 tests green (test gate with config + runtime
  stack resolution, `validate --docs` referential-integrity lint, generated
  status view, integrate primitive with gate-context token + path flock)
- [x] H — recall + remote: 39 tests green (FTS5 recall, `~/.codeflow`
  registry, remote protect GitHub adapter with degradation report)
- [x] Integration: F/G/H landed serially; integrated mains 1080 → 1181 tests
  green, clippy 0 workspace-wide (suite now 1207 after Day 3 and the docs-truth
  and engine-fix PRs below)

## Day 3 — dogfood + release

- [x] Binary installed on PATH: `codeflow 2.0.0-dev` (replacing the v1 binary)
- [x] `codeflow init` dogfood on this repo (consumer #1), idempotent rerun
  verified; full-tier additive upgrade (`project-management/`) on the closure
  branch
- [x] Doctor wired and clean (6 checks at Day 3; `repo-integrity` added later via
  ADR-0007 → now 7)
- [x] `codeflow integrate` used for all landings (4 integrations)
- [x] Live AC demos: #1, #3, #5, #6, #13 — including the `policy_armed` bug
  found during #1, fixed with a regression test (612ec23d)
- [x] EPC-001 retroactive bootstrap record (`project-management/epics/EPC-001/`)
- [x] AC #4 (force-push/push/delete/hard-reset blocked on protected;
  force-push allowed on feature branches) and AC #10 (recall answers a "why"
  from summaries + ADRs across ≥2 registered repos) demonstrated live in the
  parallel enforcement/recall wave
- [ ] `codeflow init` on first real user project (charter AC #12) — deferred past
  the v2.0.0 release; not a release blocker
- [x] v2.0.0 tagged (2026-07-03) and cargo-dist GitHub Release published: shell
  installer + 3 platform tarballs (aarch64/x86_64-apple-darwin,
  x86_64-unknown-linux-gnu). Repo stays **private** — the anonymous `curl | sh`
  installer does not work (GitHub 404s private release assets via the browser
  download URL even with a token); working installs are `cargo install --path
  crates/codeflow-cli` from a checkout, or `gh release download v2.0.0 -R
  sathyassn/codeflow -p '<platform>.tar.xz'` then extract onto PATH (see README /
  docs/adoption.md)

## Notes / deviations

- Sandbox guardrails required explicit bypasses for: `.git/config` write (hooksPath), worktree dir deletion, residual `.claude/` deletion. All sanctioned transition steps; logged here for the record.
- Remote protection unavailable on this repo (private + GitHub Free, API 403). Perimeter = CI + discipline until public/Pro. `codeflow remote protect` must report this exact degradation (charter AC #4 caveat verified empirically). Follow-on: ADR-0002 sets `push_to_protected = "warn"` here so the integrated main can sync to origin.
- Nuance (Day 3): a raw local ff-merge while on a protected branch fires no git hook (no commit or push event to intercept), but the result cannot be pushed — the perimeter holds per D19. Local layers are fast feedback, not the line.
- Amendment: `stack add` is removed from the planned CLI surface and `assets/profiles` is dropped — stack setup is agent-led via `/cf-stack`, with the test-config templates as the deterministic substrate (ADR-0003).
- Pipeline mechanics validated live (run wf_74f22cff-d1a: per-stage model, schema verdicts, bounded rework — sabotaged build caught, recovered in 2 attempts); composable pipeline ships user-owned per ADR-0004, evidence artifact at `docs/plan/v2/probe/pipeline-validation.workflow.js`.
- Cross-vendor delegation shipped (CAP-009, ADR-0005): `cf-delegate` skill + `cf-consult` command + optional pipeline `consult` stage + a `delegates` doctor check, no engine orchestration. Live-validated on this machine 2026-07-02 under the user's ChatGPT subscription (`codex login status` exit 0): a read-only `codex exec --json` consult on `registry.rs` (thread `019f23e5-7bd7-7842-8a26-009e5a652759`) returned VERDICT changes_requested with a real finding — the prune uses `Path::exists()` (registry.rs:204), which collapses permission/transient stat errors into "missing" and could permanently drop a live repo row; it recommends `try_exists()`. A `codex exec resume` follow-up retained thread context and prioritized that same finding. Both calls succeeded; the read-only sandbox blocked codex from running `cargo test` and it reported that degradation legibly.
- Composable pipeline validated end-to-end on the SHIPPED file (run wf_4de9066e-9d0:
  2-stage preset composed via args, evidence-based approval, 1 attempt). The live-test
  loop fixed three shipping defects first: missing meta export + args-as-JSON-string
  runtime quirk (040a7f60), schema_version "1" lenience (f784fafc). Bounded-rework
  exhaustion behavior proven by run wf_1380de05-e8f (threw loudly with findings).
- PR-based landings are now active on this repo (ADR-0006, superseding ADR-0002):
  `push_to_protected` returns to `block` and work lands via `gh pr create` →
  green CI → squash-merge. PR #330 (`chore(policy): switch this repo to pr-based
  landings`) was the first; `codeflow integrate` stays a shipped capability but
  is retired for this repo's day-to-day landings.
- Docs-truth audit findings fixed across two PRs: the docs-truth PR (product and
  architecture stubs, CLAUDE.md dedup, capability + hard-gate/lint overclaims,
  AGENTS entry points, the cf-delegate write invocation, new `docs/adoption.md`)
  and the parallel engine PR (test-mode vocabulary, managed-region wrap, real CI).
- Coverage/CI job root cause — 5 environment-dependent tests (the conflict helper
  and the doctor probe) — fixed in the parallel engine PR, so the suite is green
  in CI, not just locally.
- Audit cycle closed (2026-07-02): the docs-truth + test-truth audit's 12 verified
  defects fixed across three PRs — #331 (test-gate vocabulary so quick resolves and
  the pre-push gate fires; managed-region wrap dedup; recursive flat+nested epic
  discovery; runner-env test fixes incl. a production rebase-identity fallback and
  a gh BrokenPipe race; real CI perimeter: from-source gates, blocking rust job,
  pinned toolchain, gitleaks binary + allowlist), #332 (real product/architecture
  content, adoption doc, capability epics[] spine populated, overclaims corrected,
  cf-delegate write invocation live-verified), #333 (Epic model reads canonical
  planning frontmatter; status counts hand-authored epics). Verified on merged
  main: status "epics 1 / capabilities 9 shipped", validate --docs clean, doctor
  6/6, quick test gate executing. Incident note: the root repo was found
  core.bare=true with main checked out in a fix agent's worktree — recovered.
  Attribution corrected after the agent's rebuttal: its refresh used only
  fetch+merge from its own branch; the likelier trigger is `gh pr merge
  --delete-branch` moving off a branch checked out in a worktree (integrator's
  command), or the agent's sandbox-interrupted first worktree-add. Candidate
  doctor check should detect the SYMPTOM (bare flag on a working repo,
  protected branch checked out outside root) regardless of trigger.
- Merge controls shipped (ADR-0007): `merge_to_protected` + `pr_merge_to_protected`
  policy keys, a `pre-merge-commit` git-hook stage (with the honest ff-merge
  boundary), git-guard interception of `git merge`/`cherry-pick`/`gh pr merge`
  plus anti-laundering of override envs and a `gh pr merge --delete-branch`
  block, the human-only `CODEFLOW_HUMAN_OVERRIDE` (git layer only), and the
  `repo-integrity` doctor check (doctor 6→7). After this merges, agent-performed
  `gh pr merge` into a protected base is blocked — a human merges subsequent PRs.
- Root cause of the four `core.bare` flips + a stray `chore: initial commit`
  identified and fixed: git exports `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`
  to hook subprocesses, and the pre-push `test_gate_on_push` ran `cargo test`
  with those inherited, so the workspace's git-spawning tests mutated the real
  repo instead of their tempdirs. Fix: the test-gate runner (`spawn_command`)
  and the test git helpers clear the three vars. This supersedes the
  worktree-interruption *correlation* in ADR-0007's wording; the ADR stays as
  the historical record since it already states the mechanism was unconfirmed.
- Harness parity + exec-guard shipped (ADR-0008): a `codeflow hook exec-guard`
  PreToolUse (Bash) stage runs the dangerous/privilege security modules per a
  new `policy.json` `security` section (dangerous=block, privilege=warn); the
  three Claude presets adopt the autonomy posture (allow the project toolchain,
  ask for sudo/publish/delete, deny home-dir credential reads) and wire both
  guards; a `.codex/` starter (hooks.json + config.toml) wires the guards for
  Codex; the init default preset flips to acceptEdits; `bypass-sandboxed` fixes
  the sandbox key to the schema's `allowedDomains` and adds `filesystem.denyRead`.
  Live validation (codex-cli 0.142.5), recorded honestly: the git-hook plane
  refuses a Codex-driven force-push to main (`pre-push: BLOCKED — git.push_to_protected`)
  and the hook payload schema is byte-compatible — but headless `codex exec` did
  NOT run project PreToolUse hooks in testing (observable marker hook never fired
  for either the hooks.json or inline-TOML form, with layer trust +
  `--dangerously-bypass-hook-trust` + `features.hooks=true`), so the `.codex/`
  in-session guards are an interactive-Codex safeguard and headless Codex leans
  on the git-hook plane. agy deferred (dialect differs, macOS reliability open)
  with a manual experimental snippet in cf-delegate.
