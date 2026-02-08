#!/usr/bin/env bash
# Purpose:   SessionStart hook for session initialization and cleanup
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-cleanup.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Generates unique session ID
#   - Creates session directories
#   - Initializes sentinel directory
#   - Cleans up expired sentinels from previous sessions
#   - Writes session metadata
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Session initialized successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SESSION ID GENERATION
# =============================================================================

# Generate session ID if not already set
if [[ -z "${CODEFLOW_SESSION_ID:-}" ]]; then
    # Generate ULID-like session ID
    TIMESTAMP=$(date +%s%N | cut -c1-13)
    RANDOM_PART=$(head -c 6 /dev/urandom | od -An -tx1 | tr -d ' \n')
    CODEFLOW_SESSION_ID="ses-${TIMESTAMP}${RANDOM_PART}"
    export CODEFLOW_SESSION_ID
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Read sentinel directory from config
SENTINEL_DIR="/tmp/claude/managed/sentinels"
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    SENTINEL_DIR=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null || echo "/tmp/claude/managed/sentinels")
fi

# =============================================================================
# DIRECTORY CREATION
# =============================================================================

# Create session directories
mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/logs/security" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/db" 2>/dev/null || true

# Create sentinel directory
mkdir -p "$SENTINEL_DIR" 2>/dev/null || true

# =============================================================================
# SENTINEL CLEANUP
# =============================================================================

# Clean up expired sentinels from previous sessions
SENTINEL_TTL=600
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    SENTINEL_TTL=$(jq -r '.sentinel.default_ttl // 600' "$CONFIG" 2>/dev/null || echo "600")
fi

# Remove sentinels older than TTL (sentinel library creates *.json files)
if [[ -d "$SENTINEL_DIR" ]]; then
    find "$SENTINEL_DIR" -name "*.json" -type f -mmin "+$((SENTINEL_TTL / 60))" -delete 2>/dev/null || true
fi

# =============================================================================
# SESSION METADATA
# =============================================================================

SESSION_META_FILE="$REPO_ROOT/.state/logs/sessions/session-${CODEFLOW_SESSION_ID}.meta"

# Get git info
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
GIT_COMMIT=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Write session metadata (include started_epoch for reliable duration calculation)
if command -v jq &>/dev/null; then
    jq -nc \
        --arg session_id "$CODEFLOW_SESSION_ID" \
        --arg started_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --argjson started_epoch "$(date +%s)" \
        --arg repo_root "$REPO_ROOT" \
        --arg git_branch "$GIT_BRANCH" \
        --arg git_commit "$GIT_COMMIT" \
        --arg user "${USER:-unknown}" \
        '{session_id: $session_id, started_at: $started_at, started_epoch: $started_epoch, repo_root: $repo_root, git_branch: $git_branch, git_commit: $git_commit, user: $user}' \
        > "$SESSION_META_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
