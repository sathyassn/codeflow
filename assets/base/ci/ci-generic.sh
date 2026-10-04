#!/usr/bin/env sh
# codeflow CI: a PORTABLE snippet (CodeFlow ADR-0017). The checks live in the
# `codeflow` binary (single source of truth, no drift from the git hooks or the
# Claude git-guard). Run it from any CI, a Makefile target or a script that
# has a working tree of the change and its full history.
#
# It runs the same three gates the platform templates do:
#   codeflow ci              verify the commit range and branch name against policy
#   codeflow test            the test gate
#   codeflow validate --docs the doc-graph integrity lint
#
# The binary is the version the TARGET pins in .codeflow/project.toml
# (`scaffold_version`), downloaded from its release and verified against the
# archive digests pinned beside it ([scaffold_sha256], written by `codeflow
# update --pin`) and the release's published sha256.sum; a stale or partial
# digest table, or a missing or wrong checksum, fails the run and nothing
# unverified is installed (SPC-013 R-113). Without a pinned table the run
# checks sha256.sum alone and warns. The target is the current
# commit of the branch the change lands on, not the merge base; this script
# refuses to run without it and never guesses it, since a pin read from the
# change itself would let the change choose the binary that judges it. An upgrade takes two
# changes, in order: first raise only `scaffold_version` and its digests
# (`codeflow update --pin <version>`; the target's binary judges it and the
# candidate is tested alongside), then run `codeflow update`. A project that
# needs its own toolchain commits .codeflow/ci-setup.sh, sourced just before
# `codeflow test`.
#
# PR/MR body: export CODEFLOW_PR_BODY to also scan it (AI attribution, emoji,
# and the required-section structure, git.pr_sections).
#
# Usage:
#   ci-generic.sh <target> [<head>]        # target commit, head (default HEAD)
#   BASE=<target> HEAD=<head> ci-generic.sh
#
# Needs git, curl, tar with xz, awk and sha256sum or shasum. Releases carry
# Linux x86_64 and macOS binaries.

BASE="${1:-${BASE:-}}"
HEAD="${2:-${HEAD:-HEAD}}"

# >>> codeflow pinned run (SPC-013 R-113). The same text runs in ci-generic.sh,
# .gitlab-ci.yml and bitbucket-pipelines.yml: BASE names the target commit and
# HEAD the change, and the target's pin chooses the binary that judges it.
set -eu
codeflow_fail() { echo "codeflow: error: $*" >&2; exit 1; }
[ -n "${BASE:-}" ] || codeflow_fail "no target commit: pass the commit this change lands on (the pull request's base); the pinned install never guesses it"
target=$(git rev-parse --verify -q "${BASE}^{commit}") || codeflow_fail "target commit ${BASE} is not in this clone's history"
head=$(git rev-parse --verify -q "${HEAD:-HEAD}^{commit}") || codeflow_fail "head commit ${HEAD:-HEAD} is not in this clone's history"
case "$(uname -s) $(uname -m)" in
  "Linux x86_64") triple=x86_64-unknown-linux-gnu ;;
  "Darwin arm64") triple=aarch64-apple-darwin ;;
  "Darwin x86_64") triple=x86_64-apple-darwin ;;
  *) codeflow_fail "codeflow publishes no release binary for $(uname -s) $(uname -m)" ;;
esac
# The scaffold_version a commit pins, or nothing.
codeflow_pin() {
  git show "$1:.codeflow/project.toml" 2>/dev/null |
    sed -n 's/^scaffold_version[[:space:]]*=[[:space:]]*"\([0-9A-Za-z.+-]*\)".*/\1/p' | head -n 1
}
# Succeeds when version $1 is older than $2: numeric core first, and a
# pre-release before its release, as codeflow orders versions.
codeflow_older() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    i = index(a, "-"); ca = i ? substr(a, 1, i - 1) : a; pa = i ? substr(a, i + 1) : ""
    j = index(b, "-"); cb = j ? substr(b, 1, j - 1) : b; pb = j ? substr(b, j + 1) : ""
    n = split(ca, x, "."); m = split(cb, y, ".")
    for (k = 1; k <= (n > m ? n : m); k++) {
      if (x[k] + 0 < y[k] + 0) exit 0
      if (x[k] + 0 > y[k] + 0) exit 1
    }
    if (pa != "" && pb == "") exit 0
    if (pa == "" || pb == "") exit 1
    exit (pa < pb) ? 0 : 1
  }'
}
# The [scaffold_sha256] table a commit pins, as "<version> <digest>" for
# this triple with "-" for a missing value, or nothing without a table.
codeflow_digest() {
  git show "$1:.codeflow/project.toml" 2>/dev/null | LC_ALL=C tr '\000' '\001' | LC_ALL=C awk -v want="$triple" -v sq="'" '
    function fail(why) { if (bad == "") bad = why }
    /\r[^\n]/ { fail("may be hidden by a carriage return inside a line, which the CI installers do not read as a line break"); next }
    /[\001-\010\013\014\016-\037\177]/ { fail("may be hidden by a control character, which the CI installers do not read"); next }
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*[^[:space:] -~]/ { fail("may be hidden by a line that starts with a character the CI installers do not read"); next }
    index($0, "\"\"\"") || index($0, sq sq sq) { fail("may be hidden by a multi-line string, which the CI installers do not read") }
    /^[[:space:]]*\[/ {
      inside = ($0 ~ /^[[:space:]]*\[[[:space:]]*scaffold_sha256[[:space:]]*\][[:space:]]*(#.*)?$/)
      if (inside && table) fail("is declared twice")
      name = $0; sub(/\].*/, "", name)
      if (!inside && name ~ /scaffold_sha256/) fail("is written in a form the CI installers do not read (a quoted header, an inline or dotted table, or a sub-table)")
      if (!inside && name ~ /\\/) fail("may be hidden behind an escaped key, which the CI installers do not read")
      if (!inside && name ~ /[^ -~\t]/) fail("may be hidden behind a name with a character outside printable ASCII, which the CI installers do not read")
      if (inside) table = 1
      next
    }
    /=/ {
      name = $0; sub(/=.*/, "", name)
      if (name ~ /scaffold_sha256/) fail("is written in a form the CI installers do not read (a quoted header, an inline or dotted table, or a sub-table)")
      if (name ~ /\\/) fail("may be hidden behind an escaped key, which the CI installers do not read")
      if (name ~ /[^ -~\t]/) fail("may be hidden behind a name with a character outside printable ASCII, which the CI installers do not read")
    }
    inside && /[^[:space:]]/ {
      if ($0 !~ /^[[:space:]]*"?[0-9A-Za-z_-]+"?[[:space:]]*=[[:space:]]*"[^"\\]*"[[:space:]]*(#.*)?$/) {
        fail("holds a line other than key = \"value\", which the CI installers do not read"); next
      }
      key = $0; sub(/^[[:space:]]*"?/, "", key); sub(/"?[[:space:]]*=.*/, "", key)
      if (key in seen) fail("lists " key " twice")
      seen[key] = 1
      value = $0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/".*/, "", value)
      if (key == "version") version = value
      if (key == want) digest = value
    }
    END { if (bad != "") print "!", bad; else if (table) print (version == "" ? "-" : version), (digest == "" ? "-" : digest) }'
}
# Install release $1, pinned at commit $3, into directory $2. When $3 pins
# the release's archive digests ([scaffold_sha256], written by `codeflow
# update --pin`), the archive must match the digest reviewed there for $1;
# a table for another version, a missing entry or a mismatch fails closed.
# The release's sha256.sum is checked too, and is the only check while no
# digest is pinned: a missing checksum file, a missing entry or a mismatch
# fails closed.
codeflow_install() {
  url="${CODEFLOW_RELEASE_URL:-https://github.com/sathyassn/codeflow/releases/download}/v$1"
  asset="codeflow-cli-${triple}.tar.xz"
  pinned=$(codeflow_digest "$3")
  digest=
  case "$pinned" in
    "!"*) codeflow_fail "the [scaffold_sha256] table in .codeflow/project.toml at $3 ${pinned#! }, so nothing is installed; write it as the plain table \`codeflow update --pin $1\` writes" ;;
  esac
  if [ -n "$pinned" ]; then
    [ "${pinned%% *}" = "$1" ] ||
      codeflow_fail "the [scaffold_sha256] table in .codeflow/project.toml at $3 pins the digests of codeflow ${pinned%% *}, not $1; run \`codeflow update --pin $1\` and land it with the pin"
    digest=${pinned#* }
    printf '%s\n' "$digest" | grep -Eq '^[0-9a-f]{64}$' ||
      codeflow_fail "the [scaffold_sha256] table in .codeflow/project.toml at $3 lists no digest for ${triple}; run \`codeflow update --pin $1\` and land it with the pin"
  else
    echo "codeflow: warning: no release digest is pinned in .codeflow/project.toml at $3, so codeflow $1 is checked only against its release's own sha256.sum; run \`codeflow update --pin $1\` and land it to pin the reviewed digest" >&2
  fi
  work=$(mktemp -d)
  curl -fsSL "${url}/sha256.sum" -o "${work}/sha256.sum" ||
    codeflow_fail "codeflow $1 has no published checksum file (sha256.sum); refusing an unverified binary"
  expected=$(awk -v a="$asset" '{n=$2; sub(/^\*/, "", n)} n == a {print $1; exit}' "${work}/sha256.sum")
  [ -n "$expected" ] ||
    codeflow_fail "sha256.sum for codeflow $1 lists no checksum for ${asset}; refusing an unverified binary"
  curl -fsSL "${url}/${asset}" -o "${work}/${asset}" ||
    codeflow_fail "cannot download ${asset} for codeflow $1"
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "${work}/${asset}" | cut -d ' ' -f 1)
  else
    actual=$(shasum -a 256 "${work}/${asset}" | cut -d ' ' -f 1)
  fi
  [ -z "$digest" ] || [ "$actual" = "$digest" ] ||
    codeflow_fail "${asset} for codeflow $1 does not match the digest pinned in .codeflow/project.toml at $3 (pinned ${digest}, got ${actual}); refusing it"
  [ "$actual" = "$expected" ] ||
    codeflow_fail "checksum mismatch for ${asset} (codeflow $1): expected ${expected}, got ${actual}; refusing it"
  tar -xJf "${work}/${asset}" -C "$work"
  mkdir -p "$2"
  cp "${work}/codeflow-cli-${triple}/codeflow" "$2/codeflow"
  chmod +x "$2/codeflow"
  if [ -n "$digest" ]; then
    echo "codeflow $1 installed and verified against the digest pinned in .codeflow/project.toml and sha256.sum"
  else
    echo "codeflow $1 installed and verified against sha256.sum"
  fi
}
pin=$(codeflow_pin "$target")
[ -n "$pin" ] || codeflow_fail "no scaffold_version pinned in .codeflow/project.toml at ${target}"
head_pin=$(codeflow_pin "$head")
# A head that kept the pin it branched from lowers nothing: merging it
# keeps the target's pin, and the target's binary judges it. Every merge
# base must carry the head's pin, since a criss-cross head chooses which
# single base `git merge-base` prints; no base at all fails closed.
kept=
for base in $(git merge-base --all "$target" "$head" || true); do
  if [ "$head_pin" = "$(codeflow_pin "$base")" ]; then kept=yes; else kept=no; break; fi
done
[ "$kept" != yes ] || head_pin="$pin"
bin=$(mktemp -d)
codeflow_install "$pin" "$bin" "$target"
PATH="${bin}:${PATH}"
export PATH
lowered=
if [ "$head_pin" != "$pin" ]; then
  if [ -n "$head_pin" ] && codeflow_older "$pin" "$head_pin"; then
    # Upgrade step one: test the candidate the head pins, never enforce with it.
    candidate=$(mktemp -d)
    codeflow_install "$head_pin" "$candidate" "$head"
    "${candidate}/codeflow" --version
    "${candidate}/codeflow" validate --docs
  else
    lowered="${head_pin:-no pin}"
  fi
fi
# The target's binary judges the range from a checkout of the target, so the
# target's policy applies and the head is read only as git data.
judge=$(mktemp -d)
git worktree add -q --detach "${judge}/target" "$target"
trap 'git worktree remove --force "${judge}/target" >/dev/null 2>&1 || true' EXIT
branch=$(git symbolic-ref --short -q HEAD || true)
(cd "${judge}/target" && codeflow ci --base "$target" --head "$head" ${branch:+--branch "$branch"})
# Refuse a lowered pin before the project's own code runs, so nothing the
# head brings can clear the refusal.
[ -z "$lowered" ] ||
  codeflow_fail "the head lowers scaffold_version from ${pin} to ${lowered}; the pin only rises, one pull request at a time, then \`codeflow update\` in the next"
# Project setup (optional): a change that holds .codeflow/ci-setup.sh has it
# sourced here, after the install and before the test gate, under set -eu,
# so the toolchain it installs and the variables it exports reach the gate
# and a failing command fails the run. The project owns the file; `codeflow
# update` never writes it.
if [ -f .codeflow/ci-setup.sh ]; then
  echo "codeflow: sourcing the project setup hook .codeflow/ci-setup.sh"
  codeflow_root=$(pwd)
  . ./.codeflow/ci-setup.sh
  cd "$codeflow_root"
fi
codeflow test --strict
codeflow validate --docs
# <<< codeflow pinned run

# Optional external add-ons (uncomment once the tools are on PATH):
#   gitleaks detect --source . --redact --no-banner --exit-code 1 \
#     --log-opts "$target..HEAD"   # secret scan of the change's own commits;
#     a scheduled run drops --log-opts to scan the full history, as the
#     GitHub workflow does weekly
#   osv-scanner scan -r .                                           # dep audit
