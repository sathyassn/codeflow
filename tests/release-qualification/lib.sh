#!/bin/sh
# Shared helpers for the release-qualification harness.
#
# Sourced by qualify.sh. Everything here writes to the run's disposable work
# directory; nothing touches the repository under qualification.

# ---------------------------------------------------------------------------
# Result recording
# ---------------------------------------------------------------------------
#
# One row per check. Exactly one of passed / failed / unavailable, always with
# an expected and an observed value. `unavailable` additionally names the owner
# the gate belongs to, so a gap can never read as a pass.

RESULT_PASSED=passed
RESULT_FAILED=failed
RESULT_UNAVAILABLE=unavailable

# record <sample> <tier> <command> <check> <status> <expected> <observed> [owner]
record() {
  _sample=$1
  _tier=$2
  _command=$3
  _check=$4
  _status=$5
  _expected=$6
  _observed=$7
  _owner=${8:-}

  case $_status in
    "$RESULT_PASSED" | "$RESULT_FAILED" | "$RESULT_UNAVAILABLE") ;;
    *)
      printf 'harness error: bad status "%s" for %s/%s\n' "$_status" "$_command" "$_check" >&2
      exit 70
      ;;
  esac
  if [ "$_status" = "$RESULT_UNAVAILABLE" ] && [ -z "$_owner" ]; then
    printf 'harness error: unavailable row without an owner: %s/%s\n' "$_command" "$_check" >&2
    exit 70
  fi
  if [ -z "$_expected" ] || [ -z "$_observed" ]; then
    printf 'harness error: blank expected/observed: %s/%s\n' "$_command" "$_check" >&2
    exit 70
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$_sample" "$_tier" "$_command" "$_check" "$_status" \
    "$(oneline "$_expected")" "$(oneline "$_observed")" "$_owner" >>"$RESULTS"

  if [ "$_status" = "$RESULT_FAILED" ]; then
    FAILURES=$((FAILURES + 1))
  fi
  printf '  %-12s %s / %s\n' "$_status" "$_command" "$_check" >&2
}

# Collapse a value to a single table-safe line.
#
# Em and en dashes become a plain hyphen: the project's writing rules forbid
# both characters in tracked files, and some command output contains them, so
# the recorded cell is normalized rather than the rule being broken. Nothing
# else about the observed text changes.
oneline() {
  printf '%s' "$1" |
    tr '\n\t|' '  /' |
    sed $'s/\u2014/-/g; s/\u2013/-/g' |
    cut -c1-400
}

# ---------------------------------------------------------------------------
# Command execution
# ---------------------------------------------------------------------------
#
# cf() runs the binary under qualification, capturing stdout+stderr in CF_OUT
# and the exit status in CF_STATUS. It never aborts the run: a non-zero status
# is data, not an error, because half the matrix is about rejections.

cf() {
  CF_OUT=$("$BINARY" "$@" 2>&1 </dev/null) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ codeflow %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# cf_stdin <payload> <args...> - same, with a stdin payload (the Claude hooks).
cf_stdin() {
  _payload=$1
  shift
  CF_OUT=$(printf '%s' "$_payload" | "$BINARY" "$@" 2>&1) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ echo <payload> | codeflow %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# sh_run <command string> - an ordinary shell command, same capture contract.
sh_run() {
  CF_OUT=$(eval "$1" 2>&1) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ %s\n[exit %s]\n%s\n' "$1" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# ---------------------------------------------------------------------------
# Assertions
#
# Each returns the status string rather than branching at every call site.
# ---------------------------------------------------------------------------

# status_for <expected-exit> - passed when CF_STATUS matches.
status_for() {
  if [ "$CF_STATUS" = "$1" ]; then printf '%s' "$RESULT_PASSED"; else printf '%s' "$RESULT_FAILED"; fi
}

# status_for_match <expected-exit> <substring> - exit AND message must match.
status_for_match() {
  if [ "$CF_STATUS" = "$1" ] && printf '%s' "$CF_OUT" | grep -qF -- "$2"; then
    printf '%s' "$RESULT_PASSED"
  else
    printf '%s' "$RESULT_FAILED"
  fi
}

# observed_exit [extra] - the observed column for an exit-status check.
observed_exit() {
  if [ -n "${1:-}" ]; then
    printf 'exit %s: %s' "$CF_STATUS" "$1"
  else
    printf 'exit %s: %s' "$CF_STATUS" "$(printf '%s' "$CF_OUT" | grep -v '^[[:space:]]*$' | head -2 | tr '\n' ' ')"
  fi
}

# ---------------------------------------------------------------------------
# The workspace-trust prompt
# ---------------------------------------------------------------------------
#
# A freshly scaffolded sample carries its own .claude/settings.json, so the
# Claude session the canary starts asks a human to trust the folder before it
# takes any prompt. No agent may answer that question, so the harness waits for
# the operator instead of recording the whole delegate lane unavailable.

# The line Claude Code paints while it waits for that answer.
TRUST_PROMPT_MATCH='Is this a project you created or one you trust'

# How often the pane is re-read while waiting.
TRUST_POLL_SECONDS=5

# trust_prompt_showing <pane-id> - true while the pane still asks the question.
trust_prompt_showing() {
  herdr pane read "$1" --source recent --lines 120 2>/dev/null |
    grep -q "$TRUST_PROMPT_MATCH"
}

# wait_for_trust_answer <pane-id> <seconds> - poll until the prompt is gone.
#
# Returns 0 as soon as the pane no longer shows it, and 1 when the budget runs
# out with the question still on screen. The last sleep is shortened to the
# remaining budget so the wait never overruns the number the operator asked for.
wait_for_trust_answer() {
  _pane=$1
  _budget=$2
  _waited=0
  while trust_prompt_showing "$_pane"; do
    [ "$_waited" -lt "$_budget" ] || return 1
    _step=$TRUST_POLL_SECONDS
    _left=$((_budget - _waited))
    [ "$_step" -le "$_left" ] || _step=$_left
    sleep "$_step"
    _waited=$((_waited + _step))
  done
  return 0
}

# tree_digest <dir> - one digest over the content of every file in the sample,
# excluding .git. Porcelain status cannot see a change inside a file that was
# already dirty, so idempotence is judged on content instead.
tree_digest() {
  (
    cd "$1" || exit 1
    find . -name .git -prune -o -type f -print |
      LC_ALL=C sort |
      while IFS= read -r _f; do
        printf '%s  %s\n' "$(shasum -a 256 "$_f" | awk '{print $1}')" "$_f"
      done |
      shasum -a 256 |
      awk '{print $1}'
  )
}

# ---------------------------------------------------------------------------
# Sample construction
# ---------------------------------------------------------------------------

# seed_git <dir> - a repository with an identity and no codeflow state.
seed_git() {
  git init -q -b main "$1"
  git -C "$1" config user.email qualification@example.invalid
  git -C "$1" config user.name "Qualification Harness"
  git -C "$1" config commit.gpgsign false
}

# commit_in <dir> <message> - commit everything, bypassing nothing.
commit_in() {
  git -C "$1" add -A
  git -C "$1" commit -q -m "$2"
}

# build_greenfield_rust <dir>
build_greenfield_rust() {
  _d=$1
  mkdir -p "$_d/src"
  seed_git "$_d"
  cat >"$_d/Cargo.toml" <<'TOML'
[package]
name = "qualification-sample"
version = "0.1.0"
edition = "2021"

[dependencies]
TOML
  cat >"$_d/src/lib.rs" <<'RUST'
//! A minimal library so the sample has a real test target.

/// Add two numbers.
#[must_use]
pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds_two_numbers() {
        assert_eq!(add(2, 2), 4);
    }
}
RUST
  commit_in "$_d" "chore: seed the rust sample"
  printf 'target/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: ignore the build directory"
}

# build_greenfield_node <dir>
build_greenfield_node() {
  _d=$1
  mkdir -p "$_d/src"
  seed_git "$_d"
  cat >"$_d/package.json" <<'JSON'
{
  "name": "qualification-sample",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "node run-tests.mjs"
  }
}
JSON
  # A runner rather than a bare `node --test` line: the shipped single-target
  # template calls `npm test -- --ci`, and the built-in runner rejects an
  # argument it does not know.
  cat >"$_d/run-tests.mjs" <<'JS'
import process from "node:process";
import { run } from "node:test";
import { spec } from "node:test/reporters";

let failed = false;
run({ files: ["src/add.test.js"] })
  .on("test:fail", () => {
    failed = true;
  })
  .compose(new spec())
  .pipe(process.stdout)
  .on("finish", () => {
    process.exitCode = failed ? 1 : 0;
  });
JS
  cat >"$_d/src/add.js" <<'JS'
export function add(left, right) {
  return left + right;
}
JS
  cat >"$_d/src/add.test.js" <<'JS'
import assert from "node:assert/strict";
import { test } from "node:test";
import { add } from "./add.js";

test("adds two numbers", () => {
  assert.equal(add(2, 2), 4);
});
JS
  commit_in "$_d" "chore: seed the node sample"
  printf 'node_modules/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: ignore installed dependencies"
}

# build_brownfield <dir> - a project that already has a README, a CI workflow,
# its own git hooks, and an owner instruction in an AGENTS.md that codeflow
# also manages. Everything here must survive init and update.
build_brownfield() {
  _d=$1
  mkdir -p "$_d/src" "$_d/.github/workflows"
  seed_git "$_d"
  cat >"$_d/README.md" <<'MD'
# Brownfield qualification sample

This project existed before codeflow. Its README, its CI workflow, its git
hooks and the owner instruction in its agent contract are the files the
qualification proves are preserved.

## Build

Run `cargo test`.
MD
  cat >"$_d/.github/workflows/ci.yml" <<'YAML'
name: project ci
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --locked
YAML
  cat >"$_d/AGENTS.md" <<'MD'
# Brownfield agent contract

Owner instruction: never edit anything under `vendor/`.
MD
  cat >"$_d/Cargo.toml" <<'TOML'
[package]
name = "brownfield-sample"
version = "0.2.0"
edition = "2021"

[dependencies]
TOML
  cat >"$_d/src/lib.rs" <<'RUST'
//! Pre-existing library code.

/// Multiply two numbers.
#[must_use]
pub fn multiply(left: i32, right: i32) -> i32 {
    left * right
}

#[cfg(test)]
mod tests {
    use super::multiply;

    #[test]
    fn multiplies_two_numbers() {
        assert_eq!(multiply(3, 4), 12);
    }
}
RUST
  printf 'target/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: seed the brownfield sample"

  # The project's own client hooks, installed the ordinary way. init rewires
  # core.hooksPath, so these files must still be on disk afterwards.
  mkdir -p "$_d/.git/hooks"
  cat >"$_d/.git/hooks/pre-commit" <<'SH'
#!/bin/sh
echo "brownfield project pre-commit hook"
SH
  chmod 0755 "$_d/.git/hooks/pre-commit"
  cat >"$_d/.git/hooks/commit-msg" <<'SH'
#!/bin/sh
echo "brownfield project commit-msg hook"
SH
  chmod 0755 "$_d/.git/hooks/commit-msg"

  cat >"$_d/docs-note.md" <<'MD'
Release note: the 0.2.0 line is maintained from `main`.
MD
  commit_in "$_d" "docs: record the maintained release line"
}

# snapshot_files <dir> <out> <paths...> - SHA-256 of each path, for the
# before/after preservation diff.
snapshot_files() {
  _d=$1
  _out=$2
  shift 2
  : >"$_out"
  for _p in "$@"; do
    if [ -f "$_d/$_p" ]; then
      printf '%s  %s\n' "$(shasum -a 256 "$_d/$_p" | awk '{print $1}')" "$_p" >>"$_out"
    else
      printf '%s  %s\n' "ABSENT" "$_p" >>"$_out"
    fi
  done
}
