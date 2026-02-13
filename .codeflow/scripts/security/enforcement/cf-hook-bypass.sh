#!/usr/bin/env bash
# Purpose:   Extended hook bypass prevention (consolidated into cf-git-protection.sh)
# Location:  .codeflow/scripts/security/enforcement/cf-hook-bypass.sh
# Usage:     source "enforcement/cf-hook-bypass.sh" (from main hook)
# Platform:  macOS/Linux
#
# CONSOLIDATION NOTE:
#   All checks previously in this module have been consolidated into
#   cf-git-protection.sh (Sections 1-4), which runs earlier in the
#   orchestrator chain (position 3 vs this module at position 4).
#
#   Consolidated checks:
#   - core.hooksPath via -c flag → cf-git-protection.sh Section 3
#   - core.hooksPath --unset    → cf-git-protection.sh Section 3
#   - PRE_COMMIT_ALLOW_NO_CONFIG → cf-git-protection.sh Section 3
#   - .git/hooks rm/mv/chmod    → cf-git-protection.sh Section 4
#   - .git/hooks redirect       → cf-git-protection.sh Section 4
#
#   This module is retained as a no-op for backward compatibility with
#   the orchestrator's source chain. It can be removed entirely once
#   cf-pre-tool-use-security.sh drops the source line.
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security lib directory
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)

set -euo pipefail

# Source shared library (required for consistent state)
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# All hook bypass checks are handled by cf-git-protection.sh
return 0
