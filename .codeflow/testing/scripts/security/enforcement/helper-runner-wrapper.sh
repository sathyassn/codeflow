#!/usr/bin/env bash
# Wrapper to source and test cf-path-protection.sh module
# Usage: bash /tmp/claude/test-runner-wrapper.sh /path/to/command-file
# The command-file contains the COMMAND to test (one line).
# Exit code: 0 = allowed, 2 = blocked

set -euo pipefail

CMDFILE="${1:-}"
if [[ -z "$CMDFILE" ]] || [[ ! -f "$CMDFILE" ]]; then
    echo "Usage: $0 /path/to/command-file" >&2
    exit 1
fi

COMMAND="$(cat "$CMDFILE")"
export COMMAND
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

# Arrays cannot be exported in bash — they must be set here as globals
# before sourcing the module which checks them.
# shellcheck disable=SC2034  # PROTECTED_PATHS is used by the sourced module
PROTECTED_PATHS=(
    ".claude/settings.json"
    ".claude/settings.local.json"
    ".claude/hooks/codeflow"
    ".codeflow/config"
    ".codeflow/scripts/security"
)

# Source the module — it uses return 0 on success, exit 2 on block
# shellcheck source=/dev/null
source "${CF_PATH_PROTECTION_MODULE:-$REPO_ROOT/.codeflow/scripts/security/enforcement/cf-path-protection.sh}"
