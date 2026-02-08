#!/usr/bin/env bash
# Purpose:   PreToolUse hook to validate claims before Edit/Write operations
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-claim-validation.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write
# Enforcement: L2 (warn but allow)
#
# Checks if the file being edited/written overlaps with claims from OTHER users.
# Warns if there's a potential conflict but allows the operation to proceed.
#
# Also checks Draft PRs for overlapping scope claims.
#
# Configuration: Reads claims from .state/active-work-claims.yaml
#
# Exit codes:
#   - 0: Command allowed (with optional warnings)

set -euo pipefail

# =============================================================================
# EARLY EXIT FOR NON-EDIT/WRITE TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
CLAIMS_FILE="$REPO_ROOT/.state/active-work-claims.yaml"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# V4: AGENT-TEAMS MODE BYPASS
# =============================================================================
# In agent-teams mode, claim validation is handled by the team coordination
# system (task ownership via TaskUpdate). Skip local claims check.

if declare -f is_agent_teams_active &>/dev/null && is_agent_teams_active; then
    # Agent-teams mode: team task ownership replaces local claim validation
    exit 0
fi

# =============================================================================
# PARSE INPUT
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract file path from JSON input
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$TOOL_INPUT" | jq -r '.file_path // .path // empty' 2>/dev/null)
else
    FILE_PATH=$(echo "$TOOL_INPUT" | grep -o '"file_path":"[^"]*"' | sed 's/"file_path":"//;s/"$//' || echo "")
fi

# Exit silently if no file path
[[ -z "$FILE_PATH" ]] && exit 0

# =============================================================================
# DRAFT PR SCOPE CHECK
# =============================================================================
# Check if file overlaps with scope claims in Draft PRs from other users.
# This is L2 enforcement: warn but don't block (exit 0).

DRAFT_PR_SCRIPT="$REPO_ROOT/.codeflow/scripts/state/draft-pullrequest-scope-check.sh"

if [[ -x "$DRAFT_PR_SCRIPT" ]]; then
    # Call Draft PR checker with JSON output
    DRAFT_PR_OUTPUT=$("$DRAFT_PR_SCRIPT" check --scope "$FILE_PATH" --json 2>/dev/null) || true

    if [[ -n "$DRAFT_PR_OUTPUT" ]] && command -v jq &>/dev/null; then
        # Parse status from JSON output
        DRAFT_STATUS=$(echo "$DRAFT_PR_OUTPUT" | jq -r '.status // "CLEAR"' 2>/dev/null || echo "CLEAR")

        if [[ "$DRAFT_STATUS" == "CONFLICT" ]] || [[ "$DRAFT_STATUS" == "WARN" ]]; then
            # Extract conflict details for warning message
            DRAFT_CONFLICTS=$(echo "$DRAFT_PR_OUTPUT" | jq -r '
                .conflicts[]? |
                "PR #\(.pr_number) (\(.mode)): \(.owner)\n  URL: \(.pr_url)\n  Work: \(.work_id)\n  Overlapping: \(.overlapping_patterns | join(", "))"
            ' 2>/dev/null || echo "")

            if [[ "$DRAFT_STATUS" == "CONFLICT" ]]; then
                # Log the conflict warning
                if declare -f log_security_event &>/dev/null; then
                    log_security_event "warn" "draft_pr_conflict" "$TOOL_NAME" "$FILE_PATH" "Exclusive scope claim"
                fi
                cat << EOF
WARNING: DRAFT PR CONFLICT - Exclusive scope claim exists in Draft PR

File being edited: $FILE_PATH

$DRAFT_CONFLICTS

This file overlaps with an exclusive scope claim in a Draft PR.
Consider:
  1. Coordinate with the PR owner before modifying
  2. Check the Draft PR for context on claimed scope
  3. Wait for the PR to be merged or closed
EOF
            else
                # Log the notice
                if declare -f log_security_event &>/dev/null; then
                    log_security_event "audit" "draft_pr_notice" "$TOOL_NAME" "$FILE_PATH" "Shared scope claim"
                fi
                cat << EOF
NOTICE: DRAFT PR - Shared scope claim exists in Draft PR

File being edited: $FILE_PATH

$DRAFT_CONFLICTS

Shared claims allow parallel work but be aware of potential conflicts.
EOF
            fi
        fi
    fi
fi

# =============================================================================
# LOCAL CLAIMS CHECK
# =============================================================================

# Skip if no claims file exists
[[ ! -f "$CLAIMS_FILE" ]] && exit 0

# Get current user identity
CURRENT_USER="$(git config user.name 2>/dev/null || echo "${USER:-unknown}")"
CURRENT_MACHINE="$(hostname -s 2>/dev/null || echo 'unknown')"

# Check for conflicting claims using Python (for YAML parsing)
# Uses PyYAML directly instead of external yaml_utils dependency
CONFLICT_CHECK=$(/usr/bin/python3 << PYTHON_EOF
import sys
import fnmatch

try:
    import yaml
except ImportError:
    # PyYAML not available - gracefully allow operation
    sys.exit(0)

try:
    with open('$CLAIMS_FILE', 'r', encoding='utf-8') as f:
        data = yaml.safe_load(f) or {}
except Exception:
    # File read/parse error - gracefully allow operation
    sys.exit(0)

file_path = '$FILE_PATH'
current_user = '$CURRENT_USER'
current_machine = '$CURRENT_MACHINE'

# Normalize file path (remove leading ./ if present)
if file_path.startswith('./'):
    file_path = file_path[2:]

conflicts = []

for claim in data.get('claims', []):
    # Skip own claims
    if claim.get('user') == current_user and claim.get('machine') == current_machine:
        continue

    # Skip non-active claims
    if claim.get('status') != 'active':
        continue

    # Check scope patterns
    file_scope = claim.get('file_scope', {})
    patterns = file_scope.get('patterns', [])
    mode = file_scope.get('mode', 'shared')

    for pattern in patterns:
        # Normalize pattern
        if pattern.startswith('./'):
            pattern = pattern[2:]

        # Check if file matches pattern
        if fnmatch.fnmatch(file_path, pattern):
            conflicts.append({
                'claim_id': claim.get('id'),
                'user': claim.get('user'),
                'machine': claim.get('machine'),
                'pattern': pattern,
                'mode': mode,
                'description': claim.get('work_description', 'No description')
            })
            break

if conflicts:
    for c in conflicts:
        mode_indicator = "EXCLUSIVE" if c['mode'] == 'exclusive' else "shared"
        print(f"CONFLICT:{c['claim_id']}:{c['user']}@{c['machine']}:{c['pattern']}:{mode_indicator}:{c['description']}")
else:
    print("OK")
PYTHON_EOF
)

# Parse conflict check result
if [[ "$CONFLICT_CHECK" == "OK" ]] || [[ -z "$CONFLICT_CHECK" ]]; then
    # No conflicts, proceed
    exit 0
fi

# Parse conflicts and output warnings
while IFS= read -r line; do
    if [[ "$line" == CONFLICT:* ]]; then
        # Parse: CONFLICT:claim_id:user@machine:pattern:mode:description
        IFS=':' read -r _ claim_id user_machine pattern mode description <<< "$line"

        if [[ "$mode" == "EXCLUSIVE" ]]; then
            # Log exclusive claim warning
            if declare -f log_security_event &>/dev/null; then
                log_security_event "warn" "claim_conflict_exclusive" "$TOOL_NAME" "$FILE_PATH" "Claimed by $user_machine"
            fi
            # Exclusive claim - strong warning
            cat << EOF
WARNING: CLAIM CONFLICT - Exclusive claim exists on overlapping files

Claim ID: $claim_id
Owner: $user_machine
Pattern: $pattern
Work: $description

This file is exclusively claimed by another user/machine.
Consider:
  1. Coordinate with $user_machine before modifying
  2. Check claim status: .codeflow/scripts/state/work-claims.sh list
  3. Request claim release if work is complete
EOF
        else
            # Log shared claim notice
            if declare -f log_security_event &>/dev/null; then
                log_security_event "audit" "claim_notice_shared" "$TOOL_NAME" "$FILE_PATH" "Shared with $user_machine"
            fi
            # Shared claim - informational warning
            cat << EOF
NOTICE: CLAIM - Shared claim exists on overlapping files

Claim ID: $claim_id
Owner: $user_machine
Pattern: $pattern
Work: $description

Shared claims allow parallel work but be aware of potential conflicts.
EOF
        fi
    fi
done <<< "$CONFLICT_CHECK"

# L2 enforcement: warn but allow (exit 0)
exit 0
