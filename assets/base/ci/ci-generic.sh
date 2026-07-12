#!/usr/bin/env sh
# codeflow CI — a PORTABLE snippet (ADR-0017). The checks live in the `codeflow`
# binary (single source of truth, no drift from the git hooks or the Claude
# git-guard). Run or source this from any CI, a git pre-receive hook, or a
# Makefile target — anywhere without a first-class codeflow template.
#
# It runs the same three gates the platform templates do:
#   codeflow ci             — verify the commit range + branch name against policy
#   codeflow test           — the test gate
#   codeflow validate --docs — the doc-graph integrity lint
#
# Range: pass base + head as $1 $2, or set BASE/HEAD in the env. When both are
# empty, `codeflow ci` auto-detects from the CI platform's variables
# (GitHub/GitLab/Bitbucket). On hosts with none of those, export
# CODEFLOW_DEFAULT_BRANCH=<branch> to name the base branch explicitly;
# otherwise the fallback tries the policy's protected branches
# (origin/main, main, origin/master, master by default) ..HEAD.
#
# PR/MR body: export CODEFLOW_PR_BODY to also scan it for AI attribution + emoji.
#
# Usage:
#   ci-generic.sh <base> <head>        # explicit range
#   BASE=main HEAD=HEAD ci-generic.sh  # via env
#   ci-generic.sh                      # auto-detect from CI env
set -eu

BASE="${1:-${BASE:-}}"
HEAD="${2:-${HEAD:-}}"

if ! command -v codeflow >/dev/null 2>&1; then
  # A missing binary is an unarmed perimeter, not a pass — fail RED. Install the
  # codeflow binary onto PATH before this runs, e.g.:
  #   curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-installer.sh | sh
  #   export PATH="$HOME/.codeflow/bin:$PATH"
  echo "codeflow not installed — install the binary onto PATH first (failing red)." >&2
  exit 1
fi

if [ -n "$BASE" ] && [ -n "$HEAD" ]; then
  codeflow ci --base "$BASE" --head "$HEAD"
else
  codeflow ci
fi
codeflow test
codeflow validate --docs

# Optional external add-ons (uncomment once the tools are on PATH):
#   gitleaks detect --source . --redact --no-banner --exit-code 1   # secret scan
#   osv-scanner scan -r .                                           # dep audit
