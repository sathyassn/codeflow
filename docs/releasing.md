# Releasing codeflow

How a codeflow release is cut, and how a project that *consumes* codeflow should
think about its own versioning. Decision record: ADR-0012 (supersedes ADR-0010).

## codeflow's own releases

Version source of truth: `Cargo.toml [workspace.package] version` (both crates
inherit it via `version.workspace = true`). Releases are conventional-commit
driven and human-gated. Two tools do the work:

- **git-cliff** derives the next SemVer **and** the changelog from the
  Conventional Commits the commit-msg gate already enforces — reading git history
  only, so it never publishes or runs `cargo package`.
- **cargo-dist** builds the three target binaries + the shell installer and
  publishes the GitHub Release, triggered by the version tag.

### The runbook

Prerequisite: install git-cliff once (`cargo install git-cliff`, or a prebuilt
binary from <https://github.com/orhun/git-cliff/releases>).

```sh
# 1. On a release branch, off the latest main:
git switch -c chore/release main

# 2. Compute the next version from the conventional commits since the last tag:
NEXT=$(git cliff --bumped-version)        # e.g. v2.2.0
echo "$NEXT"

# 3. Bump the single workspace version (both crates inherit it):
#    edit Cargo.toml -> [workspace.package] version = "<NEXT without the leading v>"
#    then refresh the lockfile:
cargo build

# 4. Prepend the new version's section to CHANGELOG.md — this keeps the curated
#    past entries intact (unlike `-o`, which regenerates the whole file). Then
#    review/refine the draft; git-cliff writes from commit subjects, so tighten
#    the wording:
git cliff --unreleased --tag "$NEXT" --prepend CHANGELOG.md

# 5. Commit and open a PR:
git commit -am "chore(release): $NEXT"
git push -u origin chore/release   # then open the PR
```

- A **human merges** the release PR on green CI (ADR-0006/0007) — agents never
  merge to `main`.
- **Before tagging, re-verify the harness-parity claims** against the
  currently installed harness versions — these surfaces move fast, and
  ADR-0008/ADR-0013/ADR-0014 and docs/adoption.md's cross-harness section pin
  a version that decays:
  - The PreToolUse payload contract (`git-guard`/`exec-guard`) still matches
    what Claude Code and an interactive Codex session send.
  - The Codex `hooks.json` events still fire as documented, and the `cf-guard`
    permission-profile keys in `.codex/config.toml` still validate — run
    `codex --strict-config doctor` from a checkout with the shipped
    `.codex/config.toml` in place; `--strict-config` errors out on any field
    the installed Codex no longer recognizes.
  - The Claude settings/hook schema (`.claude/settings.json`) still matches
    what the installed Claude Code expects.
  - The host-neutral duo contract test passes, and both native interactive
    lanes complete a scoped canary with the task's required MCP tools:
    Claude Code → Codex through the enabled official plugin, and Codex →
    Claude through task-scoped tmux with Stop/StopFailure hook completion.
    Record versions, exact commands, and observed tool access. Do not accept
    auth status output in place of a working interactive session.
  - `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` passes
    locally; CI billing/availability never substitutes for this evidence.

  Record new verification in a current ADR/release note and update
  docs/adoption.md if anything drifted; historical ADR bodies remain append-only.
- After merge, tag the release; the tag drives cargo-dist:

  ```sh
  git tag "$NEXT" && git push origin "$NEXT"
  ```

  A real tag push triggers `.github/workflows/release.yml` — **no PAT or bot
  needed**.

### Why not release-plz / a bot?

release-plz was evaluated and removed (ADR-0012): it is publish-centric and
cannot `cargo package` this unpublished binary workspace. If full PR-bot
automation (a bot opens the release PR for you) is wanted later, **release-please**
fits — it has no crates.io coupling, unlike release-plz — but it needs a Personal
Access Token so its tag triggers cargo-dist. Until then, the runbook above is the
reliable, verifiable path.

## Versioning in a project that consumes codeflow

codeflow gives your repo the *substrate* for clean releases — the commit-msg gate
enforces Conventional Commits, so your history is SemVer-derivable — but it does
**not** scaffold a release pipeline. Release/version/changelog tooling is
stack-specific and stays yours to choose. Because your commits are already
conventional, any of these has clean input:

| Situation | Reasonable choice |
|---|---|
| Rust crate, published to crates.io | `release-plz` (the standard) or `cargo-release` |
| Rust binaries, not published | `git-cliff` + `cargo-dist` — what codeflow itself uses |
| Any language, PR-based automation | `release-please` |
| Changelog only | `git-cliff` or `conventional-changelog` |

codeflow's job is the discipline; the release mechanism is yours. This split is
deliberate — release tooling is as stack-specific as a test runner, so codeflow
records standards and enforces commit hygiene rather than prescribing one
release tool for every consumer.
