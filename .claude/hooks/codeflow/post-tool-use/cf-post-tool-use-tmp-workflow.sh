#!/usr/bin/env bash
# Purpose:   Guide tmp file workflow after editing
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-tmp-workflow.sh
# Hook Type: PostToolUse
# Matcher:   Edit|Write
# Skill:     security-management
# Version:   1.2.0
#
# Changelog:
#   - 1.2.0: Added file type detection for validation guidance
#   - 1.1.0: Adapted for codeflow project
#   - 1.0.0: Initial from workflow project
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (PostToolUse hooks should not block)

set -euo pipefail

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")

# Only for protected-edits tmp files in managed area
if [[ "$FILE_PATH" != /tmp/claude/managed/codeflow/protected-edits/* ]]; then
  exit 0
fi

# Check if this is a settings file edit
IS_SETTINGS_FILE="false"
if [[ "$FILE_PATH" == *"settings.json"* ]] || [[ "$FILE_PATH" == *"settings.local.json"* ]]; then
  IS_SETTINGS_FILE="true"
fi

# Detect file type for validation guidance
detect_file_type() {
  local path="$1"
  local ext="${path##*.}"

  case "$ext" in
    sh|bash)
      echo "shell"
      ;;
    py)
      echo "python"
      ;;
    json)
      echo "json"
      ;;
    yaml|yml)
      echo "yaml"
      ;;
    toml)
      echo "toml"
      ;;
    *)
      # Check shebang for shell scripts without extension
      if [[ -f "$path" ]] && head -1 "$path" 2>/dev/null | grep -qE '^#!.*/(ba)?sh'; then
        echo "shell"
      else
        echo "unknown"
      fi
      ;;
  esac
}

# Get validation command for file type
get_validation_hint() {
  local file_type="$1"

  case "$file_type" in
    shell)
      echo "Validate: shellcheck {file} or bash -n {file}"
      ;;
    python)
      echo "Validate: python3 -m py_compile {file} or ruff check {file}"
      ;;
    json)
      echo "Validate: jq . {file} or python3 -m json.tool {file}"
      ;;
    yaml)
      echo "Validate: python3 -c \"import yaml; yaml.safe_load(open('{file}'))\""
      ;;
    toml)
      echo "Validate: python3 -c \"import tomllib; tomllib.load(open('{file}', 'rb'))\""
      ;;
    *)
      echo ""
      ;;
  esac
}

FILE_TYPE=$(detect_file_type "$FILE_PATH")
VALIDATION_HINT=$(get_validation_hint "$FILE_TYPE")

if [[ "$IS_SETTINGS_FILE" == "true" ]]; then
  cat <<'EOF'
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "SETTINGS FILE EDITED\n\n(1) Show user the changes\n(2) Validate: jq . {file}\n(3) Provide: sudo cp /tmp/claude/managed/codeflow/protected-edits/{path} {original}\n(4) After user confirms, READ original to verify\n(5) MANDATORY: Skill('cf-security-management', args='sync-settings-templates')\n(6) Cleanup: rm /tmp/claude/managed/codeflow/protected-edits/{file}\n\nFORBIDDEN: Completing without sync-settings-templates for settings files"
  }
}
EOF
elif [[ -n "$VALIDATION_HINT" ]]; then
  # Include file-type specific validation hint
  cat <<EOF
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "PROTECTED RESOURCE WORKFLOW [${FILE_TYPE}]\n\n(1) Show user the changes\n(2) ${VALIDATION_HINT}\n(3) Provide: sudo cp /tmp/claude/managed/codeflow/protected-edits/{path} {original}\n(4) After user confirms, READ original to verify\n(5) Cleanup: rm /tmp/claude/managed/codeflow/protected-edits/{file} (targeted, not entire folder)"
  }
}
EOF
else
  cat <<'EOF'
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "PROTECTED RESOURCE WORKFLOW\n\n(1) Show user the changes\n(2) Provide: sudo cp /tmp/claude/managed/codeflow/protected-edits/{path} {original}\n(3) After user confirms, READ original to verify\n(4) Cleanup: rm /tmp/claude/managed/codeflow/protected-edits/{file} (targeted, not entire folder)"
  }
}
EOF
fi

exit 0
