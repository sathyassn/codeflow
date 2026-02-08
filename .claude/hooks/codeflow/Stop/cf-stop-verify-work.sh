#!/usr/bin/env bash
# Purpose:   Stop hook for work verification (PCV)
# Location:  .claude/hooks/codeflow/Stop/cf-stop-verify-work.sh
# Hook Type: Stop
# Usage:     Called by Claude Code at stop event
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Verifies PCV (Post-Completion Verification) marker presence
#   - Checks for required verification sections by tier
#   - Provides advisory guidance when verification missing
#
# Verification tiers (from enforcement-policy.json):
#   - Tier 1: verify-work, TIER
#   - Tier 2: verify-work, TIER, ARTIFACTS, VERIFICATION
#   - Tier 3: verify-work, TIER, ARTIFACTS, VERIFICATION, ADVERSARIAL
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (Stop hooks never block execution)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SETTINGS_LOCAL="$REPO_ROOT/.claude/settings.local.json"
SETTINGS="$REPO_ROOT/.claude/settings.json"

# Get stop context from environment
STOP_REASON="${STOP_REASON:-}"
LAST_MESSAGE="${LAST_MESSAGE:-}"

# =============================================================================
# CONFIG LOADING
# =============================================================================

# Read verification config from enforcement-policy.json with defaults
REQUIRE_PCV=true
# shellcheck disable=SC2034  # REQUIRE_TASK_COMPLETION reserved for future task status check
REQUIRE_TASK_COMPLETION=true
# shellcheck disable=SC2034  # MAX_RETRIES reserved for retry logic
MAX_RETRIES=3

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    REQUIRE_PCV=$(jq -r '.stop_verification.require_pcv_marker // true' "$CONFIG" 2>/dev/null || echo "true")
    # shellcheck disable=SC2034  # Reserved for future task status check
    REQUIRE_TASK_COMPLETION=$(jq -r '.stop_verification.require_task_completion // true' "$CONFIG" 2>/dev/null || echo "true")
    MAX_RETRIES=$(jq -r '.stop_verification.max_retries // 3' "$CONFIG" 2>/dev/null || echo "3")
fi

# Read _verify_work_config from settings (local overrides project)
get_settings_config() {
    local key="$1"
    local default="$2"
    local value=""

    # Try settings.local.json first (local overrides)
    if [[ -f "$SETTINGS_LOCAL" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS_LOCAL" 2>/dev/null || echo "")
    fi

    # Fall back to settings.json
    if [[ -z "$value" ]] && [[ -f "$SETTINGS" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS" 2>/dev/null || echo "")
    fi

    echo "${value:-$default}"
}

# Allow settings override for max retries
SETTINGS_RETRIES=$(get_settings_config '._verify_work_config.maxRetries' '')
if [[ -n "$SETTINGS_RETRIES" ]]; then
    # shellcheck disable=SC2034  # Reserved for retry logic
    MAX_RETRIES="$SETTINGS_RETRIES"
fi

# =============================================================================
# PCV MARKER CHECK
# =============================================================================

# Required marker from config
REQUIRED_MARKER="🔍"
REQUIRED_TEXT="verify-work"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    REQUIRED_MARKER=$(jq -r '.stop_hooks."working-protocol"."verify-work".required_marker // "🔍"' "$CONFIG" 2>/dev/null || echo "🔍")
    REQUIRED_TEXT=$(jq -r '.stop_hooks."working-protocol"."verify-work".required_text // "verify-work"' "$CONFIG" 2>/dev/null || echo "verify-work")
fi

check_pcv_marker() {
    local message="$1"

    # Check for required marker (🔍)
    if [[ "$message" == *"$REQUIRED_MARKER"* ]]; then
        return 0
    fi

    # Check for required text (verify-work)
    if [[ "$message" == *"$REQUIRED_TEXT"* ]] || [[ "$message" == *"VERIFY-WORK"* ]]; then
        return 0
    fi

    # Check for explicit verification statement
    if [[ "$message" == *"verification complete"* ]] || [[ "$message" == *"work verified"* ]]; then
        return 0
    fi

    return 1
}

# =============================================================================
# TIER CHECK
# =============================================================================

# Read tier requirements from config (with defaults matching cf-working-protocol skill)
TIER_2_SECTIONS="ARTIFACTS VERIFICATION"
TIER_3_SECTIONS="ARTIFACTS VERIFICATION ADVERSARIAL"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Read from stop_verification.tier_requirements (primary source)
    T2_REQ=$(jq -r '.stop_verification.tier_requirements["2"] // empty | .[2:]? | join(" ")' "$CONFIG" 2>/dev/null || echo "")
    T3_REQ=$(jq -r '.stop_verification.tier_requirements["3"] // empty | .[2:]? | join(" ")' "$CONFIG" 2>/dev/null || echo "")

    if [[ -n "$T2_REQ" ]]; then
        TIER_2_SECTIONS="$T2_REQ"
    fi
    if [[ -n "$T3_REQ" ]]; then
        TIER_3_SECTIONS="$T3_REQ"
    fi
fi

check_tier_requirements() {
    local message="$1"
    local tier=""

    # Detect tier from message
    if [[ "$message" == *"TIER 3"* ]] || [[ "$message" == *"TIER3"* ]]; then
        tier="3"
    elif [[ "$message" == *"TIER 2"* ]] || [[ "$message" == *"TIER2"* ]]; then
        tier="2"
    elif [[ "$message" == *"TIER 1"* ]] || [[ "$message" == *"TIER1"* ]]; then
        tier="1"
    fi

    # If no tier detected, skip tier-specific checks
    if [[ -z "$tier" ]]; then
        return 0
    fi

    # Check tier-specific requirements (config-driven)
    local missing_sections=""
    local required_sections=""

    case "$tier" in
        "3")
            required_sections="$TIER_3_SECTIONS"
            ;;
        "2")
            required_sections="$TIER_2_SECTIONS"
            ;;
        "1")
            # Tier 1 has no section requirements
            return 0
            ;;
    esac

    # Check each required section
    for section in $required_sections; do
        if [[ "$message" != *"$section"* ]]; then
            if [[ -n "$missing_sections" ]]; then
                missing_sections="$missing_sections, $section"
            else
                missing_sections="$section"
            fi
        fi
    done

    if [[ -n "$missing_sections" ]]; then
        echo "Note: TIER $tier verification should include: $missing_sections section(s)." >&2
    fi

    return 0
}

# =============================================================================
# VERIFICATION EXECUTION
# =============================================================================

# Skip verification if not required
if [[ "$REQUIRE_PCV" != "true" ]]; then
    exit 0
fi

# Skip if stop reason is explicit user termination or context limit
if [[ "$STOP_REASON" == "user_cancelled" ]] || [[ "$STOP_REASON" == "context_limit" ]]; then
    exit 0
fi

# Check PCV marker in last message
if [[ -n "$LAST_MESSAGE" ]]; then
    if ! check_pcv_marker "$LAST_MESSAGE"; then
        # PCV marker missing - provide guidance (advisory only)
        cat >&2 <<'EOF'
Note: Work verification (PCV) not detected.

Before stopping, consider verifying your work:
1. Review changes made (🔍 verify-work marker)
2. Run relevant tests
3. Confirm task completion

Include verification summary in your response.
EOF
        # Don't block - just advise (Stop hooks always exit 0)
    else
        # Check tier-specific requirements
        check_tier_requirements "$LAST_MESSAGE"
    fi
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
