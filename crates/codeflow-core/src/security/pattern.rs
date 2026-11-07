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

/// Most compiled glob patterns kept at once.
pub(crate) const GLOB_CACHE_ENTRIES: usize = codeflow_limits::GLOB_CACHE_ENTRIES.default_count();

/// Longest glob pattern the matcher compiles. Shipped policy patterns are far
/// shorter; a longer one is refused, never compiled.
pub const MAX_GLOB_PATTERN_BYTES: usize = codeflow_limits::GLOB_PATTERN_BYTES.default_count();
/// Largest compiled program, in bytes, for one glob. Compile work grows with
/// this size, so it is the step bound for compilation.
const GLOB_COMPILED_BYTES: usize = codeflow_limits::GLOB_COMPILED_BYTES.default_count();

/// The cause a guard names when a path is past [`codeflow_limits::GLOB_PATH_BYTES`].
pub const PATH_PAST_BOUND: &str = "a path is longer than the glob matcher's bound";

/// The result of matching a path against a glob under the bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobMatch {
    /// The path is in the glob's language.
    Match,
    /// The path is not in the glob's language.
    NoMatch,
    /// The match could not finish inside the bound; the cause is named.
    /// A guard treats this as a refusal, never as an allow.
    Unbounded(&'static str),
}

impl GlobMatch {
    /// Whether a guard that must fail closed refuses on this result.
    #[must_use]
    pub fn refuses(self) -> bool {
        !matches!(self, Self::NoMatch)
    }
}

/// Compiled glob patterns, bounded to [`GLOB_CACHE_ENTRIES`]. An instance is
/// separate state, so a test can fill its own cache without touching the
/// process-wide one.
#[derive(Default)]
struct GlobCache {
    /// Shared, not cloned: a `Regex` clone builds a new search-cache pool, so
    /// a cached hit would pay that cost on every call.
    entries: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Regex>>>,
    /// Regex compiles made through this cache; tests read it.
    #[cfg(test)]
    compiles: std::sync::atomic::AtomicUsize,
}

impl GlobCache {
    fn matching(&self, path: &str, pattern: &str) -> GlobMatch {
        if pattern.len() > MAX_GLOB_PATTERN_BYTES {
            return GlobMatch::Unbounded("a glob pattern is longer than the matcher's bound");
        }
        if path.len() > codeflow_limits::GLOB_PATH_BYTES.count() {
            return GlobMatch::Unbounded(PATH_PAST_BOUND);
        }
        let found = self
            .entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(pattern).cloned());
        let regex = if let Some(regex) = found {
            regex
        } else {
            #[cfg(test)]
            self.compiles
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let full_regex = format!("(?s)^{}$", glob_to_regex(pattern));
            let regex = match regex::RegexBuilder::new(&full_regex)
                .size_limit(GLOB_COMPILED_BYTES)
                .dfa_size_limit(GLOB_COMPILED_BYTES)
                .build()
            {
                Ok(regex) => std::sync::Arc::new(regex),
                Err(regex::Error::CompiledTooBig(_)) => {
                    return GlobMatch::Unbounded(
                        "a glob pattern compiles past the matcher's bound",
                    );
                }
                Err(_) => return GlobMatch::Unbounded("a glob pattern cannot be compiled"),
            };
            if let Ok(mut entries) = self.entries.lock() {
                // A full cache is emptied, not frozen: a frozen cache made
                // every later pattern compile again on each call (TSK-1258).
                if entries.len() >= GLOB_CACHE_ENTRIES {
                    entries.clear();
                }
                entries.insert(pattern.to_owned(), regex.clone());
            }
            regex
        };
        if regex.is_match(path) {
            GlobMatch::Match
        } else {
            GlobMatch::NoMatch
        }
    }
}

/// Match a path against an extended glob pattern supporting `**`, inside a
/// fixed bound on pattern length, compiled size and path length. A guard
/// refuses on [`GlobMatch::refuses`]; no caller reads a bound hit as "no
/// match", which is why there is no boolean form.
#[must_use]
pub fn match_extended_glob(path: &str, pattern: &str) -> GlobMatch {
    // Recursive search checks the same policy patterns for many names. Bound
    // the cache because this utility also accepts caller-provided patterns.
    static CACHE: OnceLock<GlobCache> = OnceLock::new();
    CACHE
        .get_or_init(GlobCache::default)
        .matching(path, pattern)
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

    /// A boolean view for the tests: a match, with no bound hit.
    fn matches_extended_glob(path: &str, pattern: &str) -> bool {
        match_extended_glob(path, pattern) == GlobMatch::Match
    }

    /// What a guard does: refuse on a match or on a bound hit.
    fn glob_covers_or_refuses(path: &str, pattern: &str) -> bool {
        match_extended_glob(path, pattern).refuses()
    }

    /// TSK-1258 AC-1: once a cache holds 128 patterns, a pattern used again
    /// and again must still compile once, not on every call. Before the fix
    /// every call after the cache filled recompiled. The cache is this
    /// test's own instance, so parallel tests cannot change the count.
    #[test]
    fn full_glob_cache_does_not_recompile_a_repeated_pattern() {
        use std::sync::atomic::Ordering::Relaxed;
        let cache = GlobCache::default();
        for n in 0..300 {
            let _ = cache.matching("x", &format!("tsk1258-fill-{n}/**/*.never"));
        }
        let before = cache.compiles.load(Relaxed);
        for _ in 0..50 {
            assert_eq!(
                cache.matching("a/b.pem", "tsk1258-late/**/*.pem"),
                GlobMatch::NoMatch
            );
            assert_eq!(
                cache.matching("tsk1258-late/a/b.pem", "tsk1258-late/**/*.pem"),
                GlobMatch::Match
            );
        }
        let compiles = cache.compiles.load(Relaxed) - before;
        assert_eq!(
            compiles, 1,
            "{compiles} compiles in 100 calls with a full cache"
        );
    }

    /// TSK-1258 AC-2: a pattern or path past the bound is refused with a
    /// named cause by a guard, and never reads as "no match".
    #[test]
    fn glob_past_the_bound_refuses_with_a_cause() {
        let long_pattern = "a".repeat(MAX_GLOB_PATTERN_BYTES + 1);
        let long_path = "a".repeat(codeflow_limits::GLOB_PATH_BYTES.count() + 1);
        for (path, pattern) in [("a", long_pattern.as_str()), (long_path.as_str(), "**/a")] {
            let result = match_extended_glob(path, pattern);
            assert!(matches!(result, GlobMatch::Unbounded(cause) if !cause.is_empty()));
            assert!(glob_covers_or_refuses(path, pattern));
            assert!(!matches_extended_glob(path, pattern));
        }
        assert!(!glob_covers_or_refuses("b", "a"));
        assert!(glob_covers_or_refuses("a", "a"));
    }

    /// TSK-1258 AC-2: nested and repeated wildcards finish inside the bound,
    /// and a path the pattern covers is never reported as uncovered.
    #[test]
    fn nested_wildcards_stay_bounded_and_never_allow() {
        let nested = format!("{}z", "**/*a*".repeat(150));
        let path = format!("{}z", "xa/".repeat(150));
        assert!(nested.len() <= MAX_GLOB_PATTERN_BYTES);
        let started = std::time::Instant::now();
        let covered = glob_covers_or_refuses(&path, &nested);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(covered, "a covered path must not read as uncovered");
    }

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
