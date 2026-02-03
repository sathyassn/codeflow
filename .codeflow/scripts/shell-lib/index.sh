#!/usr/bin/env bash
# CodeFlow Shell Library: Index
# Location: .codeflow/scripts/shell-lib/index.sh
#
# Usage: source .codeflow/scripts/shell-lib/index.sh

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load all modules in order (respecting dependencies)
source "$LIB_DIR/common.sh"
source "$LIB_DIR/logging.sh"
source "$LIB_DIR/errors.sh"
source "$LIB_DIR/config.sh"
source "$LIB_DIR/validation.sh"
source "$LIB_DIR/ulid.sh"

# Export library loaded flag
export CODEFLOW_SHELL_LIB_LOADED=1
