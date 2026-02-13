#!/usr/bin/env bash
# Purpose:   Set up git worktree for parallel development
# Usage:     cf-worktree-setup.sh <worktree-path> <branch-name> [agent-type] [purpose] [trigger]
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This script:
#   1. Configures the worktree (copies configs, installs deps)
#   2. Creates symlink to main repo's .state/ for unified tracking
#   3. Registers worktree in .state/worktrees.yaml
#
# Arguments:
#   worktree-path  Path to worktree (e.g., .git-worktrees/feat-phase-6-1)
#   branch-name    Branch name (e.g., feat/phase-6-1)
#   agent-type     Agent type: general, planner, developer, documenter (default: unknown)
#   purpose        Optional description of work purpose
#   trigger        What triggered creation: manual, model-orchestrator, command (default: manual)
#
# Exit codes:
#   - 0: Setup successful
#   - 1: Error (missing arguments, invalid path, etc.)

set -euo pipefail

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
cf-worktree-setup.sh - Set up git worktree for parallel development

USAGE:
    cf-worktree-setup.sh <worktree-path> <branch-name> [agent-type] [purpose] [trigger]

ARGUMENTS:
    worktree-path  Path to worktree (e.g., .git-worktrees/feat-phase-6-1)
    branch-name    Branch name (e.g., feat/phase-6-1)
    agent-type     Agent type: general, planner, developer, documenter (default: unknown)
    purpose        Optional description of work purpose
    trigger        What triggered creation: manual, model-orchestrator, command (default: manual)

OPTIONS:
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    cf-worktree-setup.sh .git-worktrees/feat-auth feat/auth developer "Implement auth"
    cf-worktree-setup.sh .git-worktrees/fix-bug fix/memory-leak general
EOF
}

show_version() {
    echo "cf-worktree-setup.sh version 1.1.0"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

case "${1:-}" in
    -h|--help)
        show_help
        exit 0
        ;;
    -V|--version)
        show_version
        exit 0
        ;;
esac

WORKTREE_PATH="${1:-}"
BRANCH_NAME="${2:-}"
AGENT_TYPE="${3:-unknown}"
PURPOSE="${4:-}"
TRIGGER="${5:-manual}"

if [[ -z "$WORKTREE_PATH" ]] || [[ -z "$BRANCH_NAME" ]]; then
    echo "Error: Missing required arguments" >&2
    echo "  Required: <worktree-path> <branch-name>" >&2
    echo "  Use --help for usage information" >&2
    exit 1
fi

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# Validate worktree path exists
if [[ ! -d "$WORKTREE_PATH" ]]; then
    echo "Error: Worktree path does not exist: $WORKTREE_PATH" >&2
    echo "  Create the worktree first: git worktree add <path> -b <branch>" >&2
    exit 1
fi

# PathFlow detection (optional, for mode-aware messaging)
_IS_PATHFLOW=false
if [[ -f "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh" ]]; then
    # shellcheck source=../security/lib/context-lib.sh
    source "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh" 2>/dev/null || true
    if is_pathflow_active 2>/dev/null; then
        _IS_PATHFLOW=true
    fi
fi

echo "Setting up worktree: $WORKTREE_PATH"
echo "  Branch: $BRANCH_NAME"
echo "  Agent: $AGENT_TYPE"
[[ -n "$PURPOSE" ]] && echo "  Purpose: $PURPOSE"

cd "$WORKTREE_PATH"

# =============================================================================
# GENERIC SETUP
# =============================================================================

echo "Copying essential config files..."
cp "$REPO_ROOT/.env.example" .env 2>/dev/null || true
cp "$REPO_ROOT/.gitignore" .gitignore 2>/dev/null || echo "  .gitignore already exists"

# =============================================================================
# STATE SYMLINK
# =============================================================================

echo "Creating .state symlink to main repo..."
if [[ ! -e ".state" ]]; then
    ln -s "$REPO_ROOT/.state" ".state"
    echo "  Created: .state -> $REPO_ROOT/.state"
else
    if [[ -L ".state" ]]; then
        echo "  .state symlink already exists"
    else
        echo "  Warning: .state exists but is not a symlink"
    fi
fi

# =============================================================================
# WORKTREE SPECIFIC SETUP
# =============================================================================

if [[ -f "package.json" ]]; then
    echo "Installing Node.js dependencies..."
    npm install --silent --no-progress 2>&1 | grep -v "npm WARN" || true
fi

# =============================================================================
# VERIFY SETUP
# =============================================================================

echo "Verifying setup..."

SETUP_WARNINGS=()

if [[ -f "package.json" ]] && [[ ! -d "node_modules" ]]; then
    SETUP_WARNINGS+=("Node.js: node_modules/ missing (run 'npm install')")
fi

if [[ ${#SETUP_WARNINGS[@]} -gt 0 ]]; then
    echo ""
    echo "SETUP WARNINGS:"
    for warning in "${SETUP_WARNINGS[@]}"; do
        echo "  - $warning"
    done
fi

# =============================================================================
# REGISTER WORKTREE
# =============================================================================
# Registers in .state/worktrees.yaml (YAML-based tracking).

echo "Registering worktree in .state/worktrees.yaml..."

STATE_DIR="$REPO_ROOT/.state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"

mkdir -p "$STATE_DIR"

CREATED_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

if [[ ! -f "$WORKTREES_FILE" ]]; then
    cat > "$WORKTREES_FILE" << 'YAML_HEADER'
# Worktree Tracking
# Managed by: cf-git-workflow skill / .codeflow/scripts/worktree/cf-worktree-setup.sh

worktrees: []

metadata:
  version: "1.0.0"
  last_updated: null
YAML_HEADER
fi

if grep -q "path: \"$WORKTREE_PATH\"" "$WORKTREES_FILE" 2>/dev/null; then
    echo "  Worktree already registered, updating status..."
    # Scope the status update to only the matching worktree entry
    TEMP_FILE=$(mktemp)
    awk -v target_path="$WORKTREE_PATH" '
    /path:/ { found_target = (index($0, "\"" target_path "\"") > 0) }
    found_target && /status: abandoned/ { sub(/status: abandoned/, "status: active"); found_target = 0 }
    /^  - path:/ && !found_target { found_target = 0 }
    { print }
    ' "$WORKTREES_FILE" > "$TEMP_FILE"
    mv "$TEMP_FILE" "$WORKTREES_FILE"
else
    TEMP_FILE=$(mktemp)
    awk -v path="$WORKTREE_PATH" \
        -v branch="$BRANCH_NAME" \
        -v created="$CREATED_AT" \
        -v agent="$AGENT_TYPE" \
        -v trigger="$TRIGGER" \
        -v purpose="$PURPOSE" \
    '
    /^worktrees:/ {
        print "worktrees:"
        print "  - path: \"" path "\""
        print "    branch: \"" branch "\""
        print "    created_at: \"" created "\""
        print "    created_by:"
        print "      agent: \"" agent "\""
        print "      trigger: \"" trigger "\""
        if (purpose != "") {
            print "    purpose: \"" purpose "\""
        }
        print "    status: active"
        next
    }
    { print }
    ' "$WORKTREES_FILE" > "$TEMP_FILE"
    mv "$TEMP_FILE" "$WORKTREES_FILE"
fi

if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s|last_updated:.*|last_updated: \"$CREATED_AT\"|" "$WORKTREES_FILE"
else
    sed -i "s|last_updated:.*|last_updated: \"$CREATED_AT\"|" "$WORKTREES_FILE"
fi

echo "Worktree registered in $WORKTREES_FILE"

# =============================================================================
# COMPLETION MESSAGE
# =============================================================================

echo "Worktree setup complete: $WORKTREE_PATH ($BRANCH_NAME)"

if [[ "$_IS_PATHFLOW" == "true" ]]; then
    echo "  PathFlow active: cf-gitops teammate manages git operations for this worktree"
fi
