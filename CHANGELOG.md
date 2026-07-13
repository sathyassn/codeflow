# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Security: three enforcement-bypass fixes from the pre-release review.**
  The destructive-command guard now tokenizes `rm` instead of pattern-matching,
  so `rm -r -f /`, `rm --recursive --force /`, `rm -rf -- /`, and `rm -rf $HOME`
  are blocked, not just the exact `rm -rf` spelling. The commit-standard merge
  exemption keys off a real merge (`MERGE_HEAD`), not the subject text, so a
  one-parent commit named `Merge ...` is fully checked. `codeflow update` rejects
  a manifest `dest` that is absolute or escapes the repo with `..`, closing an
  arbitrary out-of-repo file deletion via a tampered manifest.
- **Coverage thresholds now fail the test gate.** A configured per-file
  coverage threshold that a measured file misses fails `codeflow test --mode
  full` and the integrate gate, instead of being collected and silently
  ignored. A run with no coverage data recorded stays informational.
- **PR bodies now demand test evidence and digestible bullets.** The shipped
  PR template's `Verification` section becomes `## Testing` — required for any
  code change, carrying pasted test-summary output, the coverage number, the
  new tests added, manual/e2e evidence, and a plain "not tested" statement
  ("tests pass" as prose is a claim, not evidence). Every section is short
  one-line bullets — no paragraph-walls. The rule ships in the template, the
  AGENTS contracts (full and minimal), and `cf-ship`.
- **The Claude+codex duo is the default for all dev work** (operator policy;
  reverses the stakes-gated design). `cf-model-orchestrator` now runs for any
  feature, change, fix, or doc change with acceptance criteria — codex
  cross-reviews the plan, executes, and first-tests; Claude orchestrates and
  final-reviews; each model reviews the other's work. Solo `/cf-develop` is
  the degradation path only, when the duo is unavailable (seat/plugin/auth).
  `cf-plan` names the duo as the default execution skill; `cf-method` and the
  scaffolded AGENTS.md entry-point table match. The codex pin stays
  `gpt-5.6-sol` at `xhigh` (the verified API ceiling on ChatGPT OAuth).
- **Enforcement is the floor; the tiers scale project-management (ADR-0019).**
  The `--minimal` tier now installs the complete four-plane enforcement floor,
  not just the pre-commit secret scan: the `commit-msg`, `pre-push`,
  `pre-merge-commit`, and `reference-transaction` git hooks, the scaffolded CI
  workflow (`codeflow-ci.yml`), the in-session `git-guard`/`exec-guard` +
  orient/summary hooks (`.claude/settings.json` and the `.codex/` starter), and a
  new lean `CLAUDE.md` all moved into the minimal tier alongside the armed policy
  it already shipped. `--standard` and `--full` are unchanged; they still add the
  method (cf-* skills, reviewer agents, the pipeline), the traceability spine, and
  project-management on top. The change is additive — nothing is removed from any
  tier.
- **Existing `--minimal` installs gain the enforcement floor automatically on
  their next `codeflow update`.** The update reconciliation installs manifest
  entries that are in-tier but missing on disk, so an old-minimal repo's next
  update adds the moved hooks, CI, settings, and Codex starter and records them —
  no re-init required.
- **The original commit standard is restored and block-enforced (ADR-0020).**
  The subject description is capped at 50 chars and the whole subject line at 72,
  and a commit body is again only `-` bullets (at most 3, each ≤ 72 chars) plus
  an optional `BREAKING CHANGE:` footer — a prose "story" body is now a blocked
  mistake. The rules ship armed at every tier via five new `git` policy keys
  (`commit_desc_max_len`, `commit_subject_max_len`, `commit_body`,
  `commit_body_max_bullets`, `commit_body_bullet_max_len`), added to an existing
  `policy.json` with their defaults on the next `codeflow update`.
- **A contract-surface tripwire nudges breaking-change discipline (ADR-0020).**
  A new `git.breaking_watch_paths` key (path globs, default empty) makes the
  commit-msg check — and `codeflow ci`, which reuses it — emit a WARN (never a
  block) when a commit touches a declared contract surface without a `type!:`
  subject marker or a `BREAKING CHANGE:` footer. Detection of a break stays a
  judgment call; the glob only prompts a confirm.

### Added

- **A lazy PR body now fails CI mechanically.** When `codeflow ci` is given a
  PR/MR body, it checks the body's structure against three new `git` policy
  keys: `pr_sections` (level, default `block`) governs the check;
  `pr_required_sections` (default `["Summary", "Changes"]`) are headings every
  PR body must carry with real content — a section holding only template
  comments and bare `-` bullets counts as missing; `pr_code_sections` (default
  `["Testing"]`) are required only when the commit range touches non-docs
  files (docs-only = every changed path is `*.md`, `*.txt`, `LICENSE*`,
  `docs/**`, or a `.github` template — anything else, or a range whose files
  cannot be listed, counts as code). Leftover template placeholders — the
  paste-your-output stub, table rows of empty cells, bare `- CAP-`/`- EPC-`
  bullets — draw a warning naming their line, never a block. A run without a
  PR body skips the check, so local `codeflow ci` is unchanged; an existing
  `policy.json` gains the three keys with their defaults on the next
  `codeflow update`.
- **`codeflow policy explain` / `policy show` — the policy file is fully
  discoverable from the binary.** `explain` renders the complete
  `.codeflow/policy.json` key schema — every key's type, default (rendered live
  from the built-in defaults), valid values, purpose, and sharp edges (e.g.
  `allow` and `off` are both inactive levels) — grouped top-level/git/security;
  `show` prints the EFFECTIVE policy: each key's current value, whether it comes
  from the project file or the built-in default, and a loud flag on invalid
  values. Consumers get only the binary, so both need no source access; the
  schema registry is pinned to the policy struct's serde fields by a
  drift-guard test, so a new key cannot ship undocumented.
- **Strict `policy.json` validation — invalid config fails loudly, never a
  silent default-revert.** Malformed JSON, an unknown key, a wrong-typed value,
  an invalid enum value, or an unparseable `commit_ticket_pattern` regex is now
  a hard error naming every offending key, its value, and the valid set (e.g.
  `invalid value 'worn' for git.commit_ticket_required; expected one of: off,
  warn, allow, block`) — surfaced at the commit-msg git hook (exit 1),
  `codeflow ci` (exit 2, nothing verified), and `codeflow validate` (exit 1).
  Previously one invalid value made the whole file fail-parse and silently
  reverted EVERY key to the built-in defaults — including keys a consumer had
  hardened past them. The enforcement loaders keep their fail-safe fallback;
  the loud check is an explicit pre-check at those three surfaces.
- **Opt-in footer trailers and required footers, strict by default (ADR-0020).**
  The commit body stays bullets + a `BREAKING CHANGE:` footer only — every other
  trailer blocks — but a project can now open specific escape hatches via
  `policy.json`, all empty by default: `git.commit_footer_tokens` *allows* named
  trailers (e.g. `Signed-off-by`), and `git.commit_required_footers` *requires*
  them on every commit (e.g. DCO sign-off). Deliberately no populated default —
  in an agent-driven repo every default-allowed trailer is a slot an agent fills.
  Even when `Co-authored-by` is opted in, the `ai_attribution` rule still blocks
  an AI value; a human co-author passes only when the token is opted in.
- **Opt-in ticket references with an allow-vs-require split (ADR-0020).**
  `git.commit_ticket_keys` (default empty) *allows* ticket trailers (e.g. `Refs`,
  `Closes`); `git.commit_ticket_required` (default `off`; `warn`/`block`) makes a
  matching ticket *required*; `git.commit_ticket_pattern` (e.g. `^PROJ-\d+$`)
  constrains the value — a present-but-malformed reference blocks even when
  optional. Merge/revert/fixup commits are exempt; the git hook and `codeflow ci`
  enforce it identically.

- **Two working principles in both the minimal and full agent contracts.**
  *"Think independently — not a yes-man"*: a request, opinion, claim, or proposed
  approach — the operator's included — is owed analysis and evidence, not
  agreement; the operator still makes the final call, but agreement without
  examination is a failure mode, not deference. *"Think in depth, not at the
  surface"*: chase the implication chain ("and therefore? …") to the fundamental
  that decides the matter, and trace how each order ripples across the related
  domains, not just the immediate area.

## [2.1.0] - 2026-07-12

### Added

- Cross-harness skills: the `cf-*` skills install to `.agents/skills/` (read by
  Codex) alongside `.claude/skills/`, so one skill set serves multiple coding
  CLIs. Claude Code merged custom commands into skills (v2.1.101); codeflow now
  ships skills only.
- The commit-msg gate flags a mis-cased `BREAKING CHANGE:` / `BREAKING-CHANGE:`
  footer, so a breaking change is never silently downgraded to a minor bump.

### Fixed

- `codeflow update` no longer silently discards merged-in user edits to a
  managed file on the *next* update. After a clean 3-way merge the manifest now
  records the pristine shipped hash (restoring the invariant `recorded ==
  hash(baseline)`), so a merged file stays classified user-modified and is
  re-merged rather than overwritten with the shipped version.
- `codeflow update` reconciles orphaned managed files: an artifact removed or
  renamed upstream (e.g. a command that became a skill), and its baseline and
  manifest record, is pruned when unmodified instead of lingering and colliding
  with its renamed replacement. User-modified and user-owned files are never
  deleted (ADR-0011).

### Changed

- Shipped scaffold content and documentation corrected for the commands→skills
  rename and for install/enforcement accuracy ahead of the public release.

## [2.0.0] - 2026-07-03

2.0.0 is a complete Rust rewrite of codeflow. The 1.x line (a shell/Node tooling
set) shares no code with it and is preserved at the `v1-final` tag.

### Added

- Single `codeflow` binary (crates `codeflow-core` + `codeflow-cli`) with an
  embedded scaffold, installed into any repo via `codeflow init`.
- Policy-driven git enforcement across four planes — git client hooks, an
  in-session PreToolUse guard, CI, and remote branch protection — all reading a
  single `.codeflow/policy.json`.
- Scaffolding with three ownership classes (managed, managed-region,
  user-owned), 3-way-merge `codeflow update`, and version-skew detection.
- Knowledge model: capabilities registry, ADRs, epics/specs, an automatic
  ledger, and `codeflow recall` full-text search.
- Composable pipeline workflow (build → independent review → verify) and
  cross-vendor delegation (`cf-delegate` / `cf-consult`).
- CLI surface: `init`, `update`, `test`, `validate [--docs]`, `status`,
  `recall`, `orient`, `doctor`, `integrate`, `remote`.
- cargo-dist release pipeline with prebuilt binaries for macOS (arm64/x64) and
  Linux (x64) and a shell installer.

[Unreleased]: https://github.com/sathyassn/codeflow/compare/v2.1.0...HEAD
[2.1.0]: https://github.com/sathyassn/codeflow/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/sathyassn/codeflow/releases/tag/v2.0.0
