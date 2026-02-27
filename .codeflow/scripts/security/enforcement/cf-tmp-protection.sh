#!/usr/bin/env bash
# Purpose:   Managed tmp protection checks (Section 11)
# Location:  .codeflow/scripts/security/enforcement/cf-tmp-protection.sh
# Usage:     source "cf-tmp-protection.sh" (from main hook)
#
# This module handles:
#   - Section 11: Managed Tmp Protection
#     - Protects /tmp/claude/${CF_PROJECT_ROOT}/managed/ folder structure
#     - Blocks deletion of managed folders (config-driven via enforcement-policy.json)
#     - Blocks deletion of state files (but allows create/edit)
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security-lib.sh
#   - MANAGED_TMP_FOLDERS: Array of managed tmp folders
#   - STATE_FOLDER: Path to state folder
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_with_skill, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null  # LIB_DIR set by caller
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# SECTION 11: Managed Tmp Protection
# =============================================================================
# Purpose: Protect the managed tmp structure from accidental deletion
# Structure (config-driven, defaults shown):
#   /tmp/claude/{project}/managed/               - Container (protected from deletion)
#   /tmp/claude/{project}/managed/protected-edits/ - For protected resource workflow

# Block deletion/rename of managed folders
for folder in "${MANAGED_TMP_FOLDERS[@]}"; do
  # Check if command targets the folder itself (not contents)
  if [[ "$COMMAND" =~ (rm|rmdir|mv)[[:space:]]+((-[a-zA-Z]+[[:space:]]+)*)"$folder"($|[[:space:]]) ]]; then
    block_with_skill \
      "Managed Tmp Protection" \
      "Cannot delete/rename managed folder" \
      "$folder" \
      "security" \
      "diagnose-permission-error"
  fi
  # Also catch rm -rf targeting the folder
  if [[ "$COMMAND" =~ rm[[:space:]]+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*[[:space:]]+"$folder"($|[[:space:]]) ]] || \
     [[ "$COMMAND" =~ rm[[:space:]]+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*[[:space:]]+"$folder"($|[[:space:]]) ]]; then
    block_with_skill \
      "Managed Tmp Protection" \
      "Cannot delete managed folder recursively" \
      "$folder" \
      "security" \
      "diagnose-permission-error"
  fi
done

# Block deletion of state files (but allow creation and editing)
if [[ "$COMMAND" =~ (rm|unlink)[[:space:]]+((-[a-zA-Z]+[[:space:]]+)*)"$STATE_FOLDER/" ]]; then
  block_with_skill \
    "State File Protection" \
    "State files protected from deletion. User can rm manually if needed." \
    "${STATE_FOLDER}/*" \
    "security" \
    "diagnose-permission-error"
fi

# All tmp protection checks passed
return 0
