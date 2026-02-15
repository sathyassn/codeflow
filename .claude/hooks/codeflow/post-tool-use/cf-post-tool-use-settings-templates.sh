#!/usr/bin/env bash
# Purpose:   Verify settings template consistency after template edits
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-settings-templates.sh
# Hook Type: PostToolUse
# Matcher:   Edit|Write
# Teammate:  cf-security (sync-settings-templates)
#
# Triggers when: Edit/Write to .claude/settings-templates/*.json
#
# Config-Driven:
#   - Templates discovered dynamically from directory
#   - Reference template: first alphabetically (autonomous.json)
#   - Copy mappings defined in enforcement-policy.json
#
# Active Verification:
#   - Compares hooks sections across all templates
#   - Compares _version across all templates
#   - Verifies hook script wiring (no orphaned or broken references)
#   - Reports specific mismatches with evidence
#   - Provides cp commands from config
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (PostToolUse hooks should not block)

set -euo pipefail

readonly VERSION="2.2.0"

# -----------------------------------------------------------------------------
# Configuration (read from enforcement-policy.json)
# -----------------------------------------------------------------------------

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# Config file path (relative to repo root)
readonly CONFIG_FILE=".codeflow/config/enforcement/enforcement-policy.json"

# Load config from enforcement-policy.json
# Falls back to defaults if config not found
load_config() {
    local repo_root="$1"
    local config_path="${repo_root}/${CONFIG_FILE}"

    if [[ -f "$config_path" ]]; then
        # Read from config
        TEMPLATE_DIR_RELATIVE=$(jq -r '.settings_templates.directory // ".claude/settings-templates"' "$config_path")

        # Load copy mappings as array
        COPY_MAPPINGS=()
        while IFS= read -r mapping; do
            COPY_MAPPINGS+=("$mapping")
        done < <(jq -r '.settings_templates.copy_mappings[] | "\(.template):\(.destination)"' "$config_path" 2>/dev/null)

        # Default if no mappings found
        if [[ ${#COPY_MAPPINGS[@]} -eq 0 ]]; then
            COPY_MAPPINGS=("strict.json:.claude/settings.json" "autonomous.json:.claude/settings.local.json")
        fi
    else
        # Fallback defaults
        TEMPLATE_DIR_RELATIVE=".claude/settings-templates"
        COPY_MAPPINGS=("strict.json:.claude/settings.json" "autonomous.json:.claude/settings.local.json")
    fi
}

# Will be set by load_config
TEMPLATE_DIR_RELATIVE=""
declare -a COPY_MAPPINGS=()

# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------

show_help() {
    cat << 'EOF'
Usage: cf-post-tool-use-settings-templates.sh [-h] [-V]

Verify settings template consistency after template edits.
Called by Claude Code PostToolUse hook for Edit|Write operations.

Options:
  -h, --help     Show this help message
  -V, --version  Show version information

Config-Driven:
  - Templates discovered dynamically from directory listing
  - Reference template: first alphabetically
  - Copy mappings configured in enforcement-policy.json

Triggers when Edit/Write targets .claude/settings-templates/*.json
EOF
}

show_version() {
    echo "cf-post-tool-use-settings-templates.sh version $VERSION"
}

# Discover templates dynamically from directory
# Arguments: template_dir
# Outputs: space-separated list of template filenames
discover_templates() {
    local template_dir="$1"
    local templates=""

    # Find all .json files, sort alphabetically
    while IFS= read -r file; do
        local basename_file
        basename_file=$(basename "$file")
        templates="${templates}${basename_file} "
    done < <(find "$template_dir" -maxdepth 1 -name "*.json" -type f 2>/dev/null | sort)

    echo "${templates% }"  # Trim trailing space
}

# Generate cp commands from config
# Outputs: formatted cp command string
generate_cp_commands() {
    local cp_cmd=""
    local first=true

    for mapping in "${COPY_MAPPINGS[@]}"; do
        local template="${mapping%%:*}"
        local dest="${mapping#*:}"

        if [[ "$first" == "true" ]]; then
            cp_cmd="cp ${TEMPLATE_DIR_RELATIVE}/${template} ${dest}"
            first=false
        else
            cp_cmd="${cp_cmd} && \\\\\\n   cp ${TEMPLATE_DIR_RELATIVE}/${template} ${dest}"
        fi
    done

    echo "$cp_cmd"
}

# Verify template consistency
# Arguments: template_dir
# Outputs: Result string with verification details
verify_templates() {
    local template_dir="$1"

    # Discover templates dynamically
    local templates_str
    templates_str=$(discover_templates "$template_dir")

    if [[ -z "$templates_str" ]]; then
        echo "MISSING:no templates found"
        return 1
    fi

    # Convert to array
    local -a templates
    read -ra templates <<< "$templates_str"

    local template_count="${#templates[@]}"
    if [[ "$template_count" -lt 2 ]]; then
        echo "MISSING:need at least 2 templates, found ${template_count}"
        return 1
    fi

    # Extract hooks and versions from all templates
    local -a hooks_hashes=()
    local -a versions=()

    for template in "${templates[@]}"; do
        local file="${template_dir}/${template}"

        # Get sorted hooks section hash for comparison
        local hooks_hash
        hooks_hash=$(jq -S '.hooks' "$file" 2>/dev/null | shasum -a 256 2>/dev/null | cut -d' ' -f1 || echo "error")
        hooks_hashes+=("$hooks_hash")

        # Get version
        local ver
        ver=$(jq -r '._version // "missing"' "$file" 2>/dev/null || echo "error")
        versions+=("$ver")
    done

    # Use first template as reference (alphabetically first)
    local ref_template="${templates[0]}"
    local ref_hooks="${hooks_hashes[0]}"
    local ref_version="${versions[0]}"

    # Check consistency
    local hooks_mismatch=false
    local version_mismatch=false
    local hooks_diff=""
    local version_diff=""

    for i in "${!templates[@]}"; do
        if [[ "${hooks_hashes[$i]}" != "$ref_hooks" ]]; then
            hooks_mismatch=true
            hooks_diff="${hooks_diff}${templates[$i]} "
        fi
        if [[ "${versions[$i]}" != "$ref_version" ]]; then
            version_mismatch=true
            version_diff="${version_diff}${templates[$i]}=${versions[$i]} "
        fi
    done

    # Build result
    if [[ "$hooks_mismatch" == "true" || "$version_mismatch" == "true" ]]; then
        echo "MISMATCH:hooks=${hooks_mismatch}:version=${version_mismatch}:hooks_diff=${hooks_diff}:version_diff=${version_diff}:ref_version=${ref_version}:ref_template=${ref_template}:count=${template_count}"
        return 1
    else
        echo "OK:version=${ref_version}:count=${template_count}"
        return 0
    fi
}

# Parse field from result string
# Arguments: result_string field_name
# Note: Prepends ':' to result to ensure exact field matching
parse_field() {
    local result="$1"
    local field="$2"
    # Prepend : and search for :field= to avoid partial matches
    echo ":${result}" | grep -o ":${field}=[^:]*" | head -1 | cut -d'=' -f2-
}

# Verify hook script wiring
# Arguments: repo_root template_file
# Outputs: Result string with wiring verification details
verify_hook_wiring() {
    local repo_root="$1"
    local template_file="$2"
    local hooks_dir="${repo_root}/.claude/hooks/codeflow"

    # Get list of actual hook scripts
    local existing_list
    existing_list=$(find "$hooks_dir" -name "*.sh" -type f 2>/dev/null | sort | xargs -I{} basename {} 2>/dev/null || true)

    # Get list of referenced scripts from template
    local referenced_list
    referenced_list=$(jq -r '.. | .command? // empty' "$template_file" 2>/dev/null | \
        grep '\.claude/hooks/' | \
        sed -n 's/.*\.claude\/hooks\/[^/]*\/\([^"]*\.sh\).*/\1/p' | \
        sort -u || true)

    # Count existing and referenced
    local existing_count=0
    local referenced_count=0
    [[ -n "$existing_list" ]] && existing_count=$(echo "$existing_list" | wc -l | tr -d ' ')
    [[ -n "$referenced_list" ]] && referenced_count=$(echo "$referenced_list" | wc -l | tr -d ' ')

    # Find orphaned scripts (exist but not referenced)
    local orphaned_list=""
    local orphaned_count=0
    if [[ -n "$existing_list" ]]; then
        for script in $existing_list; do
            if ! echo "$referenced_list" | grep -q "^${script}$"; then
                orphaned_list="${orphaned_list}${script} "
                ((orphaned_count++)) || true
            fi
        done
    fi

    # Find broken references (referenced but don't exist)
    local broken_list=""
    local broken_count=0
    if [[ -n "$referenced_list" ]]; then
        for ref in $referenced_list; do
            # Check in all subdirectories
            if ! find "$hooks_dir" -name "$ref" -type f 2>/dev/null | grep -q .; then
                broken_list="${broken_list}${ref} "
                ((broken_count++)) || true
            fi
        done
    fi

    if [[ "$orphaned_count" -gt 0 || "$broken_count" -gt 0 ]]; then
        echo "WIRING_ISSUE:orphaned_count=${orphaned_count}:broken_count=${broken_count}:orphaned=${orphaned_list}:broken=${broken_list}:existing=${existing_count}:referenced=${referenced_count}"
        return 0  # Don't return 1, let caller decide severity
    else
        echo "WIRING_OK:existing=${existing_count}:referenced=${referenced_count}"
        return 0
    fi
}

# -----------------------------------------------------------------------------
# Main
# -----------------------------------------------------------------------------

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                show_help
                exit 0
                ;;
            -V|--version)
                show_version
                exit 0
                ;;
            *)
                shift
                ;;
        esac
    done

    # Get repo root for config loading
    local repo_root
    repo_root=$(git rev-parse --show-toplevel 2>/dev/null || echo "$REPO_ROOT")

    # Load configuration from enforcement-policy.json
    load_config "$repo_root"

    # Read hook input from stdin
    local input
    input=$(cat)

    local tool_name
    local file_path
    tool_name=$(echo "$input" | jq -r '.tool_name // empty')
    file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

    # Only for Edit/Write tools
    if [[ "$tool_name" != "Edit" && "$tool_name" != "Write" ]]; then
        exit 0
    fi

    # Only for settings-templates directory
    if [[ "$file_path" != *".claude/settings-templates/"* ]]; then
        exit 0
    fi

    # Only for .json files
    if [[ "$file_path" != *.json ]]; then
        exit 0
    fi

    # Extract template name and directory
    local template_name
    template_name=$(basename "$file_path")

    # Get template directory (handle both absolute and relative paths)
    local template_dir
    if [[ "$file_path" == /* ]]; then
        template_dir=$(dirname "$file_path")
    else
        template_dir="$repo_root/$TEMPLATE_DIR_RELATIVE"
    fi

    # Verify template consistency
    local verify_result
    verify_result=$(verify_templates "$template_dir" 2>/dev/null || echo "ERROR")

    # Verify hook script wiring (use first template as reference)
    local wiring_result=""
    local first_template
    first_template=$(find "$template_dir" -maxdepth 1 -name "*.json" -type f 2>/dev/null | sort | head -1)
    if [[ -n "$first_template" ]]; then
        wiring_result=$(verify_hook_wiring "$repo_root" "$first_template" 2>/dev/null || echo "WIRING_ERROR")
    fi

    local output_message=""
    local cp_commands
    cp_commands=$(generate_cp_commands)

    if [[ "$verify_result" == ERROR* || "$verify_result" == MISSING* ]]; then
        # Could not verify (missing templates or error)
        local error_detail="${verify_result#*:}"
        output_message="SETTINGS TEMPLATE EDITED: ${template_name}\\n\\n"
        output_message="${output_message}VERIFICATION INCOMPLETE: ${error_detail}\\n\\n"
        output_message="${output_message}MANDATORY: Ensure all templates exist before proceeding"

    elif [[ "$verify_result" == MISMATCH* ]]; then
        # Parse mismatch details using helper function
        local hooks_mismatch version_mismatch hooks_diff version_diff ref_version ref_template template_count
        hooks_mismatch=$(parse_field "$verify_result" "hooks")
        version_mismatch=$(parse_field "$verify_result" "version")
        hooks_diff=$(parse_field "$verify_result" "hooks_diff")
        version_diff=$(parse_field "$verify_result" "version_diff")
        ref_version=$(parse_field "$verify_result" "ref_version")
        ref_template=$(parse_field "$verify_result" "ref_template")
        template_count=$(parse_field "$verify_result" "count")

        output_message="TEMPLATE SYNC REQUIRED: ${template_name}\\n\\n"

        if [[ "$hooks_mismatch" == "true" ]]; then
            output_message="${output_message}HOOKS SECTION MISMATCH DETECTED\\n"
            output_message="${output_message}   Differing: ${hooks_diff}\\n"
            output_message="${output_message}   Reference: ${ref_template}\\n\\n"
            output_message="${output_message}   STOP: Hooks MUST be identical across all ${template_count} templates\\n"
            output_message="${output_message}   -> Copy hooks from ${ref_template} to all other templates\\n\\n"
        fi

        if [[ "$version_mismatch" == "true" ]]; then
            output_message="${output_message}VERSION MISMATCH DETECTED\\n"
            output_message="${output_message}   Reference (${ref_template}): ${ref_version}\\n"
            output_message="${output_message}   Differing: ${version_diff}\\n\\n"
            output_message="${output_message}   STOP: _version MUST be identical across all ${template_count} templates\\n\\n"
        fi

        output_message="${output_message}FIX BEFORE PROCEEDING:\\n"
        output_message="${output_message}1. Sync all differing sections\\n"
        output_message="${output_message}2. Verify consistency\\n"
        output_message="${output_message}3. Then provide cp commands to user\\n\\n"
        output_message="${output_message}-> Delegate to cf-security teammate: SendMessage(recipient=\"cf-security\", content=\"sync-settings-templates\")"

    else
        # All consistent - provide cp commands
        local current_version template_count
        current_version=$(parse_field "$verify_result" "version")
        template_count=$(parse_field "$verify_result" "count")

        output_message="TEMPLATE SYNC VERIFIED: ${template_name}\\n\\n"
        output_message="${output_message}Hooks sections: IDENTICAL across all ${template_count} templates\\n"
        output_message="${output_message}Version: ${current_version} (all templates match)\\n"

        # Add wiring verification results
        if [[ "$wiring_result" == WIRING_OK* ]]; then
            local existing_count referenced_count
            existing_count=$(parse_field "$wiring_result" "existing")
            referenced_count=$(parse_field "$wiring_result" "referenced")
            output_message="${output_message}Hook wiring: ${referenced_count} scripts referenced, ${existing_count} exist\\n\\n"
        elif [[ "$wiring_result" == WIRING_ISSUE* ]]; then
            local orphaned_count broken_count orphaned_list broken_list
            orphaned_count=$(parse_field "$wiring_result" "orphaned_count")
            broken_count=$(parse_field "$wiring_result" "broken_count")
            orphaned_list=$(parse_field "$wiring_result" "orphaned")
            broken_list=$(parse_field "$wiring_result" "broken")
            output_message="${output_message}\\nHOOK WIRING ISSUES DETECTED:\\n"
            if [[ "$orphaned_count" -gt 0 ]]; then
                output_message="${output_message}   Orphaned (exist but not referenced): ${orphaned_list}\\n"
            fi
            if [[ "$broken_count" -gt 0 ]]; then
                output_message="${output_message}   Broken (referenced but missing): ${broken_list}\\n"
            fi
            output_message="${output_message}\\n"
        else
            output_message="${output_message}\\n"
        fi

        output_message="${output_message}REMAINING STEP: Provide cp commands to user:\\n\\n"
        output_message="${output_message}   ${cp_commands}\\n\\n"
        output_message="${output_message}FORBIDDEN: Completing without providing cp commands"
    fi

    # Output JSON response
    cat <<EOF
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "${output_message}"
  }
}
EOF

    exit 0
}

main "$@"
