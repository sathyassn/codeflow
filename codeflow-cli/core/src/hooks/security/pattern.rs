//! Pattern matching utilities for security modules.
//!
//! Glob-to-regex conversion, path boundary checking, command segment
//! splitting, and helper functions shared across security modules.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

fn tmp_claude_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/tmp/claude\S*").expect("valid"))
}

/// Convert a glob pattern to a Rust regex pattern.
///
/// Handles `**` (recursive), `*` (single segment), `?` (single char).
#[must_use]
pub fn glob_to_regex(pattern: &str) -> String {
    let escaped = regex::escape(pattern);
    // Order matters: handle ** before * since escape turns * to \*
    let escaped = escaped.replace(r"\*\*", ".*");
    let escaped = escaped.replace(r"\*", "[^/]*");
    escaped.replace(r"\?", ".")
}

/// Strip `/tmp/claude...` path segments from a command to prevent false
/// positives when operations target safe scratch space.
#[must_use]
pub fn strip_tmp_claude_paths(cmd: &str) -> String {
    tmp_claude_re().replace_all(cmd, "").to_string()
}

/// Check if a path appears in a command with proper boundaries.
/// Prevents `.claude` from matching `.claude-notes.md`.
/// Also strips `/tmp/claude` paths to avoid false positives.
#[must_use]
pub fn is_path_targeted(cmd: &str, path: &str) -> bool {
    if !cmd.contains(path) {
        return false;
    }

    let cleaned = strip_tmp_claude_paths(cmd);
    if !cleaned.contains(path) {
        return false;
    }

    let mut idx = 0;
    while idx < cmd.len() {
        if let Some(pos) = cmd[idx..].find(path) {
            let abs_pos = idx + pos;
            let end = abs_pos + path.len();

            if end >= cmd.len() {
                return true;
            }

            let next = cmd.as_bytes()[end];
            if next == b'/' || next == b' ' || next == b'"' || next == b'\'' {
                return true;
            }

            idx = abs_pos + 1;
        } else {
            break;
        }
    }

    false
}

/// Check if a command targets a path matching a glob pattern.
#[must_use]
pub fn is_glob_path_targeted(cmd: &str, pattern: &str) -> bool {
    let cleaned = strip_tmp_claude_paths(cmd);
    if let Ok(re) = Regex::new(&glob_to_regex(pattern)) {
        if re.is_match(&cleaned) {
            return true;
        }
    }

    // For /** patterns, also match the directory itself.
    if let Some(dir_path) = pattern.strip_suffix("/**") {
        return is_path_targeted(cmd, dir_path);
    }

    false
}

/// Unified check for both exact paths and glob patterns.
#[must_use]
pub fn is_path_or_glob_targeted(cmd: &str, path: &str) -> bool {
    if path.contains('*') || path.contains('?') {
        is_glob_path_targeted(cmd, path)
    } else {
        is_path_targeted(cmd, path)
    }
}

/// Returns `true` if the path contains glob characters.
#[must_use]
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains('*') || path.contains('?')
}

/// Match a path against an extended glob pattern supporting `**`.
#[must_use]
pub fn matches_extended_glob(path: &str, pattern: &str) -> bool {
    let regex = glob_to_regex(pattern);
    let full_regex = format!("^{regex}$");
    Regex::new(&full_regex).is_ok_and(|re| re.is_match(path))
}

/// Extract the flags portion of a `git commit` command (before `-m`/`--message`)
/// to prevent false positives from commit messages.
#[must_use]
pub fn get_flags_portion(cmd: &str) -> &str {
    for sep in &[" --message=", " --message ", " -m\"", " -m'", " -m "] {
        if let Some(idx) = cmd.find(sep) {
            return &cmd[..idx];
        }
    }
    cmd
}

/// Strip heredoc bodies from a command string.
///
/// Replaces the content between `<<DELIM\n...\nDELIM` (or `<<'DELIM'`, `<<"DELIM"`,
/// `<<-DELIM`) with an empty string, preserving the command portion that precedes
/// the heredoc. This prevents heredoc content (documentation, comments) from
/// triggering false-positive path pattern matches.
#[must_use]
pub fn strip_heredoc_content(cmd: &str) -> String {
    static HEREDOC_START_RE: OnceLock<Regex> = OnceLock::new();
    let re = HEREDOC_START_RE.get_or_init(|| {
        Regex::new(r#"<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?"#).expect("valid regex")
    });

    let mut result = String::new();
    let mut remaining = cmd;

    while let Some(m) = re.find(remaining) {
        let caps = re.captures(&remaining[m.start()..]).unwrap();
        let delimiter = caps.get(1).unwrap().as_str();

        // Add everything before the heredoc marker.
        result.push_str(&remaining[..m.end()]);

        let after_marker = &remaining[m.end()..];

        // Find the delimiter on its own line.
        let delim_pattern = format!("\n{delimiter}");
        if let Some(end_pos) = after_marker.find(&delim_pattern) {
            // Skip the heredoc body, keep the delimiter line.
            remaining = &after_marker[end_pos + delim_pattern.len()..];
            result.push('\n');
            result.push_str(delimiter);
        } else {
            // No closing delimiter found — keep the rest as-is.
            result.push_str(after_marker);
            return result;
        }
    }

    result.push_str(remaining);
    result
}

/// Split a compound command into segments by `&&`, `||`, `;`, `|`
/// while respecting quoted strings.
///
/// Heredoc content is stripped before segmentation to prevent false-positive
/// matches on documentation or comments mentioning protected paths.
#[must_use]
pub fn split_command_segments(cmd: &str) -> Vec<String> {
    let cleaned = strip_heredoc_content(cmd);
    split_command_segments_inner(&cleaned)
}

/// Inner segmentation logic operating on a cleaned command string.
fn split_command_segments_inner(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut segment = String::new();
    let bytes = cmd.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        // Track quote state.
        if ch == b'\'' && !in_double {
            in_single = !in_single;
            segment.push(ch as char);
            i += 1;
            continue;
        }
        if ch == b'"' && !in_single {
            in_double = !in_double;
            segment.push(ch as char);
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            // Check for && or ||.
            if i + 1 < bytes.len() {
                if ch == b'&' && bytes[i + 1] == b'&' {
                    segments.push(segment.clone());
                    segment.clear();
                    i += 2;
                    continue;
                }
                if ch == b'|' && bytes[i + 1] == b'|' {
                    segments.push(segment.clone());
                    segment.clear();
                    i += 2;
                    continue;
                }
            }
            // Semicolon.
            if ch == b';' {
                segments.push(segment.clone());
                segment.clear();
                i += 1;
                continue;
            }
            // Single pipe (not ||).
            if ch == b'|' {
                segments.push(segment.clone());
                segment.clear();
                i += 1;
                continue;
            }
        }

        segment.push(ch as char);
        i += 1;
    }

    if !segment.is_empty() {
        segments.push(segment);
    }

    segments
}

/// Extract variable assignments (`VAR=value`) from command segments.
///
/// Handles unquoted, single-quoted, and double-quoted values.
/// Returns a map of variable name to assigned value (quotes stripped).
///
/// # Panics
///
/// Panics if the internal regex fails to compile (should never happen).
#[must_use]
pub fn extract_variable_assignments(segments: &[String]) -> HashMap<String, String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?:^|\s)([A-Za-z_][A-Za-z0-9_]*)=("([^"]*)"|'([^']*)'|(\S*))"#)
            .expect("valid")
    });

    let mut map = HashMap::new();
    for seg in segments {
        for caps in re.captures_iter(seg) {
            let var = caps.get(1).unwrap().as_str().to_string();
            // Group 3 = double-quoted, 4 = single-quoted, 5 = unquoted.
            let val = caps
                .get(3)
                .or_else(|| caps.get(4))
                .or_else(|| caps.get(5))
                .map_or(String::new(), |m| m.as_str().to_string());
            map.insert(var, val);
        }
    }
    map
}

/// Detect shell variable indirection (`$VAR` or `${VAR}`) in a command,
/// excluding special variables (`$?`, `$!`, `$$`, `$0`-`$9`, `$@`, `$*`, `$#`).
///
/// # Panics
///
/// Panics if the internal regex fails to compile (should never happen).
#[must_use]
pub fn has_variable_indirection(cmd: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Match $VAR or ${VAR} where VAR starts with a letter or underscore.
        Regex::new(r"\$\{?[A-Za-z_][A-Za-z0-9_]*\}?").expect("valid")
    });
    re.is_match(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_to_regex_star() {
        let re = glob_to_regex("*.json");
        assert_eq!(re, "[^/]*\\.json");
    }

    #[test]
    fn test_glob_to_regex_double_star() {
        let re = glob_to_regex(".claude/**");
        assert_eq!(re, "\\.claude/.*");
    }

    #[test]
    fn test_glob_to_regex_question() {
        let re = glob_to_regex("test?.sh");
        assert_eq!(re, "test.\\.sh");
    }

    #[test]
    fn test_strip_tmp_claude_paths() {
        let result = strip_tmp_claude_paths("cp /tmp/claude/test .claude/settings.json");
        assert!(result.contains(".claude/settings.json"));
        assert!(!result.contains("/tmp/claude"));
    }

    #[test]
    fn test_is_path_targeted_basic() {
        assert!(is_path_targeted(
            "rm .claude/settings.json",
            ".claude/settings.json"
        ));
        assert!(!is_path_targeted("rm .claude-notes.md", ".claude"));
    }

    #[test]
    fn test_is_path_targeted_boundary() {
        assert!(is_path_targeted("rm .claude/foo", ".claude"));
        assert!(is_path_targeted("rm '.claude'", ".claude"));
        assert!(is_path_targeted("rm \".claude\"", ".claude"));
    }

    #[test]
    fn test_is_path_targeted_tmp_claude_false_positive() {
        // Should NOT match when only /tmp/claude contains the path.
        assert!(!is_path_targeted(
            "cp /tmp/claude/.claude/foo bar",
            ".claude"
        ));
    }

    #[test]
    fn test_is_glob_path_targeted() {
        assert!(is_glob_path_targeted(
            "rm .claude/hooks/codeflow/test.sh",
            ".claude/hooks/codeflow/**"
        ));
    }

    #[test]
    fn test_is_glob_path_targeted_dir_suffix() {
        // /** patterns also match the directory itself.
        assert!(is_glob_path_targeted(
            "rm .claude/hooks/codeflow",
            ".claude/hooks/codeflow/**"
        ));
    }

    #[test]
    fn test_is_path_or_glob_targeted() {
        assert!(is_path_or_glob_targeted(
            "rm .claude/settings.json",
            ".claude/settings.json"
        ));
        assert!(is_path_or_glob_targeted(
            "rm .github/workflows/ci.yml",
            ".github/**"
        ));
    }

    #[test]
    fn test_is_glob_pattern() {
        assert!(is_glob_pattern("*.json"));
        assert!(is_glob_pattern("test/**"));
        assert!(is_glob_pattern("test?.sh"));
        assert!(!is_glob_pattern("test.json"));
    }

    #[test]
    fn test_matches_extended_glob() {
        assert!(matches_extended_glob(".claude/hooks/test.sh", ".claude/**"));
        assert!(matches_extended_glob("test.json", "*.json"));
        assert!(!matches_extended_glob("test.rs", "*.json"));
    }

    #[test]
    fn test_get_flags_portion() {
        assert_eq!(
            get_flags_portion("git commit -n -m 'test'"),
            "git commit -n"
        );
        assert_eq!(
            get_flags_portion("git commit --message='test'"),
            "git commit"
        );
        assert_eq!(
            get_flags_portion("git commit -am 'test'"),
            "git commit -am 'test'"
        );
    }

    #[test]
    fn test_split_command_segments() {
        let segments = split_command_segments("echo a && echo b; echo c | cat");
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].trim(), "echo a");
        assert_eq!(segments[1].trim(), "echo b");
        assert_eq!(segments[2].trim(), "echo c");
        assert_eq!(segments[3].trim(), "cat");
    }

    #[test]
    fn test_split_command_segments_quoted() {
        let segments = split_command_segments("echo 'a && b' && echo c");
        assert_eq!(segments.len(), 2);
        assert!(segments[0].contains("a && b"));
    }

    #[test]
    fn test_split_command_segments_or() {
        let segments = split_command_segments("cmd1 || cmd2");
        assert_eq!(segments.len(), 2);
    }

    // -- heredoc stripping --

    #[test]
    fn test_strip_heredoc_removes_body() {
        let cmd = "cat <<EOF\ncodeflow-cli/core/src/hooks/session_start.rs\nEOF";
        let stripped = strip_heredoc_content(cmd);
        assert!(!stripped.contains("session_start.rs"), "heredoc body should be stripped");
        assert!(stripped.contains("cat <<EOF"), "command prefix should be preserved");
    }

    #[test]
    fn test_strip_heredoc_quoted_delimiter() {
        let cmd = "cat <<'EOF'\ncodeflow-cli/core/src/hooks/session_start.rs\nEOF";
        let stripped = strip_heredoc_content(cmd);
        assert!(!stripped.contains("session_start.rs"));
    }

    #[test]
    fn test_strip_heredoc_preserves_non_heredoc() {
        let cmd = "echo hello && ls -la";
        let stripped = strip_heredoc_content(cmd);
        assert_eq!(stripped, cmd);
    }

    #[test]
    fn test_split_segments_heredoc_no_false_positive() {
        let cmd = "cat <<'EOF'\ncodeflow-cli/core/src/hooks/session_start.rs\nEOF && echo done";
        let segments = split_command_segments(cmd);
        // The heredoc body should not appear in any segment.
        for seg in &segments {
            assert!(
                !seg.contains("session_start.rs"),
                "heredoc content should not appear in segments: {seg}"
            );
        }
    }

    // -- extract_variable_assignments --

    #[test]
    fn test_extract_var_unquoted() {
        let segs = vec!["F=.claude/settings.json".to_string()];
        let map = extract_variable_assignments(&segs);
        assert_eq!(map.get("F").unwrap(), ".claude/settings.json");
    }

    #[test]
    fn test_extract_var_double_quoted() {
        let segs = vec![r#"F=".claude/settings.json""#.to_string()];
        let map = extract_variable_assignments(&segs);
        assert_eq!(map.get("F").unwrap(), ".claude/settings.json");
    }

    #[test]
    fn test_extract_var_single_quoted() {
        let segs = vec!["F='.claude/settings.json'".to_string()];
        let map = extract_variable_assignments(&segs);
        assert_eq!(map.get("F").unwrap(), ".claude/settings.json");
    }

    #[test]
    fn test_extract_var_multiple_segments() {
        let segs = vec![
            "A=/tmp/safe".to_string(),
            "B=.claude/hooks/codeflow/test.sh".to_string(),
        ];
        let map = extract_variable_assignments(&segs);
        assert_eq!(map.get("A").unwrap(), "/tmp/safe");
        assert_eq!(map.get("B").unwrap(), ".claude/hooks/codeflow/test.sh");
    }

    #[test]
    fn test_extract_var_no_assignment() {
        let segs = vec!["echo hello".to_string()];
        let map = extract_variable_assignments(&segs);
        assert!(map.is_empty());
    }

    // -- has_variable_indirection --

    #[test]
    fn test_has_var_indirection_dollar_var() {
        assert!(has_variable_indirection("rm $F"));
    }

    #[test]
    fn test_has_var_indirection_braced() {
        assert!(has_variable_indirection("rm ${DIR}"));
    }

    #[test]
    fn test_has_var_indirection_special_vars_excluded() {
        // Special vars ($?, $!, $$, $0-$9, $@, $*, $#) are not matched.
        assert!(!has_variable_indirection("echo $?"));
        assert!(!has_variable_indirection("echo $!"));
        assert!(!has_variable_indirection("echo $$"));
        assert!(!has_variable_indirection("echo $0"));
        assert!(!has_variable_indirection("echo $9"));
        assert!(!has_variable_indirection("echo $@"));
        assert!(!has_variable_indirection("echo $*"));
        assert!(!has_variable_indirection("echo $#"));
    }

    #[test]
    fn test_has_var_indirection_no_vars() {
        assert!(!has_variable_indirection("ls -la"));
    }

    #[test]
    fn test_has_var_indirection_path_env() {
        assert!(has_variable_indirection("echo $PATH"));
    }
}
