//! Pattern matching utilities for security modules.
//!
//! Glob-to-regex conversion and path boundary checking, used by the
//! git-guard and the scaffold state.

use std::sync::OnceLock;

use regex::Regex;

fn tmp_claude_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/tmp/claude\S*").expect("valid"))
}

/// Convert a glob pattern to a Rust regex pattern.
///
/// Handles `**` (recursive), `*` (single segment), `?` (single char).
/// A whole `**/` component matches zero or more directories.
#[must_use]
pub fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::new();
    let mut segments = pattern.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segment == "**" && segments.peek().is_some() {
            // Only a whole recursive directory component can be absent.
            regex.push_str("(?:.*/)?");
        } else {
            let escaped = regex::escape(segment)
                .replace(r"\*\*", ".*")
                .replace(r"\*", "[^/]*")
                .replace(r"\?", ".");
            regex.push_str(&escaped);
            if segments.peek().is_some() {
                regex.push('/');
            }
        }
    }
    regex
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

/// Match a path against an extended glob pattern supporting `**`.
#[must_use]
pub fn matches_extended_glob(path: &str, pattern: &str) -> bool {
    let regex = glob_to_regex(pattern);
    let full_regex = format!("^{regex}$");
    Regex::new(&full_regex).is_ok_and(|re| re.is_match(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_directory_glob_includes_zero_components() {
        for path in ["id_rsa", "./id_rsa", "keys/id_rsa", "/home/test/id_rsa"] {
            assert!(matches_extended_glob(path, "**/id_rsa*"), "{path}");
        }
        for path in ["root/file", "root/a/file", "root/a/b/file"] {
            assert!(matches_extended_glob(path, "root/**/file"), "{path}");
        }
        for (path, pattern) in [
            ("id_rsa", "keys/id_rsa*"),
            ("rootfile", "root/**/file"),
            ("rootish/file", "root/**/file"),
            ("file", "root/**/file"),
            ("notes/id_rsa", "*/keys/id_rsa*"),
            (".ssh-notes/key", ".ssh/**"),
            ("abfile", "ab**/file"),
        ] {
            assert!(!matches_extended_glob(path, pattern), "{path}: {pattern}");
        }
    }

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
    fn test_matches_extended_glob() {
        assert!(matches_extended_glob(".claude/hooks/test.sh", ".claude/**"));
        assert!(matches_extended_glob("test.json", "*.json"));
        assert!(!matches_extended_glob("test.rs", "*.json"));
    }
}
