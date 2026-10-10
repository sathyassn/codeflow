//! Ruleset ref matching as GitHub does it: Ruby `File.fnmatch` with
//! `FNM_PATHNAME` and no other flag (GitHub's "Creating rulesets for a
//! repository", "Using fnmatch syntax").
//!
//! The grammar, ported from Ruby's `dir.c`:
//!
//! - `*` matches any run of characters within one path segment and `?`
//!   one character; neither crosses `/`.
//! - None of `*`, `?` or a class matches a `.` that starts a segment
//!   (no `FNM_DOTMATCH`).
//! - `[abc]`, `[a-z]` and `[!abc]` match one character of a segment.
//! - `**/` at the start of a segment matches zero or more whole segments.
//!   Any other `**` is a plain `*`: a trailing `refs/heads/**` matches
//!   `refs/heads/main` and not `refs/heads/feature/foo`, and the glued
//!   `qa**/**/*` matches `qa/foo` and `qa/foo/bar`.
//! - Braces are literal (no `FNM_EXTGLOB`); `\` quotes the next character.
//!
//! A pattern whose meaning is not settled is refused instead of guessed:
//! a `[^...]` complement, which GitHub documents as unsupported, an empty
//! or unterminated class, a `/` inside a class, and a trailing `\`.

/// Whether the ref `string` matches the ruleset ref `pattern`, or why the
/// pattern cannot be read.
pub(super) fn matches(pattern: &str, string: &str) -> Result<bool, String> {
    check(pattern)?;
    let pattern: Vec<char> = pattern.chars().collect();
    let string: Vec<char> = string.chars().collect();
    Ok(pathname(&pattern, &string))
}

/// Refuse a pattern whose meaning differs between fnmatch engines or that
/// GitHub documents as unsupported.
fn check(pattern: &str) -> Result<(), String> {
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    return Err("ends in a lone backslash".to_string());
                }
            }
            '[' => {
                match chars.peek() {
                    Some('^') => {
                        return Err(
                            "uses a [^...] complement, which GitHub does not support".to_string()
                        );
                    }
                    Some('!') => {
                        chars.next();
                    }
                    _ => {}
                }
                if chars.peek() == Some(&']') {
                    return Err("has an empty character class".to_string());
                }
                loop {
                    match chars.next() {
                        None => return Err("has an unterminated character class".to_string()),
                        Some(']') => break,
                        Some('/') => return Err("has a / inside a character class".to_string()),
                        Some('\\') => {
                            if chars.next().is_none() {
                                return Err("has an unterminated character class".to_string());
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether `i` is at the end of a segment of `v`.
fn at_end(v: &[char], i: usize) -> bool {
    v.get(i).is_none_or(|&c| c == '/')
}

/// The index of the character `\` at `i` quotes, else `i`.
fn unescape(p: &[char], i: usize) -> usize {
    if p.get(i) == Some(&'\\') {
        i + 1
    } else {
        i
    }
}

/// Ruby's `fnmatch` under `FNM_PATHNAME`: segment by segment, with `**/`
/// retrying the rest of the pattern one segment further on.
fn pathname(p: &[char], s: &[char]) -> bool {
    let is_recursive = |i: usize| p.get(i..i + 3) == Some(&['*', '*', '/'][..]);
    let (mut pi, mut si) = (0, 0);
    let mut recursion: Option<(usize, usize)> = None;
    loop {
        if is_recursive(pi) {
            while is_recursive(pi) {
                pi += 3;
            }
            recursion = Some((pi, si));
        }
        if let Some((next_p, next_s)) = segment(p, pi, s, si) {
            pi = next_p;
            si = next_s;
            while si < s.len() && s[si] != '/' {
                si += 1;
            }
            if pi < p.len() && si < s.len() {
                pi += 1;
                si += 1;
                continue;
            }
            if pi >= p.len() && si >= s.len() {
                return true;
            }
        }
        // Let the latest `**/` take one more segment, never a dot one.
        if let Some((rest, from)) = recursion {
            if s.get(from) != Some(&'.') {
                let mut next = from;
                while next < s.len() && s[next] != '/' {
                    next += 1;
                }
                if next < s.len() {
                    next += 1;
                    recursion = Some((rest, next));
                    pi = rest;
                    si = next;
                    continue;
                }
            }
        }
        return false;
    }
}

/// Ruby's `fnmatch_helper`: match one segment of `p` from `pi` against
/// `s` from `si`. On a match, returns where each stopped; the pattern
/// stops at its segment end and the string may stop short of its own
/// (after a trailing `*`).
fn segment(p: &[char], mut pi: usize, s: &[char], mut si: usize) -> Option<(usize, usize)> {
    if s.get(si) == Some(&'.') && p.get(unescape(p, pi)) != Some(&'.') {
        return None;
    }
    let mut star: Option<(usize, usize)> = None;
    loop {
        match p.get(pi) {
            Some('*') => {
                while p.get(pi) == Some(&'*') {
                    pi += 1;
                }
                if at_end(p, unescape(p, pi)) {
                    return Some((unescape(p, pi), si));
                }
                if at_end(s, si) {
                    return None;
                }
                star = Some((pi, si));
                continue;
            }
            Some('?') => {
                if at_end(s, si) {
                    return None;
                }
                pi += 1;
                si += 1;
                continue;
            }
            Some('[') => {
                if at_end(s, si) {
                    return None;
                }
                if let Some(next) = bracket(p, pi + 1, s[si]) {
                    pi = next;
                    si += 1;
                    continue;
                }
            }
            _ => {
                let q = unescape(p, pi);
                if at_end(s, si) {
                    return at_end(p, q).then_some((q, si));
                }
                if !at_end(p, q) && p[q] == s[si] {
                    pi = q + 1;
                    si += 1;
                    continue;
                }
            }
        }
        // Failed here: let the last `*` take one more character.
        let (rest, from) = star?;
        star = Some((rest, from + 1));
        pi = rest;
        si = from + 1;
    }
}

/// Ruby's `bracket`: whether the class that opens before `i` matches `c`.
/// Returns the index after its closing `]`.
fn bracket(p: &[char], mut i: usize, c: char) -> Option<usize> {
    let negated = matches!(p.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let mut ok = false;
    while p.get(i) != Some(&']') {
        let t1 = unescape(p, i);
        let low = *p.get(t1)?;
        i = t1 + 1;
        if i >= p.len() {
            return None;
        }
        if p[i] == '-' && p.get(i + 1) != Some(&']') {
            let t2 = unescape(p, i + 1);
            let high = *p.get(t2)?;
            i = t2 + 1;
            if !ok && (c == low || c == high || (low <= c && c <= high)) {
                ok = true;
            }
        } else if !ok && c == low {
            ok = true;
        }
    }
    (ok != negated).then_some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::matches;

    /// Measured with Ruby 4.0.1 `File.fnmatch(pattern, ref,
    /// File::FNM_PATHNAME)` on 2026-10-08. The rows marked "glob" are
    /// where `glob` 0.3 with a literal separator answered otherwise (the
    /// round 3 review of PR 127).
    const RUBY: &[(&str, &str, bool)] = &[
        ("refs/heads/*", "refs/heads/main", true),
        ("refs/heads/*", "refs/heads/feature/foo", false),
        ("refs/heads/*", "refs/heads/.hidden", false), // glob: true
        ("refs/heads/*", "refs/heads/release/*", false),
        ("refs/heads/**/*", "refs/heads/main", true),
        ("refs/heads/**/*", "refs/heads/feature/foo", true),
        ("refs/heads/**/*", "refs/heads/release/1.2", true),
        ("refs/heads/**/*", "refs/heads/release/*", true),
        ("refs/heads/qa/**/*", "refs/heads/qa/foo", true),
        ("refs/heads/qa/**/*", "refs/heads/qa/foo/bar", true),
        ("refs/heads/qa/**/*", "refs/heads/qa", false),
        ("refs/heads/m?in", "refs/heads/main", true),
        ("refs/heads/m?in", "refs/heads/m/in", false),
        ("refs/heads/[!m]*", "refs/heads/main", false),
        ("refs/heads/[!m]*", "refs/heads/develop", true),
        ("refs/heads/[a-m]ain", "refs/heads/main", true),
        ("refs/heads/[a-l]ain", "refs/heads/main", false),
        ("refs/heads/{main,master}", "refs/heads/main", false),
        ("refs/heads/{main,master}", "refs/heads/{main,master}", true),
        ("refs/heads/**", "refs/heads/main", true),
        ("refs/heads/**", "refs/heads/feature/foo", false), // glob: true
        ("refs/heads/**", "refs/heads/release/1.2", false), // glob: true
        ("refs/heads/**", "refs/heads/release/*", false),   // glob: true
        ("refs/heads/release/**", "refs/heads/release/1.2", true),
        (
            "refs/heads/release/**",
            "refs/heads/release/1.2/hotfix",
            false,
        ), // glob: true
        ("refs/heads/a/**", "refs/heads/a/b", true),
        ("refs/heads/a/**", "refs/heads/a/b/c", false), // glob: true
        ("refs/heads/qa**/**/*", "refs/heads/qa/foo", true), // glob: no compile
        ("refs/heads/qa**/**/*", "refs/heads/qa/foo/bar", true), // glob: no compile
        ("refs/heads/qa**/**/*", "refs/heads/qaz/foo", true), // glob: no compile
        ("refs/heads/qa**/**/*", "refs/heads/qa", false),
        ("refs/heads/**/**/x", "refs/heads/a/b/x", true),
        ("refs/heads/**/.x", "refs/heads/a/.x", true),
        ("refs/heads/**/x", "refs/heads/.a/x", false),
        ("refs/heads/*/x", "refs/heads/a/x", true),
        ("refs/*/main", "refs/heads/main", true),
        ("**/main", "refs/heads/main", true),
        ("refs/heads/release/*", "refs/heads/release/*", true),
        ("refs/heads/release/*", "refs/heads/main", false),
        ("refs/heads/ma*", "refs/heads/main", true),
        ("refs/heads/ma", "refs/heads/main", false),
        ("refs/tags/*", "refs/heads/main", false),
        ("refs/heads/*o*", "refs/heads/foo", true),
        ("refs/heads/a*b*c", "refs/heads/aXbYc", true),
        ("refs/heads/a*b*c", "refs/heads/aXbY/c", false),
    ];

    #[test]
    fn matches_ruby_fnmatch_under_pathname() {
        for &(pattern, reference, expected) in RUBY {
            assert_eq!(
                matches(pattern, reference),
                Ok(expected),
                "{pattern} against {reference}"
            );
        }
    }

    #[test]
    fn a_pattern_with_an_unsettled_meaning_is_refused() {
        for pattern in [
            "refs/heads/[^m]*",
            "refs/heads/[ma",
            "refs/heads/[]]",
            "refs/heads/[!]",
            "refs/heads/[a/b]",
            "refs/heads/main\\",
        ] {
            assert!(
                matches(pattern, "refs/heads/main").is_err(),
                "{pattern} was accepted"
            );
        }
    }

    #[test]
    fn a_quoted_character_is_literal_and_a_late_caret_is_a_member() {
        assert_eq!(matches("refs/heads/m\\ain", "refs/heads/main"), Ok(true));
        assert_eq!(matches("refs/heads/[a^]", "refs/heads/^"), Ok(true));
        assert_eq!(matches("refs/heads/[z-a]", "refs/heads/z"), Ok(true));
    }
}
