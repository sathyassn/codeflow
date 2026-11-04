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
    // Recursive search checks the same policy patterns for many names. Bound
    // the cache because this utility also accepts caller-provided patterns.
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Regex>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(entries) = cache.lock() {
        if let Some(regex) = entries.get(pattern) {
            return regex.is_match(path);
        }
    }
    let regex = glob_to_regex(pattern);
    let full_regex = format!("(?s)^{regex}$");
    let Ok(regex) = Regex::new(&full_regex) else {
        return false;
    };
    if let Ok(mut entries) = cache.lock() {
        if entries.len() < 128 {
            entries.insert(pattern.to_owned(), regex.clone());
        }
    }
    regex.is_match(path)
}

/// Whether two glob languages share a path. This is a product walk over
/// epsilon NFAs, so a secret need not exist on disk to intersect a glob.
/// Unsupported syntax or a bound returns `None`; security callers fail closed.
pub(crate) fn glob_intersects(left: &str, right: &str) -> Option<bool> {
    let left = glob_tokens(left)?;
    let right = glob_tokens(right)?;
    let mut pending = vec![(0, 0)];
    let mut seen = std::collections::HashSet::new();
    while let Some((a, b)) = pending.pop() {
        if !seen.insert((a, b)) {
            continue;
        }
        if seen.len() > 65_536 {
            return None;
        }
        if a == left.len() && b == right.len() {
            return Some(true);
        }
        for (tokens, at, other, swap) in [(&left, a, b, false), (&right, b, a, true)] {
            if let Some(token) = tokens.get(at).filter(|t| t.repeat) {
                let next = at + 1;
                pending.push(if swap { (other, next) } else { (next, other) });
                if token.optional_slash {
                    let next = at + 3;
                    pending.push(if swap { (other, next) } else { (next, other) });
                }
            }
        }
        if let (Some(x), Some(y)) = (left.get(a), right.get(b)) {
            if x.ranges.iter().any(|&(lo, hi)| {
                y.ranges
                    .iter()
                    .any(|&(other_lo, other_hi)| lo <= other_hi && other_lo <= hi)
            }) {
                pending.push((a + usize::from(!x.repeat), b + usize::from(!y.repeat)));
            }
        }
    }
    Some(false)
}

struct GlobToken {
    ranges: Vec<(u32, u32)>,
    repeat: bool,
    optional_slash: bool,
}

fn glob_tokens(pattern: &str) -> Option<Vec<GlobToken>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        if tokens.len() >= 256 {
            return None;
        }
        at += 1;
        let mut repeat = false;
        let ranges = match ch {
            '*' => {
                repeat = true;
                let recursive = chars.get(at) == Some(&'*');
                if recursive {
                    at += 1;
                    if chars.get(at) == Some(&'/') {
                        tokens.push(GlobToken {
                            ranges: Vec::new(),
                            repeat: true,
                            optional_slash: true,
                        });
                    }
                    vec![(1, 0x0010_ffff)]
                } else {
                    non_slash()
                }
            }
            '?' => non_slash(),
            '[' => bracket_ranges(&chars, &mut at)?,
            '\\' => {
                let literal = u32::from(*chars.get(at)?);
                at += 1;
                vec![(literal, literal)]
            }
            '(' | ')' | '|' => return None,
            literal => vec![(u32::from(literal), u32::from(literal))],
        };
        tokens.push(GlobToken {
            ranges,
            repeat,
            optional_slash: false,
        });
    }
    Some(tokens)
}

fn bracket_ranges(chars: &[char], at: &mut usize) -> Option<Vec<(u32, u32)>> {
    let mut next = *at;
    let negate = matches!(chars.get(next), Some('!' | '^'));
    next += usize::from(negate);
    let mut ranges = Vec::new();
    while let Some(&first) = chars.get(next) {
        if first == ']' && !ranges.is_empty() {
            break;
        }
        // POSIX named classes and nested syntax are not guessed.
        if first == '[' || first == '\\' {
            return None;
        }
        next += 1;
        let last = if chars.get(next) == Some(&'-') && chars.get(next + 1) != Some(&']') {
            next += 2;
            *chars.get(next - 1)?
        } else {
            first
        };
        if first > last {
            return None;
        }
        ranges.push((u32::from(first), u32::from(last)));
    }
    if chars.get(next) != Some(&']') || ranges.is_empty() {
        return None;
    }
    next += 1;
    ranges.sort_unstable();
    let ranges = if negate {
        let mut complement = Vec::new();
        let mut start = 1;
        for (lo, hi) in ranges {
            if start < lo {
                complement.push((start, lo - 1));
            }
            start = start.max(hi + 1);
        }
        if start <= 0x0010_ffff {
            complement.push((start, 0x0010_ffff));
        }
        complement
    } else {
        ranges
    };
    *at = next;
    // A bracket expression never consumes a path separator.
    Some(
        ranges
            .into_iter()
            .flat_map(|(lo, hi)| {
                non_slash().into_iter().filter_map(move |(start, end)| {
                    (lo <= end && start <= hi).then_some((lo.max(start), hi.min(end)))
                })
            })
            .collect(),
    )
}

fn non_slash() -> Vec<(u32, u32)> {
    vec![(1, u32::from('/') - 1), (u32::from('/') + 1, 0x0010_ffff)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_intersection_matches_complete_secret_names() {
        for (left, right, expected) in [
            ("[i]d_rs?", "**/id_rsa*", true),
            ("id_*", "**/id_ed25519*", true),
            ("*.pem", "**/*.pem", true),
            ("*.rs", "**/*.pem", false),
            ("src/*.rs", "**/id_rsa*", true),
            ("src/*.rs", "~/.ssh/**", false),
            ("~/.ss[h]/config", "~/.ssh/**", true),
            ("dir\nname/id_rsa", "**/id_rsa*", true),
            ("fooid_rsa", "**/id_rsa*", false),
            ("id_rsa", "**/id_rsa*", true),
            ("a/b/id_rsa", "**/id_rsa*", true),
            ("[!a-c].pem", "b.pem", false),
            ("[!a-c].pem", "d.pem", true),
        ] {
            assert_eq!(
                glob_intersects(left, right),
                Some(expected),
                "{left} / {right}"
            );
            assert_eq!(
                glob_intersects(right, left),
                Some(expected),
                "symmetry: {left} / {right}"
            );
        }
        assert_eq!(glob_intersects("[[:alpha:]]", "id_rsa"), None);
        assert_eq!(glob_intersects(&"?".repeat(257), "id_rsa"), None);
        assert!(matches_extended_glob("dir\nname/id_rsa", "**/id_rsa*"));
    }

    /// The ordinary glob library supplies an independent language oracle for
    /// bounded strings, including newlines and zero-directory globstars.
    #[test]
    fn glob_intersection_agrees_with_literal_matching() {
        let patterns = ["a*", "[ab]?", "a/**/b", "**/b", "[!a]*", "*/b"];
        let mut words = vec![String::new()];
        for _ in 0..4 {
            let previous = words.clone();
            for prefix in previous {
                for ch in ['a', 'b', '/', '\n'] {
                    words.push(format!("{prefix}{ch}"));
                }
            }
        }
        words.sort();
        words.dedup();
        for pattern in patterns {
            let oracle = glob::Pattern::new(pattern).unwrap();
            for word in &words {
                let expected = oracle.matches_with(
                    word,
                    glob::MatchOptions {
                        require_literal_separator: true,
                        ..glob::MatchOptions::new()
                    },
                );
                assert_eq!(
                    glob_intersects(pattern, word),
                    Some(expected),
                    "{pattern}: {word:?}"
                );
            }
        }
    }

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
