#!/usr/bin/env bash
# Purpose:      Shared library for detecting file-reading Bash commands
# Location:     .codeflow/scripts/security/lib/bash-file-readers-lib.sh
# Usage:        source "$REPO_ROOT/.codeflow/scripts/security/lib/bash-file-readers-lib.sh"
# Compatibility: macOS/Linux (bash 3.2+)
# Author:       Claude Code
# Created:      2026-01-18
# Version:      1.0.0
#
# This is a LIBRARY file - meant to be sourced, not executed directly.
#
# Functions:
#   is_file_reading_command <command>  - Returns 0 if command reads files
#   detect_indirect_read <command>     - Alias for is_file_reading_command
#   extract_target_file <command>      - Extracts file path from command
#   get_detection_category <command>   - Returns matched category name
#
# Category functions (for granular detection):
#   is_cat_family <command>            - cat, bat, head, tail, less, more, etc.
#   is_text_processor <command>        - awk, sed, perl, python -c, ruby -e
#   is_search_tool <command>           - grep, rg, ag, find -exec cat
#   is_shell_builtin <command>         - source, ., read <, mapfile
#   is_diff_tool <command>             - diff, cmp, comm, sdiff
#   is_encoding_tool <command>         - base64, xxd, od, hexdump, strings
#   is_archive_reader <command>        - tar -O, zcat, bzcat, unzip -p
#   is_network_fetcher <command>       - curl file://, wget file://
#   is_utility_tool <command>          - dd if=, tee, xargs with readers

# Guard: prevent multiple sourcing
if [[ -n "${_BASH_FILE_READERS_LIB_LOADED:-}" ]]; then
    # shellcheck disable=SC2317  # exit is reachable when script is run directly
    return 0 2>/dev/null || exit 0
fi
readonly _BASH_FILE_READERS_LIB_LOADED=1

# ==============================================================================
# Constants
# ==============================================================================
# shellcheck disable=SC2034  # Version exported for external consumers
readonly BASH_FILE_READERS_LIB_VERSION="1.0.0"

# ==============================================================================
# Category 1: Cat Family
# Direct file content display commands
# ==============================================================================
is_cat_family() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        cat|bat|batcat|head|tail|less|more|most|nl|tac|rev|pr|fold|expand|unexpand|fmt|shuf)
            return 0
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 2: Text Processors
# Commands that process file content (awk, sed, perl, etc.)
# ==============================================================================
is_text_processor() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        awk|gawk|mawk|nawk|sed|gsed|perl|ruby|cut|sort|uniq|paste|join|column)
            return 0
            ;;
        python|python3|python2)
            # Check for -c flag with file operations
            if [[ "$cmd" =~ python[23]?[[:space:]]+-c ]]; then
                if [[ "$cmd" =~ (open\(|read\(|Path\() ]]; then
                    return 0
                fi
            fi
            return 1
            ;;
        php)
            if [[ "$cmd" =~ php[[:space:]]+-r ]]; then
                if [[ "$cmd" =~ (file_get_contents|fopen|fread|readfile) ]]; then
                    return 0
                fi
            fi
            return 1
            ;;
        lua)
            if [[ "$cmd" =~ lua[[:space:]]+-e ]]; then
                if [[ "$cmd" =~ (io\.open|io\.read|io\.lines) ]]; then
                    return 0
                fi
            fi
            return 1
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 3: Search Tools
# Commands that search file contents
# ==============================================================================
is_search_tool() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        grep|egrep|fgrep|rg|ripgrep|ag|ack|ack-grep)
            return 0
            ;;
        find)
            # Check for -exec with file-reading commands
            if [[ "$cmd" =~ -exec[[:space:]]+(cat|head|tail|less|more|grep|awk|sed) ]]; then
                return 0
            fi
            return 1
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 4: Shell Builtins
# Shell commands that read file content
# ==============================================================================
is_shell_builtin() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    # Check for source/dot command
    case "$first_word" in
        source|\.)
            return 0
            ;;
    esac

    # Check for read with redirection
    if [[ "$cmd" =~ read[[:space:]].*\<[[:space:]]*[^[:space:]] ]]; then
        return 0
    fi

    # Check for mapfile/readarray
    if [[ "$cmd" =~ ^(mapfile|readarray)[[:space:]] ]]; then
        return 0
    fi

    # Check for while read loop (input often comes from pipe/redirect)
    if [[ "$cmd" =~ while[[:space:]]+(.*[[:space:]]+)?read[[:space:]] ]]; then
        return 0
    fi

    # Check for while read with redirection
    if [[ "$cmd" =~ while[[:space:]]+read.*\<[[:space:]]*[^[:space:]] ]]; then
        return 0
    fi

    # Check for exec with input redirection
    if [[ "$cmd" =~ exec[[:space:]].*\<[[:space:]]*[^[:space:]] ]]; then
        return 0
    fi

    # Check for input redirection without read command
    if [[ "$cmd" =~ '<'[[:space:]]*[^[:space:]] ]]; then
        return 0
    fi

    # Check for $(<file) pattern
    local subst_pattern
    subst_pattern=$'\\$\\(<[^)]+\\)'
    if [[ "$cmd" =~ $subst_pattern ]]; then
        return 0
    fi

    return 1
}

# ==============================================================================
# Category 5: Diff Tools
# Commands that compare files (require reading)
# ==============================================================================
is_diff_tool() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        diff|diff3|cmp|comm|sdiff|vimdiff|colordiff|delta)
            return 0
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 6: Encoding Tools
# Commands that encode/decode or analyze binary content
# ==============================================================================
is_encoding_tool() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        base64|base32|xxd|od|hexdump|hd|strings|file)
            return 0
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 7: Archive Readers
# Commands that extract/read from compressed files
# ==============================================================================
is_archive_reader() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        zcat|gzcat|bzcat|xzcat|lzcat|zless|bzless|xzless|zstdcat)
            return 0
            ;;
        gunzip)
            # Check for stdout flags
            if [[ "$cmd" =~ gunzip.*(-c|--stdout|--to-stdout) ]]; then
                return 0
            fi
            return 1
            ;;
        tar)
            # Check for extract to stdout flags
            if [[ "$cmd" =~ (-O|--to-stdout|-xOf) ]]; then
                return 0
            fi
            return 1
            ;;
        unzip)
            # Check for pipe to stdout flags
            if [[ "$cmd" =~ (-p|-c) ]]; then
                return 0
            fi
            return 1
            ;;
        7z|7za|7zr)
            # Check for extract to stdout
            if [[ "$cmd" =~ (x[[:space:]]+-so|e[[:space:]]+-so) ]]; then
                return 0
            fi
            return 1
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 8: Network Fetchers with file:// protocol
# Commands that can read local files via file:// URLs
# ==============================================================================
is_network_fetcher() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        curl|wget|fetch)
            # Check for file:// protocol
            if [[ "$cmd" =~ file:// ]]; then
                return 0
            fi
            return 1
            ;;
    esac
    return 1
}

# ==============================================================================
# Category 9: Utility File Readers
# Commands like dd, tee, xargs that read files
# ==============================================================================
is_utility_tool() {
    local cmd="$1"
    local first_word
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    case "$first_word" in
        dd)
            # Check for input file
            if [[ "$cmd" =~ if= ]]; then
                return 0
            fi
            return 1
            ;;
        tee)
            return 0
            ;;
        xargs)
            # Check for xargs with file-reading commands
            if [[ "$cmd" =~ xargs[[:space:]]+(cat|head|tail|less|more|grep|awk|sed) ]]; then
                return 0
            fi
            return 1
            ;;
    esac
    return 1
}

# ==============================================================================
# Path Evasion Detection
# Detect commands run via env, command, or full paths
# ==============================================================================
is_path_evasion() {
    local cmd="$1"
    local first_word normalized_cmd
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    # Check for full path to file-reading commands
    if [[ "$cmd" =~ ^(/bin/|/usr/bin/|/usr/local/bin/)(cat|head|tail|less|more|grep|awk|sed|perl|ruby) ]]; then
        return 0
    fi

    # Check for env wrapper
    if [[ "$first_word" == "env" ]]; then
        # Strip env and any variable assignments, get the actual command
        normalized_cmd=$(echo "$cmd" | sed -E 's/^env[[:space:]]+([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]+[[:space:]]+)*//')
        # Recursively check the actual command
        if is_file_reading_command "$normalized_cmd"; then
            return 0
        fi
    fi

    # Check for command builtin wrapper
    if [[ "$first_word" == "command" ]]; then
        normalized_cmd=$(echo "$cmd" | sed -E 's/^command[[:space:]]+(-[pvV][[:space:]]+)?//')
        if is_file_reading_command "$normalized_cmd"; then
            return 0
        fi
    fi

    return 1
}

# normalize_command - Strip path and wrappers from command
# Arguments:
#   $1 - Command string
# Returns:
#   0 and prints normalized command on stdout
normalize_command() {
    local cmd="$1"
    local first_word

    # Get first word and strip path
    first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')

    # Strip env wrapper
    if [[ "$first_word" == "env" ]]; then
        cmd=$(echo "$cmd" | sed -E 's/^env[[:space:]]+([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]+[[:space:]]+)*//')
        first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')
    fi

    # Strip command wrapper
    if [[ "$first_word" == "command" ]]; then
        cmd=$(echo "$cmd" | sed -E 's/^command[[:space:]]+(-[pvV][[:space:]]+)?//')
        first_word=$(echo "$cmd" | awk '{print $1}' | sed 's|.*/||')
    fi

    echo "$first_word"
    return 0
}

# ==============================================================================
# Combined Detection Functions
# ==============================================================================

# is_file_reading_command - Check if command reads files (any category)
# Arguments:
#   $1 - Command string to analyze
# Returns:
#   0 if command reads files, 1 otherwise
is_file_reading_command() {
    local cmd="$1"
    local subshell_pattern backtick_pattern

    # Skip empty commands
    [[ -z "$cmd" ]] && return 1

    # Check for tee in pipeline (writes to file)
    if [[ "$cmd" =~ \|[[:space:]]*tee[[:space:]] ]]; then
        return 0
    fi

    # Check for xargs with file-reading commands in pipeline
    if [[ "$cmd" =~ \|[[:space:]]*xargs[[:space:]]+(cat|head|tail|less|more|grep|awk|sed) ]]; then
        return 0
    fi

    # Check for command substitution with file-reading commands
    # Pattern 1: $(cat file) - subshell with cat or other readers
    subshell_pattern='\$\(([^)]+)\)'
    if [[ "$cmd" =~ $subshell_pattern ]]; then
        local subshell_cmd="${BASH_REMATCH[1]}"
        # Recursively check if the subshell command reads files
        if is_cat_family "$subshell_cmd" || \
           is_text_processor "$subshell_cmd" || \
           is_search_tool "$subshell_cmd" || \
           is_shell_builtin "$subshell_cmd" || \
           is_diff_tool "$subshell_cmd" || \
           is_encoding_tool "$subshell_cmd" || \
           is_archive_reader "$subshell_cmd" || \
           is_network_fetcher "$subshell_cmd" || \
           is_utility_tool "$subshell_cmd" || \
           is_path_evasion "$subshell_cmd"; then
            return 0
        fi
    fi

    # Pattern 2: `cat file` - backtick command substitution
    # shellcheck disable=SC2016  # Single quotes intentional - literal backtick pattern
    backtick_pattern='`([^`]+)`'
    if [[ "$cmd" =~ $backtick_pattern ]]; then
        local backtick_cmd="${BASH_REMATCH[1]}"
        # Recursively check if the backtick command reads files
        if is_cat_family "$backtick_cmd" || \
           is_text_processor "$backtick_cmd" || \
           is_search_tool "$backtick_cmd" || \
           is_shell_builtin "$backtick_cmd" || \
           is_diff_tool "$backtick_cmd" || \
           is_encoding_tool "$backtick_cmd" || \
           is_archive_reader "$backtick_cmd" || \
           is_network_fetcher "$backtick_cmd" || \
           is_utility_tool "$backtick_cmd" || \
           is_path_evasion "$backtick_cmd"; then
            return 0
        fi
    fi

    # Check all categories for direct commands
    is_cat_family "$cmd" && return 0
    is_text_processor "$cmd" && return 0
    is_search_tool "$cmd" && return 0
    is_shell_builtin "$cmd" && return 0
    is_diff_tool "$cmd" && return 0
    is_encoding_tool "$cmd" && return 0
    is_archive_reader "$cmd" && return 0
    is_network_fetcher "$cmd" && return 0
    is_utility_tool "$cmd" && return 0
    is_path_evasion "$cmd" && return 0

    return 1
}

# detect_indirect_read - Alias for is_file_reading_command
# Arguments:
#   $1 - Command string to analyze
# Returns:
#   0 if command reads files, 1 otherwise
detect_indirect_read() {
    is_file_reading_command "$@"
}

# extract_target_file - Extract file path from command (best effort)
# Arguments:
#   $1 - Command string to analyze
# Returns:
#   0 and prints file path on stdout if found, 1 if not found
# Note: This is heuristic and may not work for all command patterns
extract_target_file() {
    local cmd="$1"

    # Skip empty commands
    [[ -z "$cmd" ]] && return 1

    # Strategy: Look for common file path patterns
    # 1. Quoted paths (single or double quotes)
    # 2. Paths starting with / or . or ~
    # 3. Last argument that looks like a path

    # Try to extract quoted path first
    if [[ "$cmd" =~ [\"\']((/|\.|\~)[^\"\']+)[\"\'] ]]; then
        echo "${BASH_REMATCH[1]}"
        return 0
    fi

    # Try to extract file:// URL path
    if [[ "$cmd" =~ file://(/[^[:space:]\"\']+) ]]; then
        echo "${BASH_REMATCH[1]}"
        return 0
    fi

    # Try to extract path after redirection
    if [[ "$cmd" =~ \<[[:space:]]*([^[:space:]\"\']+) ]]; then
        echo "${BASH_REMATCH[1]}"
        return 0
    fi

    # Try to extract last argument that looks like a path
    local last_arg
    last_arg=$(echo "$cmd" | awk '{print $NF}')

    # Check if this is a command path (full path to a command), not a file path
    if [[ "$last_arg" =~ ^/.*/(cat|head|tail|less|more|grep|awk|sed|find|xargs)$ ]]; then
        # This is a command path, not a file path
        return 1
    fi

    if [[ "$last_arg" =~ ^(/|\./|\.\./|~/) ]]; then
        echo "$last_arg"
        return 0
    fi

    # Try second-to-last for commands with flags at end
    local second_last
    second_last=$(echo "$cmd" | awk '{print $(NF-1)}')

    # Check if this is a command path
    if [[ "$second_last" =~ ^/.*/(cat|head|tail|less|more|grep|awk|sed|find|xargs)$ ]]; then
        return 1
    fi

    if [[ "$second_last" =~ ^(/|\./|\.\./|~/) ]]; then
        echo "$second_last"
        return 0
    fi

    # Could not extract file path
    return 1
}

# ==============================================================================
# Utility Functions
# ==============================================================================

# get_detection_category - Returns which category matched
# Arguments:
#   $1 - Command string to analyze
# Returns:
#   0 and prints category name on stdout if matched, 1 if no match
get_detection_category() {
    local cmd="$1"

    is_cat_family "$cmd" && echo "cat_family" && return 0
    is_text_processor "$cmd" && echo "text_processor" && return 0
    is_search_tool "$cmd" && echo "search_tool" && return 0
    is_shell_builtin "$cmd" && echo "shell_builtin" && return 0
    is_diff_tool "$cmd" && echo "diff_tool" && return 0
    is_encoding_tool "$cmd" && echo "encoding_tool" && return 0
    is_archive_reader "$cmd" && echo "archive_reader" && return 0
    is_network_fetcher "$cmd" && echo "network_fetcher" && return 0
    is_utility_tool "$cmd" && echo "utility_tool" && return 0
    is_path_evasion "$cmd" && echo "path_evasion" && return 0

    return 1
}

# ==============================================================================
# Alias Functions (v2 API compatibility)
# ==============================================================================

# Aliases for clearer naming
is_direct_display_command() { is_cat_family "$@"; }
is_text_processor_command() { is_text_processor "$@"; }
is_binary_encoder_command() { is_encoding_tool "$@"; }
is_archive_reader_command() { is_archive_reader "$@"; }
is_shell_file_reader() { is_shell_builtin "$@"; }
is_interpreter_file_reader() { is_text_processor "$@"; }
is_utility_file_reader() { is_utility_tool "$@"; }
is_path_evasion_attempt() { is_path_evasion "$@"; }

# ==============================================================================
# LIBRARY GUARD
# ==============================================================================
# This file should be sourced, not executed directly
if [[ "${BASH_SOURCE[0]:-}" == "${0:-}" ]]; then
    echo "Error: This is a library file. Source it instead of executing." >&2
    echo "Usage: source \"\$(basename \"$0\")\"" >&2
    exit 1
fi
