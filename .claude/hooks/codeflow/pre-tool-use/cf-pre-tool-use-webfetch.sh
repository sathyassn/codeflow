#!/usr/bin/env bash
# Purpose:   PreToolUse hook for network access validation (domain allowlist)
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh
# Hook Type: PreToolUse
# Matcher:   Bash|WebFetch|WebSearch
#
# This hook:
#   - Validates URLs against trusted domain allowlist
#   - Blocks access to untrusted domains
#   - Logs network activity for audit
#   - Applies to Bash (curl, wget) and WebFetch tools
#
# Configuration: Reads from enforcement-policy.json and settings.json
#   - trusted_domains: Allowlisted domains
#   - _web_fetch_config.approvalMode: autonomous|ask|block
#   - _web_fetch_config.untrustedAction: ask|block
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Network access allowed (trusted domain or no URL)
#   - 2: Network access blocked (untrusted domain)

set -euo pipefail

# =============================================================================
# TOOL FILTERING
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"

# Only check Bash and WebFetch tools
case "$TOOL_NAME" in
    Bash|WebFetch|WebSearch)
        ;;
    *)
        exit 0
        ;;
esac

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SETTINGS_LOCAL="$REPO_ROOT/.claude/settings.local.json"
SETTINGS="$REPO_ROOT/.claude/settings.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# SETTINGS CONFIG
# =============================================================================

# Read _web_fetch_config from settings (local overrides project)
get_settings_config() {
    local key="$1"
    local default="$2"
    local value=""

    # Try settings.local.json first (local overrides)
    if [[ -f "$SETTINGS_LOCAL" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS_LOCAL" 2>/dev/null)
    fi

    # Fall back to settings.json
    if [[ -z "$value" ]] && [[ -f "$SETTINGS" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS" 2>/dev/null)
    fi

    echo "${value:-$default}"
}

# approvalMode: "strict" (all require approval), "standard" (core domains), "autonomous" (extended), "permissive" (broad)
APPROVAL_MODE=$(get_settings_config '._web_fetch_config.approvalMode' 'standard')
# untrustedAction: "ask" (prompt), "block" (deny)
UNTRUSTED_ACTION=$(get_settings_config '._web_fetch_config.untrustedAction' 'ask')

# =============================================================================
# ALWAYS-BLOCK DOMAINS (config-driven)
# =============================================================================
# NOTE: Developers needing localhost access should:
#   1. Add "localhost" to the appropriate .list file in .codeflow/config/enforcement/trusted-domains/, OR
#   2. Set network.always_block_domains.enabled=false in enforcement-policy.json

# Default always-block domains
ALWAYS_BLOCK_DOMAINS=(
    "localhost"
    "127.0.0.1"
    "0.0.0.0"
    "::1"
    "*.local"
    "internal.*"
    "*.internal"
    "*.corp"
    "*.corp.*"
    "*.lan"
    "*.home"
    "*.private"
)

# Private IP ranges
PRIVATE_IP_RANGES=(
    "169.254.*"
    "10.*"
    "172.16.*" "172.17.*" "172.18.*" "172.19.*"
    "172.20.*" "172.21.*" "172.22.*" "172.23.*"
    "172.24.*" "172.25.*" "172.26.*" "172.27.*"
    "172.28.*" "172.29.*" "172.30.*" "172.31.*"
    "192.168.*"
)

# Load from config if available
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Check if always_block is enabled (default true)
    block_enabled=$(jq -r '.network.always_block_domains.enabled // true' "$CONFIG" 2>/dev/null)

    if [[ "$block_enabled" == "true" ]]; then
        # Load patterns from config (overrides defaults if present)
        config_patterns=()
        while IFS= read -r pattern; do
            [[ -n "$pattern" ]] && config_patterns+=("$pattern")
        done < <(jq -r '.network.always_block_domains.patterns[]? // empty' "$CONFIG" 2>/dev/null)

        if [[ ${#config_patterns[@]} -gt 0 ]]; then
            ALWAYS_BLOCK_DOMAINS=("${config_patterns[@]}")
        fi

        # Load private IP ranges from config
        config_ips=()
        while IFS= read -r ip; do
            [[ -n "$ip" ]] && config_ips+=("$ip")
        done < <(jq -r '.network.always_block_domains.private_ip_ranges[]? // empty' "$CONFIG" 2>/dev/null)

        if [[ ${#config_ips[@]} -gt 0 ]]; then
            PRIVATE_IP_RANGES=("${config_ips[@]}")
        fi
    else
        # Blocking disabled via config - allow all local access
        ALWAYS_BLOCK_DOMAINS=()
        PRIVATE_IP_RANGES=()
    fi
fi

# Combine patterns and IP ranges
ALWAYS_BLOCK_DOMAINS+=("${PRIVATE_IP_RANGES[@]}")

# =============================================================================
# TRUSTED DOMAINS (loaded from .list files based on approvalMode)
# =============================================================================

# Trusted domains directory
TRUSTED_DOMAINS_DIR="$REPO_ROOT/.codeflow/config/enforcement/trusted-domains"

# Map approvalMode to list file
# strict = no list (all require approval)
# standard/autonomous/permissive = corresponding .list file
TRUSTED_DOMAINS=()

load_trusted_domains() {
    local list_file="$1"

    if [[ ! -f "$list_file" ]]; then
        return
    fi

    # Parse list file: skip comments (#) and empty lines
    while IFS= read -r line || [[ -n "$line" ]]; do
        # Trim whitespace
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"

        # Skip empty lines and comments
        [[ -z "$line" ]] && continue
        [[ "$line" == \#* ]] && continue

        TRUSTED_DOMAINS+=("$line")
    done < "$list_file"
}

case "$APPROVAL_MODE" in
    strict)
        # No trusted domains - all require approval
        TRUSTED_DOMAINS=()
        ;;
    standard)
        load_trusted_domains "$TRUSTED_DOMAINS_DIR/standard.list"
        ;;
    autonomous)
        load_trusted_domains "$TRUSTED_DOMAINS_DIR/autonomous.list"
        ;;
    permissive)
        load_trusted_domains "$TRUSTED_DOMAINS_DIR/permissive.list"
        ;;
    *)
        # Default to standard mode
        load_trusted_domains "$TRUSTED_DOMAINS_DIR/standard.list"
        ;;
esac

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract URL(s) from tool input
URLS=()

if [[ "$TOOL_NAME" == "Bash" ]]; then
    # Extract command
    COMMAND=""
    if command -v jq &>/dev/null; then
        COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
    else
        COMMAND=$(echo "$TOOL_INPUT" | grep -o '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
    fi

    # Check if command uses network tools
    if [[ ! "$COMMAND" =~ (curl|wget|fetch|nc|netcat|ssh|scp|rsync|ftp) ]]; then
        exit 0  # No network commands
    fi

    # Extract URLs from command
    while IFS= read -r url; do
        [[ -n "$url" ]] && URLS+=("$url")
    done < <(echo "$COMMAND" | grep -oE 'https?://[^[:space:]"'\'']+' || true)

elif [[ "$TOOL_NAME" == "WebFetch" ]] || [[ "$TOOL_NAME" == "WebSearch" ]]; then
    # Extract URL from WebFetch input
    if command -v jq &>/dev/null; then
        url=$(echo "$TOOL_INPUT" | jq -r '.url // empty')
        [[ -n "$url" ]] && URLS+=("$url")
    fi
fi

# If no URLs found, allow
if [[ ${#URLS[@]} -eq 0 ]]; then
    exit 0
fi

# =============================================================================
# DOMAIN EXTRACTION
# =============================================================================

extract_domain() {
    local url="$1"
    # Remove protocol
    local no_proto="${url#*://}"
    # Extract domain (before first / or end)
    local domain="${no_proto%%/*}"
    # Remove port if present
    domain="${domain%%:*}"
    # Remove user info if present
    domain="${domain##*@}"
    echo "$domain"
}

# =============================================================================
# VALIDATION
# =============================================================================

# Check if domain matches always-block patterns
is_always_blocked_domain() {
    local domain="$1"

    # If no block patterns configured, allow all
    if [[ ${#ALWAYS_BLOCK_DOMAINS[@]} -eq 0 ]]; then
        return 1
    fi

    for blocked in "${ALWAYS_BLOCK_DOMAINS[@]}"; do
        # Exact match
        if [[ "$domain" == "$blocked" ]]; then
            return 0
        fi

        # Wildcard pattern match (*.local, internal.*, etc.)
        if [[ "$blocked" == \*\.* ]]; then
            # Pattern like *.local - check suffix
            local suffix="${blocked#\*.}"
            if [[ "$domain" == *".$suffix" ]] || [[ "$domain" == "$suffix" ]]; then
                return 0
            fi
        elif [[ "$blocked" == *\.\* ]]; then
            # Pattern like internal.* - check prefix
            local prefix="${blocked%.\*}"
            if [[ "$domain" == "$prefix."* ]] || [[ "$domain" == "$prefix" ]]; then
                return 0
            fi
        elif [[ "$blocked" == *\* ]]; then
            # Pattern like 192.168.* - check prefix
            local prefix="${blocked%\*}"
            if [[ "$domain" == "$prefix"* ]]; then
                return 0
            fi
        fi
    done
    return 1
}

is_trusted_domain() {
    local domain="$1"

    for trusted in "${TRUSTED_DOMAINS[@]}"; do
        # Exact match
        if [[ "$domain" == "$trusted" ]]; then
            return 0
        fi
        # Subdomain match (e.g., api.github.com matches github.com)
        if [[ "$domain" == *".$trusted" ]]; then
            return 0
        fi
    done
    return 1
}

ask_network() {
    local url="$1"
    local domain="$2"

    # Log network ask
    if declare -f log_network &>/dev/null; then
        log_network "network_ask" "$TOOL_NAME" "$url" "$domain" "not_in_allowlist"
    fi

    # Output JSON to trigger user approval prompt
    echo "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"Domain '$domain' not in ${APPROVAL_MODE} allowlist\"}}"
    exit 0
}

block_network() {
    local url="$1"
    local domain="$2"

    # Log network block
    if declare -f log_network &>/dev/null; then
        log_network "network_blocked" "$TOOL_NAME" "$url" "$domain" "not_in_allowlist"
    fi

    cat >&2 <<EOF
BLOCKED: Untrusted domain

URL: $url
Domain: $domain
Tool: $TOOL_NAME

This domain is not in the trusted allowlist.
Use the cf-security-management skill to request network access.

MUST: Skill('cf-security-management', args='request-network-access $domain')

Trusted domains are loaded from:
  .codeflow/config/enforcement/trusted-domains/${APPROVAL_MODE}.list

Current mode: $APPROVAL_MODE (set via _web_fetch_config.approvalMode in settings)
EOF
    exit 2
}

handle_untrusted() {
    local url="$1"
    local domain="$2"

    # Respect untrustedAction setting: "ask" (default) or "block"
    if [[ "$UNTRUSTED_ACTION" == "block" ]]; then
        block_network "$url" "$domain"
    else
        # Default: ask for user approval
        ask_network "$url" "$domain"
    fi
}

# Block function for always-blocked domains
block_always_blocked() {
    local url="$1"
    local domain="$2"

    # Log network block
    if declare -f log_network &>/dev/null; then
        log_network "network_blocked" "$TOOL_NAME" "$url" "$domain" "always_blocked"
    fi

    cat >&2 <<EOF
BLOCKED: Internal/local network access forbidden

URL: $url
Domain: $domain
Tool: $TOOL_NAME

This domain is on the always-block list for security reasons.
Access to localhost, internal networks, and private IPs is not allowed by default.

Always-blocked patterns include:
  - localhost, 127.0.0.1, ::1
  - *.local, internal.*, *.corp.*
  - Private IP ranges (10.*, 172.16-31.*, 192.168.*)

Developer override options:
  1. Add domain to the trusted domains list file:
     .codeflow/config/enforcement/trusted-domains/${APPROVAL_MODE}.list
  2. Or disable blocking in enforcement-policy.json:
     "network": { "always_block_domains": { "enabled": false } }
EOF
    exit 2
}

# Check each URL
for url in "${URLS[@]}"; do
    domain=$(extract_domain "$url")

    # FIRST: Check trusted domains - trusted domains bypass always-block
    if is_trusted_domain "$domain"; then
        # Log allowed network access
        if declare -f log_network &>/dev/null; then
            log_network "network_allowed" "$TOOL_NAME" "$url" "$domain" "trusted"
        fi
        continue
    fi

    # SECOND: Check always-block list (only if NOT trusted)
    if is_always_blocked_domain "$domain"; then
        block_always_blocked "$url" "$domain"
    fi

    # THIRD: Not trusted and not blocked - handle per untrustedAction setting
    handle_untrusted "$url" "$domain"
done

# =============================================================================
# NETWORK ACCESS ALLOWED (all URLs passed)
# =============================================================================

exit 0
