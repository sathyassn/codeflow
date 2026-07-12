---
id: ADR-0020
title: restore the original commit standard — 50/72 subject, bullet-only body, block-enforced
date: 2026-07-12
status: accepted
superseded_by: null
architecture_impact: The four enforcement planes are unchanged in shape; this adds two subject-length checks, a body-shape check, and a warn-only contract-surface tripwire to the existing commit-msg / `codeflow ci` standards module, plus six default-armed policy keys. A later amendment (same decision) keeps the strict default body shape but adds per-project opt-in escape hatches — allowed footer trailers, required footers, and an allow-vs-require ticket reference — for five more empty/off-by-default policy keys (`commit_footer_tokens`, `commit_required_footers`, `commit_ticket_keys`, `commit_ticket_required`, `commit_ticket_pattern`). docs/architecture.md's enumeration of the CI checks is updated to name them. No engine module, ownership class, or boundary moves.
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0020 — restore the original commit standard

## Context

The v1 CodeFlow commit standard (`CONTRIBUTING.md`, introduced at PR #328)
specified a bounded shape, verbatim:

> - **Subject:** `type: description` (max 50 chars)
> - **Body:** `- Optional bullet points` (max 3), each bullet max 72 chars

The v2 rebuild carried the conventional-format check, the type whitelist, the
no-trailing-period rule, the attribution and emoji rules — but silently dropped
the two length limits and the body-shape rule. Nothing enforced a 50-char
description, a 72-char subject line, or a bullet-only body, so commit messages
drifted: over-long subjects that wrap in every git tool, and multi-paragraph
"story" bodies that narrate rather than record. The dilution was invisible
because the rule was never declared anywhere the checker could read it. The
operator ordered the original standard restored and mechanically enforced, so
the drift cannot recur.

## Decision

The v1 standard is restored, block-enforced at every tier, and adapted for
scopes:

- **Subject:** `type(scope): description` — scope optional; description ≤ 50
  chars (measured after `type(scope): `); the whole subject line ≤ 72 chars.
  The existing conventional-format, type-whitelist, no-trailing-period, and
  breaking-footer rules stand unchanged.
- **Body:** only `- ` bullets — at most 3, each a single line ≤ 72 chars —
  interleaved with blank lines, optionally followed by a Conventional-Commits
  `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer block. A prose paragraph, a
  numbered list, or a story is a violation naming the offending line.
- **Merge commits (>1 parent) are exempt as a class**, alongside the existing
  auto-generated exemptions (`Merge `, `Revert `, `Reapply `, `fixup!`,
  `squash!`); the `codeflow ci` range enumeration already excludes merges with
  `--no-merges`.

**Scope is optional by design.** Conventional Commits treats a scope as a *MAY*,
not a *MUST*; forcing one on every commit would be noise on repo-wide changes.
Where a change belongs to an area, that area maps naturally to a scope, and
git-cliff groups the changelog by type first, so a missing scope never breaks
the derived CHANGELOG or the version bump.

**Enforcement rides the floor (ADR-0019).** The rules live in the one shared
`standards` module the commit-msg git hook, the `git-guard`, and `codeflow ci`
all call, so the four planes cannot drift from each other (ADR-0017), and the
armed policy ships at every tier — the checks are block-level from `--minimal`
up. Five new `git` policy keys carry the limits, all serde-defaulted so an older
`policy.json` gains them without a manual migration: `commit_desc_max_len` (50)
and `commit_subject_max_len` (72), governed by `commit_format`; and `commit_body`
(`block`) governing the body-shape rule with `commit_body_max_bullets` (3) and
`commit_body_bullet_max_len` (72).

**Rebase-only landings preserve the per-commit contract.** This repo's merge
settings are rebase-only — merge commits and squash are disabled, branch
auto-delete is off (`gh repo view`: `mergeCommitAllowed=false`,
`squashMergeAllowed=false`, `rebaseMergeAllowed=true`,
`deleteBranchOnMerge=false`). Every commit lands on `main` exactly as authored,
so each one passing the standard is meaningful — the gate is not papered over by
a squash at merge time.

**Breaking changes: judgment detects, a glob nudges, the footer validates and
versions.** Whether a change breaks a consumer is *semantic* — it depends on what
downstream code relies on, which no diff can know — so detection cannot be fully
mechanical, and the primary control is a judgment call the contract asks for on
every commit: does this change an API, a CLI flag, a config schema, a file
format, a default, or managed-file semantics? If so, the author marks the subject
`type!:` and writes a `BREAKING CHANGE:` footer with the migration path; that
footer is the machine-readable signal git-cliff turns into a MAJOR bump (the
mechanics that *validate* the marker's casing and *version* from it already
exist — see [`check_breaking_footer`]). Because judgment can lapse, a sixth `git`
policy key `breaking_watch_paths` (path globs, serde default empty) adds a
mechanical *tripwire*: when a commit stages a file matching a declared contract
surface with no breaking marker, the commit-msg check — and `codeflow ci`, which
reuses it via `commit_msg_with_files` — emits a **WARN, never a block**. It must
not block: a touched surface is not proof of a break (most edits to it stay
backward-compatible), so the glob can only prompt a confirm, not veto. This repo
declares its own surfaces (`policy.rs`, `scaffold-manifest.toml`, the shipped
`policy.json`, `main.rs`); consumers declare theirs. The tooling supports the
judgment; it does not replace it.

## Consequences

- **This branch's history is rewritten to the restored standard
  (operator-ordered).** The `feat/tier-enforcement-floor` branch was unmerged;
  its commits were rebuilt as an atomic sequence, every message conforming to the
  new shape, and force-pushed with `--force-with-lease`. A history rewrite is
  normally out of bounds, but the branch had not landed and the operator directed
  it explicitly.
- **Existing consumers gain the gates on their next `codeflow update`.** The five
  keys arrive with their shipped defaults via the standard policy-key
  reconciliation (values already set are never mutated), and the enforcement
  machinery is already installed at every tier by ADR-0019 — so restoring the
  standard needs no new install path, only the newer binary.
- **A prose-heavy commit body is now a blocked mistake, not a style choice.** The
  "why" that used to live in a paragraph goes in the bullets, in the PR body, or
  in an ADR — the durable homes — not in a story the changelog cannot use.
- **The limits are policy, not code.** A repo that genuinely wants a different
  budget edits `.codeflow/policy.json`; softening the commit rules is
  discouraged, because git-cliff derives the version bump and CHANGELOG from the
  commit history.
- **The breaking-change signal is mechanized where it can be, and left to
  judgment where it cannot.** The footer drives versioning; the contract asks the
  judgment on every commit; `breaking_watch_paths` warns when a declared surface
  is touched unmarked. Because the tripwire is advisory, a genuinely
  non-breaking edit to a watched file is confirmed and proceeds without friction —
  the cost of a false nudge is one moment of thought, never a blocked commit.

## Architecture impact

The four enforcement planes are unchanged in shape. The change adds two
subject-length checks, a body-shape check, and a warn-only contract-surface
tripwire inside the existing `standards` module that the commit-msg hook and
`codeflow ci` share (the latter now feeds per-commit changed files through
`commit_msg_with_files`), plus six default-armed `git` policy keys.
`docs/architecture.md`'s enumeration of the CI checks is updated to name the
subject-length budget and body-shape rule. No engine module, ownership class, or
boundary moves.

## Amendment — opt-in footer trailers, ticket references, and required footers

The first cut of the body-shape rule accepted only `- ` bullets and a
`BREAKING CHANGE:` footer. That is the right *default*, but it left no escape
hatch: a project with a genuine need — a ticket reference on every commit, a DCO
`Signed-off-by` — had no sanctioned way to express it, and a stray trailer just
blocked. This amendment (one decision with the restore above, appended before the
ADR merged) adds that escape hatch **without loosening the default**. The
tempting move — ship a populated whitelist of "common" trailers — was rejected:
in an agent-driven, no-attribution project every default-allowed trailer is a
noise slot an agent will fill, so the baseline must stay bullets +
`BREAKING CHANGE:` only, and every relaxation is a deliberate per-project opt-in.

**The strict default is unchanged.** The body is `- ` bullets (≤3, ≤72 each) then
an optional footer that, out of the box, is only the always-allowed
`BREAKING CHANGE:` / `BREAKING-CHANGE:` footer (which keeps its wrapping
multi-line description). Any other trailer-shaped line blocks. The no-stories
guarantee is preserved by two constraints that hold regardless of opt-ins: a
`<Token>` is a single word (letters/digits/hyphens), so a prose line like
`This change: …` is not a trailer and still blocks; and a trailer value is a
single line that cannot span into prose.

**Allowed and required are independent, configurable dimensions.** Five
serde-defaulted `git` keys, all empty/off by default:

- `commit_footer_tokens: []` — extra trailer tokens *allowed* (optional) in the
  footer beyond `BREAKING CHANGE:` (e.g. a project adds `Signed-off-by`). Matched
  exactly, case-sensitively. A trailer whose token is not allowed (`Note: …`)
  blocks, naming the line.
- `commit_required_footers: []` — trailer tokens that *must* appear on every
  non-exempt commit (e.g. `["Signed-off-by"]` for DCO). A required token is
  implicitly allowed; a commit missing one blocks under `commit_body`.
- `commit_ticket_keys: []` — ticket tokens the project recognizes (e.g. `["Refs",
  "Closes"]`). Non-empty *allows* those ticket trailers; empty (default) means a
  `Refs:` line is not auto-allowed and blocks on shape.
- `commit_ticket_required: off` — whether a *matching* ticket trailer is
  required. `off` = allowed-but-optional (present passes, absent is fine);
  `warn`/`block` = at least one required, at that level. Only meaningful when
  `commit_ticket_keys` is non-empty.
- `commit_ticket_pattern: ""` — a regex a ticket value must match (e.g.
  `^PROJ-\d+$`). A present ticket whose value fails it is a malformed reference
  and blocks even when optional; a required ticket must match. An unparseable
  pattern degrades to no format check (fail-open).

Merge/revert/fixup/squash commits are exempt from the ticket and required-footer
checks, as with the rest of the standard.

**AI-attribution interaction is preserved.** Even when a project opts
`Co-authored-by` into `commit_footer_tokens`, the separate `ai_attribution` check
still runs over the whole message and still blocks an AI co-author (`Claude`,
`GPT`, `noreply@anthropic`, …). A *human* co-author passes only when the project
opted the token in; an *AI* one passes the shape check but fails attribution. The
two checks are orthogonal by design: shape validates structure, attribution
validates identity — and by default `Co-authored-by` is not allowed at all, so it
blocks on shape before attribution even matters.

All five keys are serde-defaulted, so an older `policy.json` gains them (empty/off
— no behavior change) on the next `codeflow update`; the checks live in the same
shared `standards` module, so the git hook and `codeflow ci` inherit them
identically (ADR-0017), and they ride the enforcement floor (ADR-0019) — the
strict body shape is block-level from `--minimal` up, and every relaxation is a
per-project opt-in on top. **This repo keeps all five at their strict defaults**
— it references CAPs and ADRs inside bullets, not as trailers, and forbids AI
attribution outright.
