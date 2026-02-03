#!/usr/bin/env bash
# CodeFlow Shell Library: Index
# Location: .codeflow/scripts/shell-lib/index.sh
#
# Usage: source .codeflow/scripts/shell-lib/index.sh

# Source guard to prevent multiple loads in same shell
# Note: Only use _CODEFLOW_SHELL_LIB_LOADED (non-exported) to prevent subshell issues
[[ -n "${_CODEFLOW_SHELL_LIB_LOADED:-}" ]] && return 0

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load all modules in order (respecting dependencies)
source "$LIB_DIR/common.sh"
source "$LIB_DIR/logging.sh"
source "$LIB_DIR/errors.sh"
source "$LIB_DIR/config.sh"
source "$LIB_DIR/validation.sh"
source "$LIB_DIR/ulid.sh"

# Set library loaded flag (non-exported to avoid subshell issues)
_CODEFLOW_SHELL_LIB_LOADED=1
