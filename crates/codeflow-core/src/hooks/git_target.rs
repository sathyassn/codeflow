//! Which repository a git command in a shell line targets (TSK-112).
//!
//! The git-guard judges a git op against the repository it runs in. A `cd`,
//! a `-C`, a `--git-dir`, a `GIT_DIR=` prefix, or a path held in a shell
//! variable moves that repository away from the session's. This module
//! follows those moves, but only where the shell's behavior is certain: a
//! *flat* line of simple commands joined by `;`, `&&` or newlines, with no
//! subshell, group, pipe, background job, substitution, heredoc or control
//! keyword. There, each `cd` and assignment runs in the one top-level shell
//! in order, so the target can be computed. Anywhere else the guard keeps
//! its earlier reading and adds candidates, never removes them, so an
//! unresolved target is never judged more permissively than before
//! (SPC-013 planning resolution 13).

use std::collections::HashMap;

/// How a top-level simple command is joined to the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Join {
    /// The first command of the line.
    Start,
    /// After `;` or a newline: it runs whatever the previous command did.
    Seq,
    /// After `&&`: it runs only when the previous command succeeded.
    And,
}

/// Words that open shell control structures, whose bodies run conditionally
/// or repeatedly. A line using one is not flat.
const CONTROL_WORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "case", "esac",
    "select", "function", "coproc",
];

/// The joins of a flat command line's top-level simple commands, one per
/// non-empty command in order, or `None` when the line is not flat. Single
/// quotes holding a `$` also make a line not flat: the guard's words are
/// quote-stripped, so a literal `'$R'` would otherwise read as a variable.
#[allow(clippy::too_many_lines)] // one character scanner
pub(super) fn flat_joins(command: &str) -> Option<Vec<Join>> {
    let chars: Vec<char> = command.chars().collect();
    let mut joins = Vec::new();
    let mut pending = Join::Start;
    let mut cur = String::new();
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => {
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                    }
                    i += 2;
                    continue;
                }
                '`' => return None,
                '$' if next == Some('(') => return None,
                _ => {}
            }
            cur.push(c);
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                let close = chars[i + 1..].iter().position(|&ch| ch == '\'')? + i + 1;
                if chars[i + 1..close].contains(&'$') {
                    return None;
                }
                cur.extend(&chars[i..=close]);
                i = close + 1;
            }
            '"' => {
                in_double = true;
                cur.push(c);
                i += 1;
            }
            '\\' => {
                if next == Some('\n') {
                    cur.push(' ');
                } else {
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                    }
                }
                i += 2;
            }
            '#' if cur.is_empty() || cur.ends_with(char::is_whitespace) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\n' | ';' => {
                close_segment(&mut cur, &mut joins, &mut pending, Join::Seq)?;
                i += 1;
            }
            '&' if next == Some('&') => {
                close_segment(&mut cur, &mut joins, &mut pending, Join::And)?;
                i += 2;
            }
            // `>&` / `&>` redirections stay with their command.
            '&' if next == Some('>') || cur.ends_with('>') => {
                cur.push(c);
                i += 1;
            }
            '|' if cur.ends_with('>') => {
                cur.push(c);
                i += 1;
            }
            '$' if next == Some('{') => {
                let close = chars[i + 2..].iter().position(|&ch| ch == '}')? + i + 2;
                if chars[i + 2..close]
                    .iter()
                    .any(|ch| matches!(ch, '(' | '`' | '{'))
                {
                    return None;
                }
                cur.extend(&chars[i..=close]);
                i = close + 1;
            }
            '<' if matches!(next, Some('<' | '(')) => return None,
            '>' | '$' if next == Some('(') => return None,
            // Background jobs, pipes, `||`, subshells, groups, substitutions.
            '&' | '|' | '(' | ')' | '{' | '}' | '`' => return None,
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    if in_double {
        return None;
    }
    close_segment(&mut cur, &mut joins, &mut pending, Join::Seq)?;
    Some(joins)
}

/// End the current top-level command. An empty one (`;;`, a trailing `;`)
/// records nothing, and the next command keeps the weaker join. `None` when
/// the command opens a control structure.
fn close_segment(
    cur: &mut String,
    joins: &mut Vec<Join>,
    pending: &mut Join,
    next: Join,
) -> Option<()> {
    let text = std::mem::take(cur);
    let Some(first) = text.split_whitespace().next() else {
        if *pending != Join::Start {
            *pending = Join::Seq;
        }
        return Some(());
    };
    if CONTROL_WORDS.contains(&first) {
        return None;
    }
    joins.push(*pending);
    *pending = next;
    Some(())
}

/// A shell value the guard tracks: known text, or unknown with the reason.
#[derive(Clone, Debug)]
pub(super) enum Val {
    Known(String),
    Unknown(String),
}

/// Expand the `$NAME` and `${NAME}` references in a quote-stripped word from
/// the tracked variables. `Err` names what could not be expanded: an unset or
/// unknown variable, a special parameter, `~`, a glob, or a value that word
/// splitting or globbing could change.
pub(super) fn expand_word(word: &str, vars: &HashMap<String, Val>) -> Result<String, String> {
    if word.starts_with('~') {
        return Err("`~`".to_string());
    }
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '*' | '?' | '[') {
            return Err(format!("the pattern in `{word}`"));
        }
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        let (name, end) = if chars.get(i + 1) == Some(&'{') {
            let close = chars[i + 2..]
                .iter()
                .position(|&ch| ch == '}')
                .map(|p| p + i + 2)
                .ok_or_else(|| format!("`{word}`"))?;
            (chars[i + 2..close].iter().collect::<String>(), close + 1)
        } else {
            let len = chars[i + 1..]
                .iter()
                .take_while(|ch| ch.is_ascii_alphanumeric() || **ch == '_')
                .count();
            (chars[i + 1..=i + len].iter().collect(), i + 1 + len)
        };
        if !is_name(&name) {
            return Err(format!("`{word}`"));
        }
        match vars.get(&name) {
            Some(Val::Known(v))
                if !v.contains(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '?' | '[')) =>
            {
                out.push_str(v);
            }
            _ => return Err(format!("`${name}`")),
        }
        i = end;
    }
    Ok(out)
}

/// `true` for a shell variable name.
pub(super) fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Join `next` onto the directory expression `base` ("" is the session's
/// working directory). An absolute `next` replaces the base; an empty one
/// changes nothing, as `git -C ""` does.
pub(super) fn join_path(base: &str, next: &str) -> String {
    if next.is_empty() {
        base.to_string()
    } else if next.starts_with('/') || base.is_empty() {
        next.to_string()
    } else {
        format!("{}/{next}", base.trim_end_matches('/'))
    }
}

/// Environment variables that move where git reads and writes refs. A shell
/// line that sets one in a way the guard cannot scope leaves the target
/// unresolved.
pub(super) const GIT_LOCATION_VARS: &[&str] = &["GIT_DIR", "GIT_COMMON_DIR"];

/// Builtins that change shell variables in ways the tracker does not model.
const VAR_WRITERS: &[&str] = &[
    "unset",
    "read",
    "declare",
    "typeset",
    "local",
    "readonly",
    "mapfile",
    "readarray",
    "getopts",
    "let",
];

/// The top-level shell state a flat line builds up, segment by segment.
pub(super) struct ShellState {
    joins: Option<Vec<Join>>,
    /// The working directory, as a path expression ("" is the session's).
    pub(super) cwd: Val,
    pub(super) vars: HashMap<String, Val>,
    /// `true` while the current command runs only if an earlier `&&` member
    /// succeeded.
    conditional: bool,
    /// Changes made conditionally in the current and-list: after the list
    /// ends, whether they happened is unknown.
    cond_cwd: bool,
    cond_vars: Vec<String>,
}

impl ShellState {
    /// State for a line whose segments carry `joins`, or `None` when the line
    /// is not flat (nothing is tracked then).
    pub(super) fn new(joins: Option<Vec<Join>>) -> Self {
        Self {
            joins,
            cwd: Val::Known(String::new()),
            vars: HashMap::new(),
            conditional: false,
            cond_cwd: false,
            cond_vars: Vec::new(),
        }
    }

    /// `true` when the line is flat and the state is tracked.
    pub(super) fn flat(&self) -> bool {
        self.joins.is_some()
    }

    /// Enter segment `idx`: settle the previous and-list when a new one starts.
    pub(super) fn begin(&mut self, idx: usize) {
        let Some(join) = self.joins.as_ref().and_then(|j| j.get(idx)).copied() else {
            return;
        };
        match join {
            Join::Start => self.conditional = false,
            Join::Seq => {
                if std::mem::take(&mut self.cond_cwd) {
                    self.cwd = Val::Unknown(
                        "a `cd` that runs only when an earlier command succeeds".to_string(),
                    );
                }
                for name in std::mem::take(&mut self.cond_vars) {
                    let why = format!("`${name}`, set only when an earlier command succeeds");
                    self.vars.insert(name, Val::Unknown(why));
                }
                self.conditional = false;
            }
            Join::And => self.conditional = true,
        }
    }

    /// A `cd <dir>`.
    pub(super) fn cd(&mut self, dir: &str) {
        if !self.flat() {
            return;
        }
        self.cwd = match (expand_word(dir, &self.vars), &self.cwd) {
            (Ok(d), Val::Known(base)) => Val::Known(join_path(base, &d)),
            (Ok(d), Val::Unknown(_)) if d.starts_with('/') => Val::Known(d),
            (Ok(_), Val::Unknown(why)) => Val::Unknown(why.clone()),
            (Err(why), _) => Val::Unknown(why),
        };
        if self.conditional {
            self.cond_cwd = true;
        }
    }

    /// Record the variables a segment sets: standalone `NAME=value` words or
    /// an `export`. `program` is the segment's program after its leading
    /// assignments, `None` when it has none (the assignments then set shell
    /// variables rather than one command's environment).
    pub(super) fn assign(&mut self, tokens: &[String], program: Option<&str>) {
        if !self.flat() {
            return;
        }
        let words: &[String] = match program {
            None => tokens,
            Some("export") if tokens.first().is_some_and(|t| t == "export") => {
                if tokens[1..].iter().any(|t| t.starts_with('-')) {
                    self.forget_vars("an `export` with options");
                    return;
                }
                &tokens[1..]
            }
            Some(_) => {
                // `NAME+=x` reads as a program word; it still changes NAME.
                if let Some(name) = tokens
                    .first()
                    .and_then(|t| t.split_once("+="))
                    .map(|(n, _)| n)
                    .filter(|n| is_name(n))
                {
                    self.set_var(name, Val::Unknown(format!("`${name}`, appended to")));
                }
                return;
            }
        };
        for word in words {
            let Some((name, value)) = word.split_once('=') else {
                continue;
            };
            if !is_name(name) {
                continue;
            }
            let val = match expand_word(value, &self.vars) {
                Ok(v) => Val::Known(v),
                Err(why) => Val::Unknown(why),
            };
            self.set_var(name, val);
        }
    }

    fn set_var(&mut self, name: &str, val: Val) {
        self.vars.insert(name.to_string(), val);
        if self.conditional {
            self.cond_vars.push(name.to_string());
        }
    }

    fn forget_vars(&mut self, why: &str) {
        for val in self.vars.values_mut() {
            *val = Val::Unknown(format!("a variable changed by {why}"));
        }
    }

    /// Account for a program that may change the shell's state untracked.
    pub(super) fn observe(&mut self, program: &str, args: &[String]) {
        if !self.flat() {
            return;
        }
        match program {
            // `cd` forms the tracker cannot follow (`cd`, `cd -`, `builtin cd`).
            "cd" | "pushd" | "popd" | "chdir" => {
                self.cwd = Val::Unknown(format!("the directory after `{program}`"));
            }
            "source" | "." | "eval" => {
                self.cwd = Val::Unknown(format!("the directory after `{program}`"));
                self.forget_vars(&format!("`{program}`"));
            }
            "printf" if args.iter().any(|a| a == "-v") => self.forget_vars("`printf -v`"),
            p if VAR_WRITERS.contains(&p) => self.forget_vars(&format!("`{p}`")),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flat_joins_reads_sequences_and_and_lists() {
        assert_eq!(
            flat_joins("R=/x; cd a && git commit -m 'm'\ngit status"),
            Some(vec![Join::Start, Join::Seq, Join::And, Join::Seq])
        );
        assert_eq!(
            flat_joins("git log 2>&1 >/dev/null"),
            Some(vec![Join::Start])
        );
        assert_eq!(
            flat_joins("cd \"${R}\" && git commit # done"),
            Some(vec![Join::Start, Join::And])
        );
        assert_eq!(flat_joins(";; git status;"), Some(vec![Join::Start]));
    }

    #[test]
    fn test_flat_joins_rejects_what_scopes_or_conditions_state() {
        for cmd in [
            "(cd a); git commit",
            "{ cd a; }; git commit",
            "cd a | git commit",
            "false || cd a; git commit",
            "cd a & git commit",
            "git commit -m \"$(date)\"",
            "git commit -m `date`",
            "git commit -F - <<EOF\nx\nEOF",
            "if true; then cd a; fi; git commit",
            "for d in a; do cd $d; done",
            "R=/x; git -C '$R' commit",
            "git -C \"unterminated",
            "diff <(git show a) b",
        ] {
            assert_eq!(flat_joins(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn test_expand_word_resolves_only_known_plain_values() {
        let mut vars = HashMap::new();
        vars.insert("R".to_string(), Val::Known("/repo".to_string()));
        vars.insert("S".to_string(), Val::Known("/a b".to_string()));
        vars.insert("U".to_string(), Val::Unknown("u".to_string()));
        assert_eq!(expand_word("$R/sub", &vars).unwrap(), "/repo/sub");
        assert_eq!(expand_word("${R}x", &vars).unwrap(), "/repox");
        for word in ["$S", "$U", "$MISSING", "$1", "~/x", "/a/*", "${R:-/y}"] {
            assert!(expand_word(word, &vars).is_err(), "{word}");
        }
    }

    #[test]
    fn test_join_path_composes_like_chdir() {
        assert_eq!(join_path("", "a"), "a");
        assert_eq!(join_path("/w", "a"), "/w/a");
        assert_eq!(join_path("/w/", "a"), "/w/a");
        assert_eq!(join_path("/w", "/abs"), "/abs");
        assert_eq!(join_path("/w", ""), "/w");
    }
}
